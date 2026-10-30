//! Versioned application framing inside MLS, not authentication or capability negotiation.
//! Legacy text stays under its existing AAD; never guess this format from text bytes.

use crate::{Error, ReceivedText, Result, config};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use zeroize::Zeroizing;

pub const AAD: &[u8] = b"K&K room conversation v1\0";
pub const VERSION: u8 = 1;
const HEADER_BYTES: usize = 3;
const TARGET_BYTES: usize = 32 + 32 + 8 + 32;
const TARGET_PREFIX: &str = "kk-msg-v1";
pub const MAX_TARGET_ID_BYTES: usize = TARGET_PREFIX.len() + 4 + 3 * 64 + 20;
pub const MAX_ENCODED_BYTES: usize = HEADER_BYTES + TARGET_BYTES + config::MAX_MESSAGE_BYTES;
pub const MAX_REACTION_ENTRIES: usize = 256;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Capabilities {
    pub epoch: u64,
    pub version: u8,
}

/// In-process evidence from authenticated capability queries, tied to one exact
/// epoch audience. It cannot be supplied by an HTTP/IPC JSON caller.
pub struct Audience {
    room: [u8; 32],
    epoch: u64,
    members: BTreeSet<[u8; 32]>,
}
impl Audience {
    pub(crate) fn negotiated(
        room: [u8; 32],
        epoch: u64,
        members: BTreeSet<[u8; 32]>,
    ) -> Result<Self> {
        if members.is_empty() || members.len() > config::ROOM_MAX_MEMBERS {
            return Err(Error::Protocol);
        }
        Ok(Self {
            room,
            epoch,
            members,
        })
    }
    pub(crate) fn validate(
        &self,
        room: [u8; 32],
        epoch: u64,
        members: BTreeSet<[u8; 32]>,
    ) -> Result<()> {
        if self.room != room || self.epoch != epoch || self.members != members {
            return Err(Error::Denied);
        }
        Ok(())
    }
}

/// An authenticated message identity. Local storage direction is not part of it:
/// OutgoingPrepared and IncomingProcessed must identify the same target on peers.
/// The target is not proof of existence and never grants remote history access.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub room: [u8; 32],
    pub member: [u8; 32],
    pub epoch: u64,
    pub wire_sha256: [u8; 32],
}
impl From<&super::group::StableReference> for Target {
    fn from(reference: &super::group::StableReference) -> Self {
        Self {
            room: reference.room,
            member: reference.member,
            epoch: reference.epoch,
            wire_sha256: reference.wire_sha256,
        }
    }
}
impl Target {
    pub fn validate(&self, room: [u8; 32], epoch: u64) -> Result<()> {
        if self.room != room || self.epoch > epoch {
            return Err(Error::Denied);
        }
        Ok(())
    }
    fn encode(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&self.room);
        bytes.extend_from_slice(&self.member);
        bytes.extend_from_slice(&self.epoch.to_be_bytes());
        bytes.extend_from_slice(&self.wire_sha256);
    }
    fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != TARGET_BYTES {
            return Err(Error::Protocol);
        }
        Ok(Self {
            room: bytes[..32].try_into().map_err(|_| Error::Protocol)?,
            member: bytes[32..64].try_into().map_err(|_| Error::Protocol)?,
            epoch: u64::from_be_bytes(bytes[64..72].try_into().map_err(|_| Error::Protocol)?),
            wire_sha256: bytes[72..].try_into().map_err(|_| Error::Protocol)?,
        })
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{TARGET_PREFIX}.")?;
        for byte in self.room {
            write!(formatter, "{byte:02x}")?;
        }
        formatter.write_str(".")?;
        for byte in self.member {
            write!(formatter, "{byte:02x}")?;
        }
        write!(formatter, ".{}.", self.epoch)?;
        for byte in self.wire_sha256 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}
