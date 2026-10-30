//! Group-owned opt-in capture. IncomingProcessed is MLS processing, NOT runtime
//! post-authorization. An actor must recheck current authorization before exposing
//! search results or incoming plaintext. This SDK does not grant UI/IPC access.
use super::*;
use crate::rooms::store::{
    Transaction, history as store,
    history_key::{HistoryKey, HistoryKeyAnchor},
};
use age::x25519::{Identity, Recipient};
use std::path::Path;

const REFERENCES: &str = "group_history_references_v1";

/// Exact-wire digest plus authenticated MLS context, never a display name or receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StableReference {
    pub room: [u8; 32],
    pub member: [u8; 32],
    pub epoch: u64,
    pub event: store::Event,
    pub wire_sha256: [u8; 32],
}
#[derive(Serialize)]
pub struct HistoryRow {
    pub record: store::Row,
    pub reference: StableReference,
}
#[derive(Serialize)]
pub struct HistoryPage {
    pub rows: Vec<HistoryRow>,
    pub next_after: u64,
    pub has_more: bool,
}

// No Debug, Serialize, private export or checkpoint field for either key source.
enum Source {
    Sdk(Identity),
    File {
        anchor: HistoryKeyAnchor,
        key: HistoryKey,
    },
}
pub(super) struct Context {
    binding: store::Binding,
    source: Source,
}
impl Context {
    fn with_identity<T>(&self, use_key: impl FnOnce(&Identity) -> T) -> T {
        match &self.source {
            Source::Sdk(identity) => use_key(identity),
            Source::File { key, .. } => key.with_identity(use_key),
        }
    }
    fn verify(&self, group: &Group) -> Result<()> {
        group.check_history_binding(&self.binding)?;
        if let Source::File { anchor, key } = &self.source {
            anchor.verify()?;
            #[cfg(unix)]
            group
                .actor
                .provider
                .verify_history_directory(&anchor.directory_file()?)?;
            // Losing/replacing the protected file is not masked by an in-memory key.
            let loaded = anchor.load()?;
            if loaded.recipient().to_string() != key.recipient().to_string() {
                return Err(Error::Storage(
                    "history key changed; explicit recovery required",
                ));
            }
        }
        Ok(())
    }
}

// An encrypted bounded companion record binds trusted MLS member IDs to the
// foundation's unchanged Row schema. Replaced, pruned and cleared atomically with
// rows, never an unbounded/plaintext metadata index or another SQL connection.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct References {
    version: u8,
    binding: store::Binding,
    entries: Vec<(u64, StableReference)>,
}

pub(super) fn now() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|_| Error::Storage("history clock unavailable"))
}

