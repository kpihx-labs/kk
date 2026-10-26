//! Bounded LOCAL applied journal. A public snapshot is not private MLS catch-up.
use super::{
    PeerId,
    log::{Internal, encode, failure},
};
use crate::{Error, Result, config};
use openraft::{ErrorVerb, LogId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub log: LogId<PeerId>,
    pub hash: [u8; 32],
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Change {
    pub author: [u8; 32],
    pub commit: Vec<u8>,
    pub accepted: bool,
    #[serde(default)]
    pub proposal: Option<[u8; 32]>,
    #[serde(default)]
    pub recipient: Option<[u8; 32]>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredRecord {
    pub room: [u8; 32],
    pub log: LogId<PeerId>,
    pub previous: Option<Position>,
    pub epoch: u64,
    pub audience: [u8; 32],
    pub change: Option<Change>,
}

/// Only returned from protected, durably applied LOCAL control state. No public
/// constructor or Deserialize implementation: a remote DTO is not this authority.
#[derive(Clone)]
pub struct CommittedRecord(pub(super) StoredRecord);
impl CommittedRecord {
    pub fn proposal_id(&self) -> Option<[u8; 32]> {
        self.0.change.as_ref().and_then(|change| change.proposal)
    }
    pub fn admitted_peer(&self) -> Option<PeerId> {
        self.0
            .change
            .as_ref()
            .and_then(|change| change.recipient)
            .map(PeerId)
    }
    pub fn position(&self) -> Result<Position> {
        self.0
            .position()
            .map_err(|_| Error::Storage("invalid applied record"))
    }
    pub fn previous(&self) -> Option<Position> {
        self.0.previous
    }
    pub fn room(&self) -> [u8; 32] {
        self.0.room
    }
    pub fn epoch(&self) -> u64 {
        self.0.epoch
    }
    pub fn membership_digest(&self) -> [u8; 32] {
        self.0.audience
    }
    pub(crate) fn change(&self) -> Option<([u8; 32], &[u8], bool)> {
        self.0
            .change
            .as_ref()
            .map(|change| (change.author, change.commit.as_slice(), change.accepted))
    }
}
impl StoredRecord {
    pub(super) fn position(&self) -> Internal<Position> {
        let mut digest = Sha256::new();
        digest.update(b"K&K applied public control record v1\0");
        digest.update(encode(self)?);
        Ok(Position {
            log: self.log,
            hash: digest.finalize().into(),
        })
    }
}

pub(super) fn validate(
    records: &[StoredRecord],
    room: [u8; 32],
    last: Option<Position>,
) -> Internal<()> {
    if records.len() > config::ROOM_APPLIED_RECORDS
        || encode(&records)?.len() > config::ROOM_APPLIED_RECORD_BYTES
    {
        return Err(failure(ErrorVerb::Read, "applied journal exceeds bound"));
    }
    let mut previous = None;
    for (index, record) in records.iter().enumerate() {
        if record.room != room
            || record
                .change
                .as_ref()
                .is_some_and(|change| change.commit.len() > config::ROOM_MAX_WIRE_BYTES)
            || (index > 0 && record.previous != previous)
            || record
                .previous
                .is_some_and(|position| position.log.index.checked_add(1) != Some(record.log.index))
        {
            return Err(failure(ErrorVerb::Read, "invalid applied journal chain"));
        }
        previous = Some(record.position()?);
    }
    if !records.is_empty() && previous != last {
        return Err(failure(
            ErrorVerb::Read,
            "applied journal and machine disagree",
        ));
    }
    Ok(())
}

pub(super) fn append(records: &mut Vec<StoredRecord>, record: StoredRecord) -> Internal<Position> {
    let position = record.position()?;
    let added = encode(&record)?.len() + 1;
    if added + 2 > config::ROOM_APPLIED_RECORD_BYTES {
        return Err(failure(
            ErrorVerb::Write,
            "single applied record exceeds retention bound",
        ));
    }
    // Bound by encoded record sizes BEFORE allocating the aggregate value.
    // Encoding an oversized aggregate first could hit the database value cap
    // before old history has the chance to be evicted.
    let mut bytes = records
        .iter()
        .try_fold(2 + added, |total, stored| -> Internal<usize> {
            Ok(total + encode(stored)?.len() + 1)
        })?;
    records.push(record);
    while records.len() > config::ROOM_APPLIED_RECORDS || bytes > config::ROOM_APPLIED_RECORD_BYTES
    {
        bytes -= encode(&records.remove(0))?.len() + 1;
    }
    Ok(position)
}

pub(super) fn after(
    records: Vec<StoredRecord>,
    cursor: Option<Position>,
    last: Option<Position>,
) -> Internal<Vec<CommittedRecord>> {
    if cursor == last {
        return Ok(Vec::new());
    }
    let start = records
        .iter()
        .position(|record| record.previous == cursor)
        .ok_or_else(|| {
            failure(
                ErrorVerb::Read,
                "private catch-up gap; explicit verified rejoin required",
            )
        })?;
    Ok(records
        .into_iter()
        .skip(start)
        .map(CommittedRecord)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openraft::CommittedLeaderId;

    fn record(index: u64, previous: Option<Position>) -> StoredRecord {
        StoredRecord {
            room: [1; 32],
            log: LogId::new(CommittedLeaderId::new(1, PeerId([2; 32])), index),
            previous,
            epoch: 0,
            audience: [3; 32],
            change: None,
        }
    }

    #[test]
    fn retained_history_is_bounded_and_an_evicted_bridge_requires_rejoin() {
        let mut journal = Vec::new();
        let mut last = None;
        for index in 0..(config::ROOM_APPLIED_RECORDS + 3) as u64 {
            last = Some(append(&mut journal, record(index, last)).unwrap());
        }
        assert_eq!(journal.len(), config::ROOM_APPLIED_RECORDS);
        validate(&journal, [1; 32], last).unwrap();
        assert!(after(journal.clone(), None, last).is_err());
        let start = journal[0].previous;
        assert_eq!(
            after(journal.clone(), start, last).unwrap().len(),
            config::ROOM_APPLIED_RECORDS
        );
        assert!(after(journal, last, last).unwrap().is_empty());
    }

    #[test]
    fn tampered_record_or_log_position_does_not_validate_as_the_same_chain() {
        let mut journal = Vec::new();
        let first = append(&mut journal, record(0, None)).unwrap();
        let last = Some(append(&mut journal, record(1, Some(first))).unwrap());
        journal[0].audience[0] ^= 1;
        assert!(validate(&journal, [1; 32], last).is_err());
        journal[0].audience[0] ^= 1;
        journal[1].log.index = 5;
        assert!(validate(&journal, [1; 32], last).is_err());
    }
}
