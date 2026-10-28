//! Foreground-owned history configuration and quorum-current disclosure gates.
use super::*;
use crate::rooms::{group::StableReference, store::history as store};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use zeroize::Zeroize;

pub use crate::rooms::group::HistoryRow;
pub use store::Policy;

/// Cached public metadata, not a fresh quorum or verified row-count claim.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Summary {
    Unknown,
    Off,
    Enrolled {
        recipient: String,
        enabled: bool,
        unlocked: bool,
        policy: Policy,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Search {
    pub text: String,
    pub after_id: u64,
    pub limit: usize,
    pub response_bytes: usize,
}
impl Drop for Search {
    fn drop(&mut self) {
        self.text.zeroize();
    }
}
impl Search {
    pub fn validate(&self) -> Result<()> {
        if self.text.len() > store::MAX_QUERY_BYTES
            || self.limit == 0
            || self.limit > store::MAX_RESULTS
            || self.response_bytes == 0
            || self.response_bytes > store::MAX_RESPONSE_BYTES
        {
            return Err(Error::Configuration("invalid bounded history query"));
        }
        Ok(())
    }
}
#[derive(Serialize)]
pub struct Page {
    pub rows: Vec<HistoryRow>,
    pub next_after: u64,
    pub has_more: bool,
    /// Rows scanned on this page but not currently authorized for disclosure.
    pub withheld: usize,
}

/// No paths or private key material in remotely/adaptor supplied commands.
pub(crate) enum Operation {
    Enable(Policy),
    Open,
    Recover(age::x25519::Recipient, Policy),
    Identity(age::x25519::Identity),
    EnableIdentity(Policy),
    Disable,
    Clear,
    Status,
}
fn now() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .map_err(|_| Error::Storage("history clock unavailable"))
}
impl Room {
    pub async fn history_enable_file(
        &mut self,
        policy: Policy,
        deadline: Duration,
    ) -> Result<Summary> {
        policy.validate()?;
        self.history_operation(Operation::Enable(policy), deadline)
            .await
    }
    pub async fn history_open_file(&mut self, deadline: Duration) -> Result<Summary> {
        self.history_operation(Operation::Open, deadline).await
    }
    pub async fn history_recover_file(
        &mut self,
        expected: age::x25519::Recipient,
        policy: Policy,
        deadline: Duration,
    ) -> Result<Summary> {
        policy.validate()?;
        self.history_operation(Operation::Recover(expected, policy), deadline)
            .await
    }
    pub async fn history_use_identity(
        &mut self,
        identity: age::x25519::Identity,
        deadline: Duration,
    ) -> Result<Summary> {
        self.history_operation(Operation::Identity(identity), deadline)
            .await
    }
    pub async fn history_enable(&mut self, policy: Policy, deadline: Duration) -> Result<Summary> {
        policy.validate()?;
        self.history_operation(Operation::EnableIdentity(policy), deadline)
            .await
    }
    pub async fn history_disable(&mut self, deadline: Duration) -> Result<Summary> {
        self.history_operation(Operation::Disable, deadline).await
    }
    pub async fn history_clear(&mut self, deadline: Duration) -> Result<Summary> {
        self.history_operation(Operation::Clear, deadline).await
    }
    pub async fn history_status(&mut self, deadline: Duration) -> Result<Summary> {
        self.history_operation(Operation::Status, deadline).await
    }
    /// Trusted foreground owner only, before moving Room into Actor::new.
    /// Group additionally verifies this private directory against its database.
    pub fn configure_history_directory(&mut self, directory: PathBuf) -> Result<()> {
        if self.history_directory.is_some() || !directory.is_absolute() {
            return Err(Error::Configuration(
                "history directory already set or not absolute",
            ));
        }
        self.history_directory = Some(directory);
        Ok(())
    }
    fn history_reader(&self, view: &View) -> Result<()> {
        if !view
            .member_rights(&self.identity.peer().fingerprint_bytes())
            .is_some_and(|rights| rights.valid())
        {
            return Err(Error::Denied);
        }
        self.authorize_audience()
    }
    fn history_row(&self, view: &View, row: &HistoryRow) -> Result<()> {
        let reference = &row.reference;
        if reference.room != self.room_id()
            || reference.epoch > view.epoch()
            || reference.epoch != row.record.epoch
            || reference.event != row.record.event
            || reference.wire_sha256 != row.record.wire_sha256
            || row.record.binding.room != reference.room
        {
            return Err(Error::Authentication);
        }
        if reference.event == store::Event::OutgoingPrepared {
            if reference.member != self.identity.peer().fingerprint_bytes() {
                return Err(Error::Authentication);
            }
            return self.authorize_audience();
        }
        let peer = self
            .peers()
            .into_iter()
            .find(|peer| peer.fingerprint_bytes() == reference.member)
            .ok_or(Error::Denied)?;
        if !view
            .member_rights(&reference.member)
            .is_some_and(|rights| rights.can_publish())
        {
            return Err(Error::Denied);
        }
        self.sender(&peer)
    }
    pub(crate) async fn history_operation(
        &mut self,
        operation: Operation,
        deadline: Duration,
    ) -> Result<Summary> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        let expires = tokio::time::Instant::now() + deadline;
        tokio::time::timeout(deadline, async {
            let view = self.synchronize(deadline).await?;
            self.history_reader(&view)?;
            let revision = self.profile.revision()?;
            let now = now()?;
            match operation {
                Operation::Enable(policy) => self.group.history_enable_file(
                    self.history_directory
                        .as_deref()
                        .ok_or(Error::Configuration("history directory not configured"))?,
                    policy,
                    now,
                )?,
                Operation::Open => self.group.history_open_file(
                    self.history_directory
                        .as_deref()
                        .ok_or(Error::Configuration("history directory not configured"))?,
                    now,
                )?,
                Operation::Recover(expected, policy) => self.group.history_recover_file(
                    self.history_directory
                        .as_deref()
                        .ok_or(Error::Configuration("history directory not configured"))?,
                    &expected,
                    policy,
                    now,
                )?,
                Operation::Identity(identity) => self.group.history_use_identity(identity, now)?,
                Operation::EnableIdentity(policy) => self.group.history_enable(policy, now)?,
                Operation::Disable => self.group.history_disable(now)?,
                Operation::Clear => self.group.history_clear(now)?,
                Operation::Status => (),
            }
            let summary = match self.group.history_status(now)? {
                store::Status::Off => Summary::Off,
                store::Status::Locked {
                    binding,
                    enabled,
                    policy,
                } => Summary::Enrolled {
                    recipient: binding.recipient,
                    enabled,
                    unlocked: false,
                    policy,
                },
                store::Status::Configured {
                    binding,
                    enabled,
                    policy,
                    ..
                } => Summary::Enrolled {
                    recipient: binding.recipient,
                    enabled,
                    unlocked: true,
                    policy,
                },
            };
            let after = self.synchronize(deadline).await?;
            self.history_reader(&after)?;
            if revision != self.profile.revision()? || after.epoch() != view.epoch() {
                return Err(Error::Denied);
            }
            if tokio::time::Instant::now() >= expires {
                return Err(Error::Timeout);
            }
            self.history_summary = summary.clone();
            Ok(summary)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    pub async fn history_search(&mut self, search: Search, deadline: Duration) -> Result<Page> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        search.validate()?;
        let expires = tokio::time::Instant::now() + deadline;
        tokio::time::timeout(deadline, async {
            let before = self.synchronize(deadline).await?;
            self.history_reader(&before)?;
            let revision = self.profile.revision()?;
            let result = self.group.history_search(
                store::Query {
                    text: &search.text,
                    after_id: search.after_id,
                    limit: search.limit,
                    response_bytes: search.response_bytes,
                },
                now()?,
            )?;
            #[cfg(test)]
            after_private_operation();
            let after = self.synchronize(deadline).await?;
            self.history_reader(&after)?;
            if revision != self.profile.revision()? || before.epoch() != after.epoch() {
                return Err(Error::Denied);
            }
            let mut page = Page {
                rows: vec![],
                next_after: search.after_id,
                has_more: true,
                withheld: 0,
            };
            let count = result.rows.len();
            for (index, row) in result.rows.into_iter().enumerate() {
                let previous_cursor = page.next_after;
                let previous_withheld = page.withheld;
                page.next_after = row.record.id;
                page.has_more = index + 1 < count || result.has_more;
                match self.history_row(&after, &row) {
                    Ok(()) => page.rows.push(row),
                    Err(Error::Denied | Error::Revoked) => page.withheld += 1,
                    Err(error) => return Err(error),
                }
                // Budget the complete runtime frame, not only the Group frame.
                // A deferred authorized row must remain behind the cursor.
                if page_size(&page)? > search.response_bytes {
                    if page.withheld == previous_withheld {
                        page.rows.pop();
                    }
                    page.withheld = previous_withheld;
                    page.next_after = previous_cursor;
                    page.has_more = true;
                    if previous_cursor == search.after_id
                        || page_size(&page)? > search.response_bytes
                    {
                        return Err(Error::Limit {
                            resource: "history response",
                            max: search.response_bytes,
                        });
                    }
                    break;
                }
            }
            if count == 0 {
                page.next_after = result.next_after;
                page.has_more = result.has_more;
            }
            // No async boundary between final per-author/local-veto check and return.
            for row in &page.rows {
                self.history_row(&after, row)?;
            }
            self.history_reader(&after)?;
            if revision != self.profile.revision()? {
                return Err(Error::Denied);
            }
            if tokio::time::Instant::now() >= expires {
                return Err(Error::Timeout);
            }
            // Include the runtime's withheld counter in the caller's wire budget.
            if page_size(&page)? > search.response_bytes {
                return Err(Error::Limit {
                    resource: "history response",
                    max: search.response_bytes,
                });
            }
            Ok(page)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    pub async fn encrypt_text_referenced(
        &mut self,
        text: &str,
        sensitive: bool,
        deadline: Duration,
    ) -> Result<(Vec<u8>, StableReference)> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        if text.len() > config::MAX_MESSAGE_BYTES {
            return Err(Error::MessageTooLarge);
        }
        let expires = tokio::time::Instant::now() + deadline;
        tokio::time::timeout(deadline, async {
            let before = self.synchronize(deadline).await?;
            self.history_reader(&before)?;
            if !before
                .member_rights(&self.identity.peer().fingerprint_bytes())
                .is_some_and(|rights| rights.can_publish())
            {
                return Err(Error::Denied);
            }
            let revision = self.profile.revision()?;
            let result = self
                .group
                .encrypt_text_referenced(text, sensitive, now()?)?;
            #[cfg(test)]
            after_private_operation();
            let after = self.synchronize(deadline).await?;
            self.history_reader(&after)?;
            if before.epoch() != after.epoch() || revision != self.profile.revision()? {
                return Err(Error::Denied);
            }
            if tokio::time::Instant::now() >= expires {
                return Err(Error::Timeout);
            }
            Ok(result)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
    pub async fn decrypt_text_referenced(
        &mut self,
        sender: &Peer,
        wire: &[u8],
        deadline: Duration,
    ) -> Result<(Text, StableReference)> {
        crate::transport::validate_duration(deadline, config::MAX_FILE_TIMEOUT)?;
        if wire.len() > config::ROOM_MAX_WIRE_BYTES {
            return Err(Error::MessageTooLarge);
        }
        let expires = tokio::time::Instant::now() + deadline;
        tokio::time::timeout(deadline, async {
            let before = self.synchronize(deadline).await?;
            self.history_reader(&before)?;
            self.sender(sender)?;
            if !before
                .member_rights(&sender.fingerprint_bytes())
                .is_some_and(|rights| rights.can_publish())
            {
                return Err(Error::Denied);
            }
            let revision = self.profile.revision()?;
            let result = self
                .group
                .decrypt_text_referenced(wire, Some(sender), now()?)?;
            #[cfg(test)]
            after_private_operation();
            let after = self.synchronize(deadline).await?;
            self.history_reader(&after)?;
            self.sender(sender)?;
            if before.epoch() != after.epoch() || revision != self.profile.revision()? {
                return Err(Error::Denied);
            }
            if tokio::time::Instant::now() >= expires {
                return Err(Error::Timeout);
            }
            Ok(result)
        })
        .await
        .map_err(|_| Error::Timeout)?
    }
}

#[cfg(test)]
thread_local! {
    pub(super) static AFTER_PRIVATE_OPERATION: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
fn after_private_operation() {
    let hook = AFTER_PRIVATE_OPERATION.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}

fn page_size(page: &Page) -> Result<usize> {
    struct Counter(usize);
    impl std::io::Write for Counter {
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
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, page)
        .map_err(|_| Error::Storage("cannot size history page"))?;
    Ok(counter.0)
}