impl std::str::FromStr for Target {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        if value.len() > MAX_TARGET_ID_BYTES {
            return Err(Error::Protocol);
        }
        let mut parts = value.split('.');
        if parts.next() != Some(TARGET_PREFIX) {
            return Err(Error::Protocol);
        }
        let room = target_hex(parts.next().ok_or(Error::Protocol)?)?;
        let member = target_hex(parts.next().ok_or(Error::Protocol)?)?;
        let epoch = parts.next().ok_or(Error::Protocol)?;
        if epoch.is_empty()
            || (epoch.len() > 1 && epoch.starts_with('0'))
            || !epoch.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(Error::Protocol);
        }
        let epoch = epoch.parse().map_err(|_| Error::Protocol)?;
        let wire_sha256 = target_hex(parts.next().ok_or(Error::Protocol)?)?;
        if parts.next().is_some() {
            return Err(Error::Protocol);
        }
        Ok(Self {
            room,
            member,
            epoch,
            wire_sha256,
        })
    }
}
fn target_hex(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::Protocol);
    }
    let mut bytes = [0; 32];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
            .map_err(|_| Error::Protocol)?;
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reaction {
    Like,
    Love,
    Laugh,
    Surprised,
    Sad,
    Thanks,
}
impl Reaction {
    fn byte(self) -> u8 {
        match self {
            Self::Like => 1,
            Self::Love => 2,
            Self::Laugh => 3,
            Self::Surprised => 4,
            Self::Sad => 5,
            Self::Thanks => 6,
        }
    }
    fn decode(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::Like),
            2 => Ok(Self::Love),
            3 => Ok(Self::Laugh),
            4 => Ok(Self::Surprised),
            5 => Ok(Self::Sad),
            6 => Ok(Self::Thanks),
            _ => Err(Error::Protocol),
        }
    }
}
impl std::str::FromStr for Reaction {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        match value {
            "like" => Ok(Self::Like),
            "love" => Ok(Self::Love),
            "laugh" => Ok(Self::Laugh),
            "surprised" => Ok(Self::Surprised),
            "sad" => Ok(Self::Sad),
            "thanks" => Ok(Self::Thanks),
            _ => Err(Error::Protocol),
        }
    }
}

/// Author is deliberately absent: only the authenticated MLS sender supplies it.
/// ReceivedText retains the existing zeroizing storage and redacted Debug policy.
#[derive(Debug)]
pub enum Operation {
    Text(ReceivedText),
    Reply {
        target: Target,
        text: ReceivedText,
    },
    Reaction {
        target: Target,
        value: Reaction,
        added: bool,
        sensitive: bool,
    },
}

