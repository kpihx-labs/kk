//! Durable PUBLIC Raft log, distinct from every private MLS database. IO runs on
//! blocking workers; the flush callback occurs only after SQLite FULL commit.
use super::{PeerId, TypeConfig};
use crate::rooms::{database, lease};
use crate::{Error, Result, config};
use openraft::storage::{LogFlushed, LogState, RaftLogStorage};
use openraft::{
    AnyError, Entry, ErrorSubject, ErrorVerb, LogId, RaftLogReader, StorageError, StorageIOError,
    Vote,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fmt::Debug,
    fs::File,
    ops::{Bound, RangeBounds},
    path::Path,
    sync::{Arc, Mutex},
};

type Stored<T> = std::result::Result<T, StorageError<PeerId>>;
pub(super) type Internal<T> = std::result::Result<T, Box<StorageError<PeerId>>>;

pub(super) struct Database {
    pub connection: Connection,
    poisoned: bool,
    _lease: File,
    // Last field: notify only AFTER SQLite and its exclusive lease are dropped.
    _released: Released,
}
struct Released(tokio::sync::watch::Sender<bool>);
impl Drop for Released {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}
#[derive(Clone)]
pub struct LogStore {
    inner: Arc<Mutex<Database>>,
    released: tokio::sync::watch::Receiver<bool>,
}

pub(super) fn failure(verb: ErrorVerb, message: &'static str) -> Box<StorageError<PeerId>> {
    Box::new(StorageIOError::new(ErrorSubject::Store, verb, AnyError::error(message)).into())
}

impl LogStore {
    pub fn create(directory: &Path) -> Result<Self> {
        Self::open_file(directory, true)
    }
    pub fn open(directory: &Path) -> Result<Self> {
        Self::open_file(directory, false)
    }
    fn open_file(directory: &Path, create: bool) -> Result<Self> {
        let lease = lease::Lease::open(
            directory,
            "control.sqlite3",
            create,
            config::ROOM_CONTROL_DATABASE_BYTES,
        )?;
        let connection = Connection::open_with_flags(
            lease.path(),
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_NOFOLLOW
                | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
        )
        .map_err(|_| Error::Storage("cannot open public control database"))?;
        lease.verify_path()?;
        if !create {
            let version: i64 = connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(|_| Error::Storage("cannot read control schema version"))?;
            if version != 2 {
                return Err(Error::Storage(
                    "unknown control schema; explicit recovery required",
                ));
            }
            connection
                .prepare("SELECT idx,entry FROM kk_control_logs LIMIT 0")
                .map_err(|_| Error::Storage("control log table missing"))?;
            connection
                .prepare("SELECT name,value FROM kk_control_meta LIMIT 0")
                .map_err(|_| Error::Storage("control metadata table missing"))?;
        }
        database::configure(
            &connection,
            config::ROOM_CONTROL_DATABASE_BYTES,
            config::ROOM_CONTROL_DATABASE_VALUE_BYTES,
        )?;
        if create {
            connection.execute_batch("CREATE TABLE kk_control_logs(idx BLOB PRIMARY KEY CHECK(length(idx)=8),entry BLOB NOT NULL);
                CREATE TABLE kk_control_meta(name TEXT PRIMARY KEY,value BLOB NOT NULL); PRAGMA user_version=2;")
                .map_err(|_| Error::Storage("cannot initialize public control database"))?;
            lease::sync_creation(directory)?;
        }
        let (released, receiver) = tokio::sync::watch::channel(false);
        Ok(Self {
            inner: Arc::new(Mutex::new(Database {
                connection,
                poisoned: false,
                _lease: lease.into_file(),
                _released: Released(released),
            })),
            released: receiver,
        })
    }
    pub(super) fn released(&self) -> tokio::sync::watch::Receiver<bool> {
        self.released.clone()
    }
    pub(super) async fn io<T: Send + 'static>(
        &self,
        verb: ErrorVerb,
        operation: impl FnOnce(&mut Database) -> Internal<T> + Send + 'static,
    ) -> Internal<T> {
        let shared = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let mut database = shared
                .lock()
                .map_err(|_| failure(verb, "control database lock poisoned"))?;
            if database.poisoned {
                return Err(failure(verb, "uncertain control commit; reopen and verify"));
            }
            operation(&mut database)
        })
        .await
        .map_err(|_| failure(verb, "control IO worker failed"))?
    }
}

