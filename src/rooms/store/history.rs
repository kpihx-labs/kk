//! Opt-in, future-only encrypted history on the existing private-store connection.
//! No live capture, delivery claims, identity provisioning or plaintext SQL index.

use super::{Provider, Transaction};
use crate::{Error, Result, config, envelope::record};
use age::x25519::{Identity, Recipient};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::{
    io::{Cursor, Write},
    rc::Rc,
};
use zeroize::{Zeroize, Zeroizing};

pub(super) const SCHEMA_VERSION: i64 = 2;
pub const MAX_ROWS: usize = 256;
pub const MAX_ENCRYPTED_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_RETENTION_SECONDS: u64 = 7 * 24 * 60 * 60;
pub const MAX_QUERY_BYTES: usize = 256;
pub const MAX_RESULTS: usize = 32;
/// Includes the JSON page framing, metadata and escaped UTF-8 bodies.
pub const MAX_RESPONSE_BYTES: usize = config::MAX_RECORD_PLAINTEXT_BYTES;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub profile: [u8; 32],
    pub room: [u8; 32],
    pub recipient: String,
}
impl Binding {
    pub fn new(profile: [u8; 32], room: [u8; 32], recipient: &Recipient) -> Self {
        Self {
            profile,
            room,
            recipient: recipient.to_string(),
        }
    }
    fn validate(&self) -> Result<Recipient> {
        if self.recipient.len() > 128 {
            return Err(invalid());
        }
        let recipient: Recipient = self.recipient.parse().map_err(|_| invalid())?;
        if recipient.to_string() != self.recipient {
            return Err(invalid());
        }
        Ok(recipient)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub rows: usize,
    pub encrypted_bytes: usize,
    pub retention_seconds: u64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            rows: MAX_ROWS,
            encrypted_bytes: MAX_ENCRYPTED_BYTES,
            retention_seconds: MAX_RETENTION_SECONDS,
        }
    }
}
impl Policy {
    pub fn validate(self) -> Result<()> {
        if self.rows == 0
            || self.rows > MAX_ROWS
            || self.encrypted_bytes == 0
            || self.encrypted_bytes > MAX_ENCRYPTED_BYTES
            || self.retention_seconds == 0
            || self.retention_seconds > MAX_RETENTION_SECONDS
        {
            return Err(invalid());
        }
        Ok(())
    }
}

/// This is a storage fact, never a delivery/read receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    OutgoingPrepared,
    IncomingProcessed,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub version: u8,
    pub binding: Binding,
    pub id: u64,
    pub event: Event,
    pub epoch: u64,
    /// SHA-256 of the exact authenticated wire bytes, supplied by the caller.
    pub wire_sha256: [u8; 32],
    pub captured_at: u64,
    pub body: String,
}
impl Drop for Row {
    fn drop(&mut self) {
        self.body.zeroize();
    }
}