/// Stored only inside the encrypted history row; author comes from its verified
/// group reference. Plain text has no relation and keeps the legacy row format.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Relation {
    Reply {
        target: Target,
    },
    Reaction {
        target: Target,
        value: Reaction,
        added: bool,
    },
}
impl Relation {
    pub fn validate(&self, room: [u8; 32], epoch: u64, body: &str) -> Result<()> {
        match self {
            Self::Reply { target } => target.validate(room, epoch),
            Self::Reaction { target, .. } => {
                target.validate(room, epoch)?;
                if !body.is_empty() {
                    return Err(Error::Protocol);
                }
                Ok(())
            }
        }
    }
}
impl Operation {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Reply { .. } => "reply",
            Self::Reaction { .. } => "reaction",
        }
    }
    pub fn body(&self) -> &str {
        match self {
            Self::Text(text) | Self::Reply { text, .. } => text.text(),
            Self::Reaction { .. } => "",
        }
    }
    pub fn relation(&self) -> Option<Relation> {
        match self {
            Self::Text(_) => None,
            Self::Reply { target, .. } => Some(Relation::Reply {
                target: target.clone(),
            }),
            Self::Reaction {
                target,
                value,
                added,
                ..
            } => Some(Relation::Reaction {
                target: target.clone(),
                value: *value,
                added: *added,
            }),
        }
    }
    pub fn sensitive(&self) -> bool {
        match self {
            Self::Text(text) | Self::Reply { text, .. } => text.sensitive,
            Self::Reaction { sensitive, .. } => *sensitive,
        }
    }
    pub fn target(&self) -> Option<&Target> {
        match self {
            Self::Text(_) => None,
            Self::Reply { target, .. } | Self::Reaction { target, .. } => Some(target),
        }
    }
    pub fn encode(&self, room: [u8; 32], epoch: u64) -> Result<Zeroizing<Vec<u8>>> {
        if let Some(target) = self.target() {
            target.validate(room, epoch)?;
        }
        let (kind, text) = match self {
            Self::Text(text) => (0, Some(text.text())),
            Self::Reply { text, .. } => (1, Some(text.text())),
            Self::Reaction { added, .. } => (if *added { 2 } else { 3 }, None),
        };
        let length = text.map_or(0, str::len);
        if length > config::MAX_MESSAGE_BYTES {
            return Err(Error::MessageTooLarge);
        }
        let size = HEADER_BYTES
            + self.target().map_or(0, |_| TARGET_BYTES)
            + length
            + usize::from(matches!(self, Self::Reaction { .. }));
        let mut bytes = Zeroizing::new(Vec::with_capacity(size));
        bytes.extend_from_slice(&[VERSION, kind, u8::from(self.sensitive())]);
        if let Some(target) = self.target() {
            target.encode(&mut bytes);
        }
        if let Some(text) = text {
            bytes.extend_from_slice(text.as_bytes());
        }
        if let Self::Reaction { value, .. } = self {
            bytes.push(value.byte());
        }
        Ok(bytes)
    }
    /// Call only after matching AAD and authenticating/authorizing the MLS sender.
    /// All lengths, kinds and target scope are checked before allocating plaintext.
    pub fn decode(bytes: &[u8], room: [u8; 32], epoch: u64) -> Result<Self> {
        if bytes.len() > MAX_ENCODED_BYTES {
            return Err(Error::MessageTooLarge);
        }
        let [version, kind, sensitive, tail @ ..] = bytes else {
            return Err(Error::Protocol);
        };
        if *version != VERSION || *sensitive > 1 {
            return Err(Error::Protocol);
        }
        if *kind == 0 {
            return Ok(Self::Text(ReceivedText::from_bytes(tail, *sensitive == 1)?));
        }
        if !matches!(*kind, 1..=3) || tail.len() < TARGET_BYTES {
            return Err(Error::Protocol);
        }
        let target = Target::decode(&tail[..TARGET_BYTES])?;
        target.validate(room, epoch)?;
        let body = &tail[TARGET_BYTES..];
        match *kind {
            1 => Ok(Self::Reply {
                target,
                text: ReceivedText::from_bytes(body, *sensitive == 1)?,
            }),
            2 | 3 if body.len() == 1 => Ok(Self::Reaction {
                target,
                value: Reaction::decode(body[0])?,
                added: *kind == 2,
                sensitive: *sensitive == 1,
            }),
            _ => Err(Error::Protocol),
        }
    }
}

/// A bounded presentation projection, not a membership or existence oracle.
/// Apply authenticated references in event order; a removal can only address
/// that event's MLS author, never a display name or a caller-supplied author.
#[derive(Default)]
pub struct Reactions {
    entries: BTreeSet<(Target, Reaction, [u8; 32])>,
}
#[derive(Serialize)]
pub struct ReactionSummary {
    pub target: Target,
    pub value: Reaction,
    pub members: Vec<[u8; 32]>,
}
impl Reactions {
    pub fn apply(
        &mut self,
        reference: &super::group::StableReference,
        operation: &Operation,
    ) -> Result<()> {
        let Operation::Reaction {
            target,
            value,
            added,
            sensitive,
        } = operation
        else {
            return Ok(());
        };
        target.validate(reference.room, reference.epoch)?;
        if *sensitive {
            return Ok(());
        }
        let entry = (target.clone(), *value, reference.member);
        if *added {
            if !self.entries.contains(&entry) && self.entries.len() >= MAX_REACTION_ENTRIES {
                return Err(Error::Limit {
                    resource: "reaction projection",
                    max: MAX_REACTION_ENTRIES,
                });
            }
            self.entries.insert(entry);
        } else {
            self.entries.remove(&entry);
        }
        Ok(())
    }
    pub fn summaries(self) -> Vec<ReactionSummary> {
        let mut summaries: Vec<ReactionSummary> = Vec::new();
        for (target, value, member) in self.entries {
            if let Some(last) = summaries
                .last_mut()
                .filter(|entry| entry.target == target && entry.value == value)
            {
                last.members.push(member);
            } else {
                summaries.push(ReactionSummary {
                    target,
                    value,
                    members: vec![member],
                });
            }
        }
        summaries
    }
}

#[cfg(test)]
#[path = "conversation_tests.rs"]
mod tests;