impl Group {
    fn history_profile(&self) -> Result<[u8; 32]> {
        self.members
            .iter()
            .find(|(_, member)| member.peer().fingerprint() == self.actor.owner)
            .map(|(_, member)| member.peer().fingerprint_bytes())
            .ok_or(Error::Authentication)
    }
    fn check_history_binding(&self, binding: &store::Binding) -> Result<()> {
        if binding.profile != self.history_profile()? || binding.room != self.room_id() {
            return Err(Error::Storage("history profile or room mismatch"));
        }
        Ok(())
    }
    pub(super) fn verify_history_anchor(&self) -> Result<()> {
        if let Some(context) = &self.history {
            context.verify(self)?;
        }
        Ok(())
    }
    fn history_state(
        &self,
        tx: &Transaction,
        context: Option<&Context>,
        now: u64,
    ) -> Result<store::Status> {
        let status = if let Some(context) = context {
            context.verify(self)?;
            context.with_identity(|identity| {
                self.actor
                    .provider
                    .history_status(tx, Some(&context.binding), Some(identity), now)
            })?
        } else {
            self.actor.provider.history_status(tx, None, None, now)?
        };
        match &status {
            store::Status::Locked { binding, .. } | store::Status::Configured { binding, .. } => {
                self.check_history_binding(binding)?
            }
            store::Status::Off => (),
        }
        Ok(status)
    }
    pub(super) fn history_preflight(&self, tx: &Transaction, now: u64) -> Result<bool> {
        match self.history_state(tx, self.history.as_deref(), now)? {
            store::Status::Off => Ok(false),
            store::Status::Configured { enabled, .. } => Ok(enabled),
            store::Status::Locked { .. } => {
                Err(Error::Storage("history is locked; supply the enrolled key"))
            }
        }
    }
    /// Configure an SDK key without persisting it or enrolling an off store.
    /// Existing policy must agree. Reopen requires the caller to supply it again.
    pub fn history_use_identity(&mut self, identity: Identity, now: u64) -> Result<()> {
        let context = Rc::new(Context {
            binding: store::Binding::new(
                self.history_profile()?,
                self.room_id(),
                &identity.to_public(),
            ),
            source: Source::Sdk(identity),
        });
        self.transactional_with_tx(|group, tx| {
            group.history_state(tx, Some(&context), now)?;
            Ok(())
        })?;
        self.history = Some(context);
        Ok(())
    }
    /// Explicit policy enable/update for a previously supplied or loaded key.
    pub fn history_enable(&mut self, policy: store::Policy, now: u64) -> Result<()> {
        let context = self
            .history
            .clone()
            .ok_or(Error::Storage("history key context required"))?;
        self.transactional_with_tx(|group, tx| {
            group.enable_history_context(tx, &context, policy, now)
        })
    }
    fn enable_history_context(
        &self,
        tx: &Transaction,
        context: &Context,
        policy: store::Policy,
        now: u64,
    ) -> Result<()> {
        context.verify(self)?;
        context.with_identity(|identity| {
            self.actor
                .provider
                .history_enable(tx, &context.binding, identity, policy, now)
        })?;
        let rows = self.history_all_rows(tx, context, now)?;
        let mut references = self.history_references(context)?;
        Self::retain_references(&mut references, &rows)?;
        self.save_history_references(context, &references)?;
        context.verify(self)
    }
    /// Load an already enrolled protected room file. Never initializes or adopts
    /// an orphan. Path must be the same directory as this Provider's open database.
    pub fn history_open_file(&mut self, directory: &Path, now: u64) -> Result<()> {
        self.history_file(directory, None, None, now)
    }
    /// Explicit initialization only when both database policy and key are absent.
    /// An existing enrolled policy may be re-enabled; an orphan is refused.
    pub fn history_enable_file(
        &mut self,
        directory: &Path,
        policy: store::Policy,
        now: u64,
    ) -> Result<()> {
        self.history_file(directory, Some(policy), None, now)
    }
    /// Explicit recovery of a key published before enrollment committed. Requires
    /// the caller's expected PUBLIC recipient, an off V1 store and no companion
    /// records. Never substitutes a recipient into an existing policy.
    pub fn history_recover_file(
        &mut self,
        directory: &Path,
        expected: &Recipient,
        policy: store::Policy,
        now: u64,
    ) -> Result<()> {
        self.history_file(directory, Some(policy), Some(expected), now)
    }
    fn history_file(
        &mut self,
        directory: &Path,
        policy: Option<store::Policy>,
        recover: Option<&Recipient>,
        now: u64,
    ) -> Result<()> {
        if let Some(policy) = policy {
            policy.validate()?;
        }
        let anchor = HistoryKeyAnchor::open(directory, self.history_profile()?, self.room_id())?;
        #[cfg(unix)]
        self.actor
            .provider
            .verify_history_directory(&anchor.directory_file()?)?;
        let context = self.transactional_with_tx(|group, tx| {
            let status = group.history_state(tx, None, now)?;
            let off = matches!(status, store::Status::Off);
            let key = if off {
                if policy.is_none() {
                    return Err(Error::Storage("history is not enrolled"));
                }
                if group.actor.provider.read_blob(REFERENCES)?.is_some() {
                    return Err(Error::Storage("ambiguous history recovery state"));
                }
                if let Some(expected) = recover {
                    let key = anchor.load()?;
                    if key.recipient().to_string() != expected.to_string() {
                        return Err(Error::Authentication);
                    }
                    key
                } else {
                    anchor.initialize()?
                }
            } else {
                if recover.is_some() {
                    return Err(Error::Storage("orphan recovery requires an off store"));
                }
                anchor.load()?
            };
            let context = Rc::new(Context {
                binding: store::Binding::new(
                    group.history_profile()?,
                    group.room_id(),
                    &key.recipient(),
                ),
                source: Source::File { anchor, key },
            });
            group.history_state(tx, Some(&context), now)?;
            if let Some(policy) = policy {
                group.enable_history_context(tx, &context, policy, now)?;
            }
            context.verify(group)?;
            Ok(context)
        })?;
        self.history = Some(context);
        Ok(())
    }
    pub fn history_status(&mut self, now: u64) -> Result<store::Status> {
        self.transactional_with_tx(|group, tx| {
            group.history_state(tx, group.history.as_deref(), now)
        })
    }
    pub fn history_disable(&mut self, now: u64) -> Result<()> {
        let context = self
            .history
            .clone()
            .ok_or(Error::Storage("history key context required"))?;
        self.transactional_with_tx(|group, tx| {
            context.verify(group)?;
            context.with_identity(|identity| {
                group
                    .actor
                    .provider
                    .history_disable(tx, &context.binding, identity, now)
            })
        })
    }
    pub fn history_clear(&mut self, now: u64) -> Result<()> {
        let context = self
            .history
            .clone()
            .ok_or(Error::Storage("history key context required"))?;
        self.transactional_with_tx(|group, tx| {
            context.verify(group)?;
            context.with_identity(|identity| {
                group
                    .actor
                    .provider
                    .history_clear(tx, &context.binding, identity, now)
            })?;
            group.save_history_references(
                &context,
                &References {
                    version: 1,
                    binding: context.binding.clone(),
                    entries: vec![],
                },
            )
        })
    }
    /// SDK-only processed/prepared records. The future actor adapter MUST recheck
    /// current runtime authorization, including each incoming author, before any
    /// result is exposed. MLS processing is not post-authorization or delivery.
    pub fn history_search(&mut self, query: store::Query<'_>, now: u64) -> Result<HistoryPage> {
        let context = self
            .history
            .clone()
            .ok_or(Error::Storage("history key context required"))?;
        self.transactional_with_tx(|group, tx| {
            context.verify(group)?;
            let references = group.history_references(&context)?;
            // Validate the companion context for the entire bounded retained
            // set, not only the matches returned on this page.
            for row in group.history_all_rows(tx, &context, now)? {
                Self::reference_for(&references, &row)?;
            }
            let budget = query.response_bytes;
            let page = context.with_identity(|identity| {
                group.actor.provider.history_search(
                    tx,
                    &context.binding,
                    Some(identity),
                    query,
                    now,
                )
            })?;
            let mut result = HistoryPage {
                rows: Vec::with_capacity(page.rows.len()),
                next_after: page.next_after,
                has_more: page.has_more,
            };
            for record in page.rows {
                let reference = Self::reference_for(&references, &record)?.clone();
                result.rows.push(HistoryRow { record, reference });
            }
            // Foundation's budget covers its Page, not the added authenticated
            // references. Count actual escaped JSON without a plaintext copy.
            while response_size(&result)? > budget {
                if result.rows.len() <= 1 {
                    return Err(Error::Limit {
                        resource: "history response",
                        max: budget,
                    });
                }
                result.rows.pop();
                result.next_after = result.rows.last().ok_or(Error::Protocol)?.record.id;
                result.has_more = true;
            }
            Ok(result)
        })
    }
    fn history_references(&self, context: &Context) -> Result<References> {
        let Some(ciphertext) = self.actor.provider.read_blob(REFERENCES)? else {
            return Ok(References {
                version: 1,
                binding: context.binding.clone(),
                entries: vec![],
            });
        };
        let plaintext = context
            .with_identity(|identity| crate::envelope::record::open(&ciphertext, identity))?;
        let references: References = serde_json::from_slice(&plaintext)
            .map_err(|_| Error::Storage("invalid history references"))?;
        if references.version != 1
            || references.binding != context.binding
            || references.entries.len() > store::MAX_ROWS
            || references
                .entries
                .windows(2)
                .any(|pair| pair[0].0 >= pair[1].0)
        {
            return Err(Error::Storage("invalid history reference binding"));
        }
        Ok(references)
    }
    fn save_history_references(&self, context: &Context, references: &References) -> Result<()> {
        let plaintext = Zeroizing::new(
            serde_json::to_vec(references)
                .map_err(|_| Error::Storage("cannot encode history references"))?,
        );
        let recipient = context
            .binding
            .recipient
            .parse()
            .map_err(|_| Error::Authentication)?;
        let ciphertext = crate::envelope::record::seal(plaintext, &recipient)?;
        self.actor.provider.write_blob(REFERENCES, &ciphertext)
    }
    fn reference_for<'a>(
        references: &'a References,
        row: &store::Row,
    ) -> Result<&'a StableReference> {
        let reference = references
            .entries
            .iter()
            .find(|(id, _)| *id == row.id)
            .map(|(_, value)| value)
            .ok_or(Error::Storage("history author context is missing"))?;
        if reference.room != row.binding.room
            || reference.epoch != row.epoch
            || reference.event != row.event
            || reference.wire_sha256 != row.wire_sha256
        {
            return Err(Error::Storage("history author context mismatch"));
        }
        Ok(reference)
    }
    fn retain_references(references: &mut References, rows: &[store::Row]) -> Result<()> {
        for row in rows {
            Self::reference_for(references, row)?;
        }
        references
            .entries
            .retain(|(id, _)| rows.iter().any(|row| row.id == *id));
        Ok(())
    }
    fn history_all_rows(
        &self,
        tx: &Transaction,
        context: &Context,
        now: u64,
    ) -> Result<Vec<store::Row>> {
        let mut rows = Vec::new();
        let mut after = 0;
        loop {
            let page = context.with_identity(|identity| {
                self.actor.provider.history_search(
                    tx,
                    &context.binding,
                    Some(identity),
                    store::Query {
                        text: "",
                        after_id: after,
                        limit: store::MAX_RESULTS,
                        response_bytes: store::MAX_RESPONSE_BYTES,
                    },
                    now,
                )
            })?;
            after = page.next_after;
            rows.extend(page.rows);
            if rows.len() > store::MAX_ROWS {
                return Err(Error::Storage("history reference bound exceeded"));
            }
            if !page.has_more {
                return Ok(rows);
            }
        }
    }
    pub(super) fn capture_history(
        &self,
        tx: &Transaction,
        reference: &StableReference,
        body: &str,
        sensitive: bool,
        relation: Option<super::super::conversation::Relation>,
        now: u64,
    ) -> Result<()> {
        let context = self
            .history
            .as_deref()
            .ok_or(Error::Storage("history key context required"))?;
        let recorded = context.with_identity(|identity| {
            self.actor.provider.history_record_related(
                tx,
                &context.binding,
                Some(identity),
                store::Capture {
                    event: reference.event,
                    epoch: reference.epoch,
                    wire_sha256: reference.wire_sha256,
                    body,
                    sensitive,
                },
                relation,
                now,
            )
        })?;
        if let store::Recorded::Stored { id, .. } = recorded {
            let mut references = self.history_references(context)?;
            references.entries.push((id, reference.clone()));
            Self::retain_references(&mut references, &self.history_all_rows(tx, context, now)?)?;
            self.save_history_references(context, &references)?;
        }
        Ok(())
    }
    /// Prepared is not delivered/read. Wire bytes and reference escape only after
    /// MLS ratchet, encrypted history and checkpoint COMMIT succeeds.
    pub fn encrypt_text_referenced(
        &mut self,
        text: &str,
        sensitive: bool,
        now: u64,
    ) -> Result<(Vec<u8>, StableReference)> {
        self.transactional_with_tx(|group, tx| {
            let enabled = group.history_preflight(tx, now)?;
            let member = group
                .member(group.state.own_leaf_index())?
                .peer()
                .fingerprint_bytes();
            let wire = group.encrypt_text_mut(text, sensitive)?;
            let reference = StableReference {
                room: group.room_id(),
                member,
                epoch: group.state.epoch().as_u64(),
                event: store::Event::OutgoingPrepared,
                wire_sha256: Sha256::digest(&wire).into(),
            };
            if enabled {
                group.capture_history(tx, &reference, text, sensitive, None, now)?;
            }
            Ok((wire, reference))
        })
    }
    /// Processed is not runtime post-authorized. Caller must retain the existing
    /// runtime recheck before releasing plaintext or receipts to observers.
    pub fn decrypt_text_referenced(
        &mut self,
        wire: &[u8],
        sender: Option<&Peer>,
        now: u64,
    ) -> Result<(Text, StableReference)> {
        self.transactional_with_tx(|group, tx| {
            let enabled = group.history_preflight(tx, now)?;
            let text = group.decrypt_text_from_mut(wire, sender.map(Peer::fingerprint_bytes))?;
            let member = group
                .members
                .iter()
                .find(|(_, member)| member.peer().fingerprint() == text.sender)
                .map(|(_, member)| member.peer().fingerprint_bytes())
                .ok_or(Error::Authentication)?;
            let reference = StableReference {
                room: group.room_id(),
                member,
                epoch: group.state.epoch().as_u64(),
                event: store::Event::IncomingProcessed,
                wire_sha256: Sha256::digest(wire).into(),
            };
            if enabled {
                group.capture_history(
                    tx,
                    &reference,
                    text.content.text(),
                    text.content.sensitive,
                    None,
                    now,
                )?;
            }
            Ok((text, reference))
        })
    }
}

fn response_size(value: &HistoryPage) -> Result<usize> {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or(std::io::ErrorKind::InvalidData)?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    serde_json::to_writer(&mut count, value)
        .map_err(|_| Error::Storage("cannot size history response"))?;
    Ok(count.0)
}

#[cfg(all(test, unix))]
#[path = "history_tests.rs"]
mod tests;
