//! Durable PUBLIC state machine and bounded snapshots. No private MLS provider,
//! device signer or ratchet enters this type or its serialized state.
use super::log::Internal;
use super::log::{blob, decode, encode, failure, metadata, put, put_blob, write};
use super::records::{self, Change, CommittedRecord, Position, StoredRecord};
use super::{ApplyResult, LogStore, PeerId, SnapshotBuffer, TypeConfig};
use crate::{
    config,
    rooms::{
        Anchor,
        control::{Snapshot as PublicSnapshot, View},
        mls::PublicIdentity,
    },
};
use openraft::storage::{RaftSnapshotBuilder, RaftStateMachine};
use openraft::{
    Entry, EntryPayload, ErrorVerb, LogId, Snapshot, SnapshotMeta, StorageError, StoredMembership,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

type Stored<T> = Result<T, StorageError<PeerId>>;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u8,
    origin: PublicIdentity,
    applied: Option<LogId<PeerId>>,
    membership: StoredMembership<PeerId, super::Node>,
    view: PublicSnapshot,
    #[serde(default)]
    receipts: Vec<Receipt>,
    #[serde(default)]
    position: Option<Position>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    id: [u8; 32],
    wire: [u8; 32],
    result: ApplyResult,
}

#[derive(Clone)]
pub struct StateMachine {
    store: LogStore,
    anchor: Anchor,
    changes: tokio::sync::watch::Sender<u64>,
}
pub(super) struct TransportScope {
    pub applied: Option<LogId<PeerId>>,
    pub epoch: u64,
    pub members: Vec<crate::Peer>,
    pub nodes: std::collections::BTreeMap<PeerId, super::Node>,
}
impl StateMachine {
    fn owner(store: LogStore, anchor: &Anchor) -> Self {
        let (changes, _) = tokio::sync::watch::channel(0);
        Self {
            store,
            anchor: anchor.clone(),
            changes,
        }
    }
    pub(super) fn subscribe_scope(&self) -> tokio::sync::watch::Receiver<u64> {
        self.changes.subscribe()
    }
    fn changed(&self) {
        self.changes
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }
    pub(super) async fn transport_scope(&self) -> Internal<TransportScope> {
        let root = self.anchor.clone();
        self.store
            .io(ErrorVerb::Read, move |database| {
                let state = read_state(&database.connection)?;
                let view = checked_view(&root, &state)?;
                Ok(TransportScope {
                    applied: state.applied,
                    epoch: view.epoch(),
                    members: view.peers(),
                    nodes: state
                        .membership
                        .membership()
                        .nodes()
                        .map(|(id, node)| (*id, node.clone()))
                        .collect(),
                })
            })
            .await
    }
    pub(super) fn room_id(&self) -> [u8; 32] {
        self.anchor.id()
    }
    pub(super) async fn configuration(&self) -> Internal<StoredMembership<PeerId, super::Node>> {
        self.store
            .io(ErrorVerb::Read, move |database| {
                Ok(read_state(&database.connection)?.membership)
            })
            .await
    }
    pub async fn create(
        store: LogStore,
        anchor: &Anchor,
        creator: PublicIdentity,
    ) -> Internal<Self> {
        let root = anchor.clone();
        store
            .io(ErrorVerb::Write, move |database| {
                write(database, |connection| {
                    if metadata::<State>(connection, "machine")?.is_some() {
                        return Err(failure(ErrorVerb::Write, "public machine already exists"));
                    }
                    let view = View::genesis(&root, creator.clone())
                        .map_err(|_| failure(ErrorVerb::Write, "invalid room genesis"))?;
                    put(
                        connection,
                        "machine",
                        &State {
                            version: 2,
                            origin: creator,
                            applied: None,
                            membership: StoredMembership::default(),
                            view: view.snapshot(),
                            receipts: Vec::new(),
                            position: None,
                        },
                    )
                })
            })
            .await?;
        Ok(Self::owner(store, anchor))
    }
    pub async fn open(store: LogStore, anchor: &Anchor) -> Internal<Self> {
        let root = anchor.clone();
        store
            .io(ErrorVerb::Read, move |database| {
                let state = read_state(&database.connection)?;
                if state.applied.is_some() && state.position.is_none() {
                    return Err(failure(
                        ErrorVerb::Read,
                        "legacy public state has no private catch-up baseline",
                    ));
                }
                checked_view(&root, &state)?;
                Ok(())
            })
            .await?;
        Ok(Self::owner(store, anchor))
    }
    pub async fn public_view(&self) -> Internal<View> {
        let root = self.anchor.clone();
        self.store
            .io(ErrorVerb::Read, move |database| {
                checked_view(&root, &read_state(&database.connection)?)
            })
            .await
    }
    /// Immutable PUBLIC genesis proof, retained after creator departure and log
    /// compaction. It is not a current-membership or live-creator claim.
    pub async fn public_origin(&self) -> Internal<PublicIdentity> {
        let root = self.anchor.clone();
        self.store
            .io(ErrorVerb::Read, move |database| {
                let state = read_state(&database.connection)?;
                checked_origin(&root, &state)?;
                Ok(state.origin)
            })
            .await
    }
    /// Find the actual admission in retained protected history, not a client DTO
    /// or a receipt's historical epoch. Genesis need not still be retained.
    pub async fn committed_welcome(
        &self,
        recipient: PeerId,
    ) -> Internal<Option<(super::super::Welcome, CommittedRecord)>> {
        let root = self.anchor.clone();
        self.store
            .io(ErrorVerb::Read, move |database| {
                let state = read_state(&database.connection)?;
                let view = checked_view(&root, &state)?;
                let Some(welcome) = view.welcome_for(&recipient.0) else {
                    return Ok(None);
                };
                let journal = read_records(&database.connection, &state, root.id())?;
                let record = journal
                    .into_iter()
                    .map(CommittedRecord)
                    .find(|record| record.proposal_id() == Some(welcome.proposal))
                    .ok_or_else(|| {
                        failure(
                            ErrorVerb::Read,
                            "Welcome admission bridge missing; verified rejoin required",
                        )
                    })?;
                if record.epoch() != welcome.epoch
                    || record.admitted_peer() != Some(recipient)
                    || record
                        .change()
                        .is_none_or(|(author, _, accepted)| !accepted || author != welcome.author)
                {
                    return Err(failure(
                        ErrorVerb::Read,
                        "committed Welcome and admission disagree",
                    ));
                }
                Ok(Some((welcome, record)))
            })
            .await
    }
    pub async fn committed_after(
        &self,
        cursor: Option<Position>,
    ) -> Internal<Vec<CommittedRecord>> {
        self.committed_state_after(cursor)
            .await
            .map(|(_, records)| records)
    }
    pub async fn committed_state_after(
        &self,
        cursor: Option<Position>,
    ) -> Internal<(View, Vec<CommittedRecord>)> {
        let root = self.anchor.clone();
        self.store
            .io(ErrorVerb::Read, move |database| {
                let state = read_state(&database.connection)?;
                let journal = read_records(&database.connection, &state, root.id())?;
                Ok((
                    checked_view(&root, &state)?,
                    records::after(journal, cursor, state.position)?,
                ))
            })
            .await
    }
}
fn read_records(
    connection: &rusqlite::Connection,
    state: &State,
    room: [u8; 32],
) -> Internal<Vec<StoredRecord>> {
    let journal: Vec<StoredRecord> = metadata(connection, "applied_journal")?.unwrap_or_default();
    records::validate(&journal, room, state.position)?;
    Ok(journal)
}