pub(super) fn metadata<T: DeserializeOwned>(
    connection: &Connection,
    name: &str,
) -> Internal<Option<T>> {
    blob(connection, name)?
        .map(|value| decode(&value))
        .transpose()
}
pub(super) fn blob(connection: &Connection, name: &str) -> Internal<Option<Vec<u8>>> {
    let bytes: Option<Vec<u8>> = connection
        .query_row(
            "SELECT value FROM kk_control_meta WHERE name=?1",
            [name],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| failure(ErrorVerb::Read, "cannot read control metadata"))?;
    if bytes
        .as_ref()
        .is_some_and(|value| value.len() > config::ROOM_CONTROL_DATABASE_VALUE_BYTES)
    {
        return Err(failure(ErrorVerb::Read, "control metadata exceeds limit"));
    }
    Ok(bytes)
}
pub(super) fn put(connection: &Connection, name: &str, value: &impl Serialize) -> Internal<()> {
    let bytes = encode(value)?;
    put_blob(connection, name, &bytes)
}
pub(super) fn put_blob(connection: &Connection, name: &str, bytes: &[u8]) -> Internal<()> {
    if bytes.len() > config::ROOM_CONTROL_DATABASE_VALUE_BYTES {
        return Err(failure(ErrorVerb::Write, "control metadata exceeds limit"));
    }
    connection
        .execute(
            "INSERT INTO kk_control_meta(name,value) VALUES(?1,?2)
        ON CONFLICT(name) DO UPDATE SET value=excluded.value",
            rusqlite::params![name, bytes],
        )
        .map_err(|_| failure(ErrorVerb::Write, "cannot save control metadata"))?;
    Ok(())
}
pub(super) fn encode(value: &impl Serialize) -> Internal<Vec<u8>> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| failure(ErrorVerb::Write, "cannot encode public control record"))?;
    if bytes.len() > config::ROOM_CONTROL_DATABASE_VALUE_BYTES {
        return Err(failure(
            ErrorVerb::Write,
            "control record exceeds storage limit",
        ));
    }
    Ok(bytes)
}
pub(super) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Internal<T> {
    if bytes.len() > config::ROOM_CONTROL_DATABASE_VALUE_BYTES {
        return Err(failure(
            ErrorVerb::Read,
            "control record exceeds storage limit",
        ));
    }
    serde_json::from_slice(bytes)
        .map_err(|_| failure(ErrorVerb::Read, "invalid public control record"))
}
pub(super) fn write<T>(
    database: &mut Database,
    operation: impl FnOnce(&Connection) -> Internal<T>,
) -> Internal<T> {
    let transaction = database
        .connection
        .transaction()
        .map_err(|_| failure(ErrorVerb::Write, "cannot begin control transaction"))?;
    match operation(&transaction) {
        Ok(value) => match transaction.commit() {
            Ok(()) => Ok(value),
            Err(_) => {
                database.poisoned = true;
                Err(failure(
                    ErrorVerb::Write,
                    "control commit failed; recovery required",
                ))
            }
        },
        Err(error) => {
            if transaction.rollback().is_err() {
                database.poisoned = true;
            }
            Err(error)
        }
    }
}