/// Borrowed input; the caller remains responsible for its original body allocation.
pub struct Capture<'a> {
    pub event: Event,
    pub epoch: u64,
    pub wire_sha256: [u8; 32],
    pub body: &'a str,
    pub sensitive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Off,
    /// Public metadata only; no rows/counters are presented as verified results.
    Locked {
        binding: Binding,
        enabled: bool,
        policy: Policy,
    },
    Configured {
        binding: Binding,
        enabled: bool,
        policy: Policy,
        rows: usize,
        encrypted_bytes: usize,
        next_id: u64,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recorded {
    Off,
    SensitiveExcluded,
    Stored { id: u64, event: Event },
}

pub struct Query<'a> {
    pub text: &'a str,
    pub after_id: u64,
    pub limit: usize,
    pub response_bytes: usize,
}
#[derive(Serialize)]
pub struct Page {
    pub rows: Vec<Row>,
    pub next_after: u64,
    pub has_more: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u8,
    binding: Binding,
    policy: Policy,
    enabled: bool,
    next_id: u64,
    rows: usize,
    bytes: usize,
    clock: u64,
}
fn invalid() -> Error {
    Error::Storage("invalid or substituted history state")
}
fn locked() -> Error {
    Error::Storage("history is locked; matching AGE identity required")
}
fn sql<T>(value: rusqlite::Result<T>) -> Result<T> {
    value.map_err(|_| Error::Storage("history database operation failed"))
}
fn version(connection: &Connection) -> Result<i64> {
    sql(connection.pragma_query_value(None, "user_version", |row| row.get(0)))
}
fn read_state(connection: &Connection) -> Result<(State, Vec<u8>)> {
    let (state_length, check_length): (i64, i64) = sql(connection.query_row(
        "SELECT length(state), length(key_check) FROM kk_history_state WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    if !(1..=4096).contains(&state_length)
        || check_length <= 0
        || check_length > config::MAX_RECORD_CIPHERTEXT_BYTES as i64
    {
        return Err(invalid());
    }
    let (bytes, check): (Vec<u8>, Vec<u8>) = sql(connection.query_row(
        "SELECT state, key_check FROM kk_history_state WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    ))?;
    if bytes.len() > 4096 || check.len() > config::MAX_RECORD_CIPHERTEXT_BYTES {
        return Err(invalid());
    }
    let state: State = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    state.policy.validate()?;
    state.binding.validate()?;
    if state.version != 1
        || state.next_id == 0
        || state.next_id > i64::MAX as u64
        || state.rows > state.policy.rows
        || state.bytes > state.policy.encrypted_bytes
    {
        return Err(invalid());
    }
    let (count, bytes, maximum): (i64, i64, i64) = sql(connection.query_row(
        "SELECT count(*), coalesce(sum(length(ciphertext)),0), coalesce(max(id),0) FROM (SELECT id,ciphertext FROM kk_history_rows ORDER BY id LIMIT 257)",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))))?;
    if count != state.rows as i64
        || bytes != state.bytes as i64
        || maximum < 0
        || maximum as u64 >= state.next_id
    {
        return Err(invalid());
    }
    Ok((state, check))
}
pub(super) fn validate_schema(connection: &Connection) -> Result<()> {
    read_state(connection)?;
    let count: i64 =
        sql(connection.query_row("SELECT count(*) FROM kk_history_state", [], |r| r.get(0)))?;
    if count != 1 {
        return Err(invalid());
    }
    Ok(())
}

impl Provider {
    fn history_transaction(&self, tx: &Transaction) -> Result<&Connection> {
        self.writable()?;
        if tx.finished || !Rc::ptr_eq(&self.session.0, &tx.session.0) {
            return Err(invalid());
        }
        Ok(&self.session.0.connection)
    }
    // A savepoint gives each SDK operation error atomicity without owning/committing
    // another connection or the caller's MLS transaction.
    fn history_mutate<T>(
        &self,
        tx: &Transaction,
        f: impl FnOnce(&Connection) -> Result<T>,
    ) -> Result<T> {
        let connection = self.history_transaction(tx)?;
        sql(connection.execute_batch("SAVEPOINT kk_history_operation"))?;
        match f(connection) {
            Ok(value) => {
                if connection
                    .execute_batch("RELEASE kk_history_operation")
                    .is_err()
                {
                    self.session.0.poisoned.set(true);
                    return Err(invalid());
                }
                Ok(value)
            }
            Err(error) => {
                if connection
                    .execute_batch("ROLLBACK TO kk_history_operation; RELEASE kk_history_operation")
                    .is_err()
                {
                    self.session.0.poisoned.set(true);
                }
                Err(error)
            }
        }
    }

    /// The sole migration entry point. V1 remains untouched until this is requested.
    pub fn history_enable(
        &self,
        tx: &Transaction,
        binding: &Binding,
        identity: &Identity,
        policy: Policy,
        now: u64,
    ) -> Result<()> {
        policy.validate()?;
        let recipient = binding.validate()?;
        if identity.to_public().to_string() != binding.recipient {
            return Err(locked());
        }
        self.history_mutate(tx, |connection| {
            match version(connection)? {
                1 => {
                    sql(connection.execute_batch("CREATE TABLE kk_history_state (
                        singleton INTEGER PRIMARY KEY CHECK(singleton=1), state BLOB NOT NULL, key_check BLOB NOT NULL);
                        CREATE TABLE kk_history_rows (id INTEGER PRIMARY KEY CHECK(id>0), ciphertext BLOB NOT NULL);
                        PRAGMA user_version=2;"))?;
                    let state = State { version: 1, binding: binding.clone(), policy, enabled: true,
                        next_id: 1, rows: 0, bytes: 0, clock: now };
                    let check = record::seal(Zeroizing::new(serde_json::to_vec(binding).map_err(|_| invalid())?), &recipient)?;
                    let state = serde_json::to_vec(&state).map_err(|_| invalid())?;
                    sql(connection.execute("INSERT INTO kk_history_state VALUES(1,?1,?2)", params![state, check]))?;
                }
                SCHEMA_VERSION => {
                    let (mut state, rows) = unlocked(connection, binding, Some(identity), now)?;
                    state.policy = policy;
                    state.enabled = true;
                    retain(connection, &mut state, rows, now, 0)?;
                    save_state(connection, &state)?;
                }
                _ => return Err(invalid()),
            }
            Ok(())
        })
    }

    /// Missing keys never become empty results, even after disable or clear.
    /// A successful keyed observation advances the clock in `tx`; commit before
    /// exposing results. Rollback discards both the observation and its clock.
    pub fn history_status(
        &self,
        tx: &Transaction,
        expected_context: Option<&Binding>,
        identity: Option<&Identity>,
        now: u64,
    ) -> Result<Status> {
        let connection = self.history_transaction(tx)?;
        if version(connection)? == 1 {
            return Ok(Status::Off);
        }
        if version(connection)? != SCHEMA_VERSION {
            return Err(invalid());
        }
        let (public, _) = read_state(connection)?;
        if expected_context.is_some_and(|binding| binding != &public.binding) {
            return Err(invalid());
        }
        if identity.is_none_or(|key| key.to_public().to_string() != public.binding.recipient) {
            return Ok(Status::Locked {
                binding: public.binding,
                enabled: public.enabled,
                policy: public.policy,
            });
        }
        let (state, _) = unlocked(connection, &public.binding, identity, now)?;
        save_state(connection, &state)?;
        Ok(Status::Configured {
            binding: state.binding,
            enabled: state.enabled,
            policy: state.policy,
            rows: state.rows,
            encrypted_bytes: state.bytes,
            next_id: state.next_id,
        })
    }

    pub fn history_disable(
        &self,
        tx: &Transaction,
        binding: &Binding,
        identity: &Identity,
        now: u64,
    ) -> Result<()> {
        self.history_mutate(tx, |connection| {
            if version(connection)? == 1 {
                return Ok(());
            }
            let (mut state, _) = unlocked(connection, binding, Some(identity), now)?;
            state.enabled = false;
            save_state(connection, &state)
        })
    }

    /// Explicit removal, not key destruction or forensic erasure. Sequence survives.
    pub fn history_clear(
        &self,
        tx: &Transaction,
        binding: &Binding,
        identity: &Identity,
        now: u64,
    ) -> Result<()> {
        self.history_mutate(tx, |connection| {
            if version(connection)? == 1 {
                return Ok(());
            }
            let (mut state, _) = unlocked(connection, binding, Some(identity), now)?;
            sql(connection.execute("DELETE FROM kk_history_rows", []))?;
            state.rows = 0;
            state.bytes = 0;
            save_state(connection, &state)
        })
    }

    pub fn history_record(
        &self,
        tx: &Transaction,
        binding: &Binding,
        identity: Option<&Identity>,
        capture: Capture<'_>,
        now: u64,
    ) -> Result<Recorded> {
        self.history_mutate(tx, |connection| {
            if version(connection)? == 1 {
                return Ok(Recorded::Off);
            }
            let (mut state, rows) = unlocked(connection, binding, identity, now)?;
            if !state.enabled {
                return Ok(Recorded::Off);
            }
            if capture.sensitive {
                return Ok(Recorded::SensitiveExcluded);
            }
            if capture.body.len() > config::MAX_MESSAGE_BYTES {
                return Err(invalid());
            }
            let id = state.next_id;
            state.next_id = id
                .checked_add(1)
                .filter(|id| *id <= i64::MAX as u64)
                .ok_or_else(invalid)?;
            let row = Row {
                version: 1,
                binding: binding.clone(),
                id,
                event: capture.event,
                epoch: capture.epoch,
                wire_sha256: capture.wire_sha256,
                captured_at: now,
                body: capture.body.to_owned(),
            };
            let plaintext = private_json(&row)?;
            let ciphertext = record::seal(plaintext, &binding.validate()?)?;
            if ciphertext.len() > state.policy.encrypted_bytes {
                return Err(invalid());
            }
            retain(connection, &mut state, rows, now, ciphertext.len())?;
            state.rows = state.rows.checked_add(1).ok_or_else(invalid)?;
            state.bytes = state
                .bytes
                .checked_add(ciphertext.len())
                .ok_or_else(invalid)?;
            sql(connection.execute(
                "INSERT INTO kk_history_rows(id,ciphertext) VALUES(?1,?2)",
                params![id as i64, ciphertext],
            ))?;
            save_state(connection, &state)?;
            Ok(Recorded::Stored {
                id,
                event: capture.event,
            })
        })
    }

    pub fn history_prune(
        &self,
        tx: &Transaction,
        binding: &Binding,
        identity: &Identity,
        now: u64,
    ) -> Result<()> {
        self.history_mutate(tx, |connection| {
            if version(connection)? == 1 {
                return Ok(());
            }
            let (mut state, rows) = unlocked(connection, binding, Some(identity), now)?;
            retain(connection, &mut state, rows, now, 0)?;
            save_state(connection, &state)
        })
    }

    /// Literal, case-sensitive UTF-8 substring search. Bounded full decrypt scan;
    /// pagination never hides corrupt/substituted rows beyond the returned page.
    /// Successful search advances the durable clock in `tx`, including empty
    /// expired pages. Commit before exposing the page; rollback discards it.
    pub fn history_search(
        &self,
        tx: &Transaction,
        binding: &Binding,
        identity: Option<&Identity>,
        query: Query<'_>,
        now: u64,
    ) -> Result<Page> {
        let connection = self.history_transaction(tx)?;
        if query.text.len() > MAX_QUERY_BYTES
            || query.limit == 0
            || query.limit > MAX_RESULTS
            || query.response_bytes == 0
            || query.response_bytes > MAX_RESPONSE_BYTES
        {
            return Err(invalid());
        }
        let mut page = Page {
            rows: Vec::new(),
            next_after: query.after_id,
            has_more: false,
        };
        if version(connection)? == 1 {
            if !page_fits(&page, query.response_bytes)? {
                return Err(invalid());
            }
            return Ok(page);
        }
        let (state, rows) = unlocked(connection, binding, identity, now)?;
        for (row, _) in rows {
            if row.id <= query.after_id
                || expired(&row, state.policy, now)
                || !row.body.contains(query.text)
            {
                continue;
            }
            if page.rows.len() == query.limit {
                page.has_more = true;
                break;
            }
            let previous = page.next_after;
            page.next_after = row.id;
            page.rows.push(row);
            // false is one byte longer than true in JSON: reserve the larger form.
            page.has_more = false;
            if !page_fits(&page, query.response_bytes)? {
                page.rows.pop();
                page.next_after = previous;
                page.has_more = true;
                if page.rows.is_empty() {
                    return Err(Error::Storage("history response budget cannot fit a row"));
                }
                save_state(connection, &state)?;
                return Ok(page);
            }
            page.has_more = false;
        }
        if !page_fits(&page, query.response_bytes)? {
            return Err(invalid());
        }
        save_state(connection, &state)?;
        Ok(page)
    }

    #[cfg(all(test, unix))]
    pub(super) fn inject_history_cleanup_failure(&self, tx: &Transaction) -> Result<()> {
        self.history_mutate(tx, |connection| {
            sql(connection.execute("DELETE FROM kk_history_rows", []))?;
            // Deterministically lose the operation savepoint, but not the outer
            // transaction. The real ROLLBACK TO cleanup then fails in SQLite.
            sql(connection.execute_batch("RELEASE kk_history_operation"))?;
            Err::<(), _>(invalid())
        })
    }
}

fn unlocked(
    connection: &Connection,
    binding: &Binding,
    identity: Option<&Identity>,
    now: u64,
) -> Result<(State, Vec<(Row, usize)>)> {
    if version(connection)? != SCHEMA_VERSION {
        return Err(invalid());
    }
    let (mut state, check) = read_state(connection)?;
    if &state.binding != binding || now < state.clock {
        return Err(invalid());
    }
    let identity = identity.ok_or_else(locked)?;
    if identity.to_public().to_string() != binding.recipient {
        return Err(locked());
    }
    let plaintext = record::open(&check, identity).map_err(|_| locked())?;
    let checked: Binding = serde_json::from_slice(&plaintext).map_err(|_| invalid())?;
    if checked != *binding {
        return Err(invalid());
    }
    let mut statement =
        sql(connection.prepare("SELECT id,ciphertext FROM kk_history_rows ORDER BY id LIMIT 257"))?;
    let mut cursor = sql(statement.query([]))?;
    let mut rows = Vec::with_capacity(state.rows);
    while let Some(entry) = sql(cursor.next())? {
        if rows.len() >= MAX_ROWS {
            return Err(invalid());
        }
        let id: i64 = sql(entry.get(0))?;
        let bytes: Vec<u8> = sql(entry.get(1))?;
        if bytes.len() > config::MAX_RECORD_CIPHERTEXT_BYTES {
            return Err(invalid());
        }
        let plaintext = record::open(&bytes, identity)?;
        let row: Row = serde_json::from_slice(&plaintext).map_err(|_| invalid())?;
        if id <= 0
            || row.version != 1
            || row.id != id as u64
            || row.binding != *binding
            || row.captured_at > state.clock
            || row.body.len() > config::MAX_MESSAGE_BYTES
        {
            return Err(invalid());
        }
        rows.push((row, bytes.len()));
    }
    if rows.len() != state.rows {
        return Err(invalid());
    }
    state.clock = now;
    Ok((state, rows))
}
fn expired(row: &Row, policy: Policy, now: u64) -> bool {
    now.saturating_sub(row.captured_at) >= policy.retention_seconds
}
fn retain(
    connection: &Connection,
    state: &mut State,
    rows: Vec<(Row, usize)>,
    now: u64,
    reserve: usize,
) -> Result<()> {
    for (row, length) in rows {
        let count_full = if reserve == 0 {
            state.rows > state.policy.rows
        } else {
            state.rows >= state.policy.rows
        };
        let bytes_full =
            state.bytes.checked_add(reserve).ok_or_else(invalid)? > state.policy.encrypted_bytes;
        if expired(&row, state.policy, now) || count_full || bytes_full {
            if sql(connection.execute("DELETE FROM kk_history_rows WHERE id=?1", [row.id as i64]))?
                != 1
            {
                return Err(invalid());
            }
            state.rows = state.rows.checked_sub(1).ok_or_else(invalid)?;
            state.bytes = state.bytes.checked_sub(length).ok_or_else(invalid)?;
        }
    }
    Ok(())
}
fn save_state(connection: &Connection, state: &State) -> Result<()> {
    let bytes = serde_json::to_vec(state).map_err(|_| invalid())?;
    if sql(connection.execute(
        "UPDATE kk_history_state SET state=?1 WHERE singleton=1",
        [bytes],
    ))? != 1
    {
        return Err(invalid());
    }
    Ok(())
}

// Serialize into one fixed, zeroizing allocation; Vec growth must not leave old
// private JSON body allocations behind. Cursor's slice bounds are the hard cap.
fn private_json(value: &impl Serialize) -> Result<Zeroizing<Vec<u8>>> {
    let mut bytes = Zeroizing::new(vec![0; config::MAX_RECORD_PLAINTEXT_BYTES]);
    let mut cursor = Cursor::new(bytes.as_mut_slice());
    serde_json::to_writer(&mut cursor, value).map_err(|_| invalid())?;
    let length = cursor.position() as usize;
    bytes.truncate(length);
    Ok(bytes)
}

// Count actual escaped JSON bytes without allocating another plaintext page.
fn page_fits(page: &Page, maximum: usize) -> Result<bool> {
    struct Counter {
        length: usize,
        maximum: usize,
        exceeded: bool,
    }
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.maximum.saturating_sub(self.length) {
                self.exceeded = true;
                return Err(std::io::Error::other("history response bound"));
            }
            self.length += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter {
        length: 0,
        maximum,
        exceeded: false,
    };
    match serde_json::to_writer(&mut counter, page) {
        Ok(()) => Ok(true),
        Err(_) if counter.exceeded => Ok(false),
        Err(_) => Err(invalid()),
    }
}