fn read_state(connection: &rusqlite::Connection) -> Internal<State> {
    let state = metadata::<State>(connection, "machine")?
        .ok_or_else(|| failure(ErrorVerb::Read, "public machine state is missing"))?;
    checked_header(&state)?;
    Ok(state)
}
fn checked_header(state: &State) -> Internal<()> {
    if state.version != 2 {
        return Err(failure(ErrorVerb::Read, "unknown public machine version"));
    }
    if state
        .position
        .is_some_and(|position| Some(position.log) != state.applied)
    {
        return Err(failure(
            ErrorVerb::Read,
            "applied position and log disagree",
        ));
    }
    Ok(())
}
fn checked_view(anchor: &Anchor, state: &State) -> Internal<View> {
    checked_header(state)?;
    for (id, node) in state.membership.membership().nodes() {
        if !node.certificate.is_empty() {
            node.peer(*id)
                .map_err(|_| failure(ErrorVerb::Read, "invalid scoped control certificate"))?;
        }
    }
    checked_origin(anchor, state)?;
    if state.receipts.len() > config::ROOM_CONTROL_RECEIPTS
        || state
            .receipts
            .iter()
            .map(|receipt| receipt.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != state.receipts.len()
        || state.receipts.iter().any(|receipt| {
            !receipt.result.accepted
                || receipt.result.replayed
                || receipt.result.first_log.is_none()
                || receipt.result.epoch > state.view.epoch()
        })
    {
        return Err(failure(ErrorVerb::Read, "invalid public result history"));
    }
    let digest = state
        .view
        .digest()
        .map_err(|_| failure(ErrorVerb::Read, "invalid public state encoding"))?;
    // Protected local state or a verified Raft snapshot caller supplies authority.
    // This structural check does not promote a self-supplied hash to a quorum.
    View::restore_verified(anchor, state.view.clone(), state.view.head(), digest)
        .map_err(|_| failure(ErrorVerb::Read, "invalid public room state"))
}
fn checked_origin(anchor: &Anchor, state: &State) -> Internal<()> {
    let member = state
        .origin
        .verify(anchor)
        .map_err(|_| failure(ErrorVerb::Read, "invalid public genesis origin"))?;
    if member.grant_id() != anchor.id()
        || member.peer().fingerprint_bytes() != anchor.creator.fingerprint_bytes()
    {
        return Err(failure(
            ErrorVerb::Read,
            "genesis origin does not match root",
        ));
    }
    Ok(())
}

fn apply_proposal(
    view: &mut View,
    receipts: &mut Vec<Receipt>,
    proposal: super::super::Proposal,
    log_id: LogId<PeerId>,
) -> ApplyResult {
    let rejected = |code: &str, view: &View| ApplyResult {
        accepted: false,
        epoch: view.epoch(),
        head: view.head(),
        error: Some(code.to_owned()),
        replayed: false,
        first_log: Some(log_id),
    };
    let Ok(id) = proposal.id() else {
        return rejected("PROTOCOL", view);
    };
    let Ok(encoded) = proposal.encode() else {
        return rejected("PROTOCOL", view);
    };
    let wire: [u8; 32] = Sha256::digest(encoded).into();
    if let Some(prior) = receipts.iter().find(|receipt| receipt.id == id) {
        // Exact previously authenticated bytes only. A changed signature/body
        // sharing a header ID cannot obtain a success receipt without verification.
        if prior.wire != wire {
            return rejected("AUTHENTICATION", view);
        }
        let mut result = prior.result.clone();
        result.replayed = true;
        return result;
    }
    if receipts.len() >= config::ROOM_CONTROL_RECEIPTS {
        return rejected("LIMIT", view);
    }
    if let Err(error) = view.apply_accepted(&proposal) {
        return rejected(error.code(), view);
    }
    let result = ApplyResult {
        accepted: true,
        epoch: view.epoch(),
        head: view.head(),
        error: None,
        replayed: false,
        first_log: Some(log_id),
    };
    receipts.push(Receipt {
        id,
        wire,
        result: result.clone(),
    });
    result
}
fn snapshot_id(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut identifier = String::from("public-");
    for byte in Sha256::digest(bytes) {
        write!(identifier, "{byte:02x}").expect("writing into a String");
    }
    identifier
}
fn bounded_state(bytes: &[u8]) -> Internal<State> {
    if bytes.len() > config::ROOM_CONTROL_SNAPSHOT_BYTES {
        return Err(failure(
            ErrorVerb::Read,
            "public snapshot exceeds aggregate limit",
        ));
    }
    decode(bytes)
}

impl RaftStateMachine<TypeConfig> for StateMachine {
    type SnapshotBuilder = Self;
    async fn applied_state(
        &mut self,
    ) -> Stored<(Option<LogId<PeerId>>, StoredMembership<PeerId, super::Node>)> {
        self.store
            .io(ErrorVerb::Read, |database| {
                let state = read_state(&database.connection)?;
                Ok((state.applied, state.membership))
            })
            .await
            .map_err(|error| *error)
    }
    async fn apply<I>(&mut self, entries: I) -> Stored<Vec<ApplyResult>>
    where
        I: IntoIterator<Item = Entry<TypeConfig>> + Send,
        I::IntoIter: Send,
    {
        let entries: Vec<_> = entries
            .into_iter()
            .take(config::ROOM_CONTROL_LOG_BATCH + 1)
            .collect();
        if entries.len() > config::ROOM_CONTROL_LOG_BATCH {
            return Err(*failure(
                ErrorVerb::Write,
                "public apply batch exceeds resource bound",
            ));
        }
        let root = self.anchor.clone();
        let result = self
            .store
            .io(ErrorVerb::Write, move |database| {
                write(database, |connection| {
                    let mut state = read_state(connection)?;
                    let mut view = checked_view(&root, &state)?;
                    let mut journal: Vec<StoredRecord> =
                        metadata(connection, "applied_journal")?.unwrap_or_default();
                    records::validate(&journal, root.id(), state.position)?;
                    let mut responses = Vec::with_capacity(entries.len());
                    for entry in entries {
                        let mut change = None;
                        let mut response = ApplyResult {
                            accepted: true,
                            epoch: view.epoch(),
                            head: view.head(),
                            error: None,
                            replayed: false,
                            first_log: Some(entry.log_id),
                        };
                        match entry.payload {
                            EntryPayload::Blank => {}
                            EntryPayload::Membership(membership) => {
                                for (id, node) in membership.nodes() {
                                    if !node.certificate.is_empty() {
                                        node.peer(*id).map_err(|_| {
                                            failure(
                                                ErrorVerb::Write,
                                                "invalid committed control certificate",
                                            )
                                        })?;
                                    }
                                }
                                state.membership =
                                    StoredMembership::new(Some(entry.log_id), membership);
                            }
                            EntryPayload::Normal(proposal) => {
                                let author = proposal.author();
                                let commit = proposal.commit_bytes().to_vec();
                                let operation = proposal.id().ok();
                                let prior = view.peers();
                                response = apply_proposal(
                                    &mut view,
                                    &mut state.receipts,
                                    proposal,
                                    entry.log_id,
                                );
                                if response.accepted && !response.replayed {
                                    change = Some(Change {
                                        author,
                                        commit,
                                        accepted: response.accepted,
                                        proposal: operation,
                                        recipient: view
                                            .peers()
                                            .iter()
                                            .find(|peer| {
                                                !prior.iter().any(|known| {
                                                    known.fingerprint_bytes()
                                                        == peer.fingerprint_bytes()
                                                })
                                            })
                                            .map(|peer| peer.fingerprint_bytes()),
                                    });
                                }
                            }
                        }
                        state.position = Some(records::append(
                            &mut journal,
                            StoredRecord {
                                room: root.id(),
                                log: entry.log_id,
                                previous: state.position,
                                epoch: view.epoch(),
                                audience: view.membership_digest().map_err(|_| {
                                    failure(ErrorVerb::Write, "invalid applied audience")
                                })?,
                                change,
                            },
                        )?);
                        state.applied = Some(entry.log_id);
                        responses.push(response);
                    }
                    state.view = view.snapshot();
                    put(connection, "applied_journal", &journal)?;
                    put(connection, "machine", &state)?;
                    Ok(responses)
                })
            })
            .await
            .map_err(|error| *error);
        if result.is_ok() {
            self.changed();
        }
        result
    }
    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        self.clone()
    }
    async fn begin_receiving_snapshot(&mut self) -> Stored<Box<SnapshotBuffer>> {
        Ok(Box::default())
    }
    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<PeerId, super::Node>,
        snapshot: Box<SnapshotBuffer>,
    ) -> Stored<()> {
        let data = (*snapshot).into_bytes();
        let state = bounded_state(&data).map_err(|error| *error)?;
        checked_view(&self.anchor, &state).map_err(|error| *error)?;
        if meta.last_log_id != state.applied
            || meta.last_membership != state.membership
            || meta.snapshot_id != snapshot_id(&data)
        {
            return Err(*failure(
                ErrorVerb::Write,
                "snapshot metadata and public state disagree",
            ));
        }
        let meta = meta.clone();
        let result = self
            .store
            .io(ErrorVerb::Write, move |database| {
                write(database, |connection| {
                    put(connection, "machine", &state)?;
                    put(connection, "snapshot_meta", &meta)?;
                    // A snapshot supplies PUBLIC state, not the omitted MLS commits.
                    // Existing private state must catch up from retained records or rejoin.
                    put(connection, "applied_journal", &Vec::<StoredRecord>::new())?;
                    put_blob(connection, "snapshot_data", &data)
                })
            })
            .await
            .map_err(|error| *error);
        if result.is_ok() {
            self.changed();
        }
        result
    }
    async fn get_current_snapshot(&mut self) -> Stored<Option<Snapshot<TypeConfig>>> {
        self.store
            .io(ErrorVerb::Read, |database| {
                let meta: Option<SnapshotMeta<PeerId, super::Node>> =
                    metadata(&database.connection, "snapshot_meta")?;
                match meta {
                    None => Ok(None),
                    Some(meta) => {
                        let data =
                            blob(&database.connection, "snapshot_data")?.ok_or_else(|| {
                                failure(ErrorVerb::Read, "public snapshot data missing")
                            })?;
                        if snapshot_id(&data) != meta.snapshot_id {
                            return Err(failure(ErrorVerb::Read, "public snapshot hash mismatch"));
                        }
                        let buffer = SnapshotBuffer::new(data).map_err(|_| {
                            failure(ErrorVerb::Read, "stored public snapshot exceeds limit")
                        })?;
                        Ok(Some(Snapshot {
                            meta,
                            snapshot: Box::new(buffer),
                        }))
                    }
                }
            })
            .await
            .map_err(|error| *error)
    }
}
impl RaftSnapshotBuilder<TypeConfig> for StateMachine {
    async fn build_snapshot(&mut self) -> Stored<Snapshot<TypeConfig>> {
        self.store
            .io(ErrorVerb::Write, |database| {
                write(database, |connection| {
                    let state = read_state(connection)?;
                    let data = encode(&state)?;
                    if data.len() > config::ROOM_CONTROL_SNAPSHOT_BYTES {
                        return Err(failure(
                            ErrorVerb::Write,
                            "public snapshot exceeds aggregate limit",
                        ));
                    }
                    let meta = SnapshotMeta {
                        last_log_id: state.applied,
                        last_membership: state.membership,
                        snapshot_id: snapshot_id(&data),
                    };
                    put(connection, "snapshot_meta", &meta)?;
                    put_blob(connection, "snapshot_data", &data)?;
                    Ok(Snapshot {
                        meta,
                        snapshot: Box::new(
                            SnapshotBuffer::new(data)
                                .map_err(|_| failure(ErrorVerb::Write, "snapshot exceeds limit"))?,
                        ),
                    })
                })
            })
            .await
            .map_err(|error| *error)
    }
}

#[cfg(all(test, unix))]
mod tests;