impl RaftLogReader<TypeConfig> for LogStore {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        range: RB,
    ) -> Stored<Vec<Entry<TypeConfig>>> {
        let start = match range.start_bound() {
            Bound::Included(value) => *value,
            Bound::Excluded(value) => match value.checked_add(1) {
                Some(value) => value,
                None => return Ok(Vec::new()),
            },
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(value) => value.checked_add(1),
            Bound::Excluded(value) => Some(*value),
            Bound::Unbounded => None,
        };
        if end.is_some_and(|end| end <= start) {
            return Ok(Vec::new());
        }
        self.io(ErrorVerb::Read, move |database| {
            let mut statement = database.connection.prepare("SELECT entry FROM kk_control_logs WHERE idx>=?1 AND (?2 IS NULL OR idx<?2) ORDER BY idx")
                .map_err(|_| failure(ErrorVerb::Read, "cannot prepare control log read"))?;
            let rows = statement.query_map(rusqlite::params![start.to_be_bytes().to_vec(), end.map(|value| value.to_be_bytes().to_vec())], |row| row.get::<_, Vec<u8>>(0))
                .map_err(|_| failure(ErrorVerb::Read, "cannot read control logs"))?;
            let mut entries = Vec::new();
            for row in rows { entries.push(decode(&row.map_err(|_| failure(ErrorVerb::Read, "invalid control log row"))?)?); }
            Ok(entries)
        }).await.map_err(|error| *error)
    }
}
impl RaftLogStorage<TypeConfig> for LogStore {
    type LogReader = Self;
    async fn get_log_state(&mut self) -> Stored<LogState<TypeConfig>> {
        self.io(ErrorVerb::Read, |database| {
            let purged: Option<LogId<PeerId>> = metadata(&database.connection, "purged")?;
            let latest: Option<Vec<u8>> = database
                .connection
                .query_row(
                    "SELECT entry FROM kk_control_logs ORDER BY idx DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|_| failure(ErrorVerb::Read, "cannot read last control log"))?;
            let last = latest
                .map(|bytes| decode::<Entry<TypeConfig>>(&bytes).map(|entry| entry.log_id))
                .transpose()?
                .or(purged);
            Ok(LogState {
                last_purged_log_id: purged,
                last_log_id: last,
            })
        })
        .await
        .map_err(|error| *error)
    }
    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }
    async fn save_vote(&mut self, vote: &Vote<PeerId>) -> Stored<()> {
        let vote = *vote;
        self.io(ErrorVerb::Write, move |database| {
            write(database, |connection| put(connection, "vote", &vote))
        })
        .await
        .map_err(|error| *error)
    }
    async fn read_vote(&mut self) -> Stored<Option<Vote<PeerId>>> {
        self.io(ErrorVerb::Read, |database| {
            metadata(&database.connection, "vote")
        })
        .await
        .map_err(|error| *error)
    }
    async fn save_committed(&mut self, committed: Option<LogId<PeerId>>) -> Stored<()> {
        self.io(ErrorVerb::Write, move |database| {
            write(database, |connection| {
                put(connection, "committed", &committed)
            })
        })
        .await
        .map_err(|error| *error)
    }
    async fn read_committed(&mut self) -> Stored<Option<LogId<PeerId>>> {
        self.io(ErrorVerb::Read, |database| {
            Ok(metadata::<Option<LogId<PeerId>>>(&database.connection, "committed")?.flatten())
        })
        .await
        .map_err(|error| *error)
    }
    async fn append<I>(&mut self, entries: I, callback: LogFlushed<TypeConfig>) -> Stored<()>
    where
        I: IntoIterator<Item = Entry<TypeConfig>> + Send,
        I::IntoIter: Send,
    {
        let entries: Vec<_> = entries
            .into_iter()
            .take(config::ROOM_CONTROL_LOG_BATCH + 1)
            .collect();
        let result = if entries.len() > config::ROOM_CONTROL_LOG_BATCH {
            Err(failure(
                ErrorVerb::Write,
                "control log batch exceeds resource limit",
            ))
        } else {
            self.io(ErrorVerb::Write, move |database| write(database, |connection| {
                // Raft owns sequence/conflict validation. Storage must atomically
                // persist the supplied batch, never partially introduce holes.
                for entry in entries {
                    let bytes = encode(&entry)?;
                    connection.execute("INSERT INTO kk_control_logs(idx,entry) VALUES(?1,?2) ON CONFLICT(idx) DO UPDATE SET entry=excluded.entry",
                        rusqlite::params![entry.log_id.index.to_be_bytes().to_vec(), bytes])
                        .map_err(|_| failure(ErrorVerb::Write, "cannot append public control log"))?;
                }
                Ok(())
            })).await
        };
        match &result {
            Ok(()) => callback.log_io_completed(Ok(())),
            Err(_) => callback.log_io_completed(Err(std::io::Error::other(
                "public control log flush failed",
            ))),
        }
        result.map_err(|error| *error)
    }
    async fn truncate(&mut self, log_id: LogId<PeerId>) -> Stored<()> {
        self.io(ErrorVerb::Write, move |database| {
            write(database, |connection| {
                connection
                    .execute(
                        "DELETE FROM kk_control_logs WHERE idx>=?1",
                        [log_id.index.to_be_bytes().to_vec()],
                    )
                    .map_err(|_| failure(ErrorVerb::Write, "cannot truncate control log suffix"))?;
                Ok(())
            })
        })
        .await
        .map_err(|error| *error)
    }
    async fn purge(&mut self, log_id: LogId<PeerId>) -> Stored<()> {
        self.io(ErrorVerb::Write, move |database| {
            write(database, |connection| {
                connection
                    .execute(
                        "DELETE FROM kk_control_logs WHERE idx<=?1",
                        [log_id.index.to_be_bytes().to_vec()],
                    )
                    .map_err(|_| failure(ErrorVerb::Write, "cannot compact public control log"))?;
                put(connection, "purged", &log_id)
            })
        })
        .await
        .map_err(|error| *error)
    }
}

#[cfg(all(test, unix))]
mod tests;
