//! Private, mutable nicknames for room members, never authenticated self-names.
//! Callers must obtain the full peer ID from authenticated room membership, not
//! resolve identity from a nickname. These overrides confer no contact grant,
//! certificate verification or trust, and are never announced to other peers.

use crate::{Error, Result, rooms::control::raft};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const FILE: &str = "labels.json";
const VERSION: u32 = 1;
const MAX_LABELS: usize = 512;

/// Provenance of presentation text, never a trust or identity assertion.
/// In particular, a local override is not a remote self-announcement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationSource {
    SelfProfile,
    LocalOverride,
    Unknown,
}

/// Shared local presentation for an authenticated member ID supplied by a caller.
/// Authorization, identity matching and media routing must continue to use `id`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MemberPresentation {
    pub id: raft::PeerId,
    pub display_name: String,
    pub source: PresentationSource,
    pub is_self: bool,
}

/// Resolve only supplied member IDs, preserving their order. The caller supplies
/// authenticated membership and its own full peer ID; neither names nor aliases
/// establish membership. Self uses local settings even if a self alias exists.
/// Missing remote names are explicitly unknown, not verified announcements.
/// This reads metadata only and never creates files or loads identity keys.
pub fn present(
    profile: &Path,
    room: [u8; 32],
    self_peer: raft::PeerId,
    members: &[raft::PeerId],
) -> Result<Vec<MemberPresentation>> {
    if members.len() > crate::config::ROOM_MAX_MEMBERS {
        return Err(Error::Configuration("too many presentation members"));
    }
    let mut unique = BTreeSet::new();
    if members.iter().any(|id| !unique.insert(*id)) {
        return Err(Error::Configuration("duplicate presentation member"));
    }
    let settings = super::Settings::load(profile)?;
    let aliases = list(profile, room)?;
    Ok(members
        .iter()
        .map(|&id| {
            let is_self = id == self_peer;
            let (display_name, source) = if is_self {
                (
                    settings.display_name.clone(),
                    PresentationSource::SelfProfile,
                )
            } else if let Some(name) = aliases.get(&id) {
                (name.clone(), PresentationSource::LocalOverride)
            } else {
                ("Unnamed member".into(), PresentationSource::Unknown)
            };
            MemberPresentation {
                id,
                display_name,
                source,
                is_self,
            }
        })
        .collect())
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Alias {
    room: [u8; 32],
    peer: raft::PeerId,
    name: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Labels {
    version: u32,
    aliases: Vec<Alias>,
}

impl Default for Labels {
    fn default() -> Self {
        Self {
            version: VERSION,
            aliases: Vec::new(),
        }
    }
}

impl Labels {
    fn validate(&self) -> Result<()> {
        if self.version != VERSION {
            return Err(Error::Configuration("unsupported local labels version"));
        }
        if self.aliases.len() > MAX_LABELS {
            return Err(Error::Configuration("too many local labels"));
        }
        let mut ids = BTreeSet::new();
        for alias in &self.aliases {
            super::settings::validate_name(&alias.name)?;
            if !ids.insert((alias.room, alias.peer)) {
                return Err(Error::Configuration("duplicate local label binding"));
            }
        }
        Ok(())
    }
}

/// Read only the given profile's local overrides for this exact room ID.
/// Missing metadata yields an empty map without creating files, scanning rooms,
/// or reading keys. Every stored entry is validated, even in other rooms.
pub fn list(profile: &Path, room: [u8; 32]) -> Result<BTreeMap<raft::PeerId, String>> {
    let labels: Labels = super::storage::read(profile, FILE)?;
    labels.validate()?;
    Ok(labels
        .aliases
        .into_iter()
        .filter(|alias| alias.room == room)
        .map(|alias| (alias.peer, alias.name))
        .collect())
}

/// Set an explicit local presentation override, or remove only this binding.
/// Requires an existing private profile directory; never generates an identity.
/// Shared storage serializes writers and bounds the entire JSON to 128 KiB.
pub fn set(profile: &Path, room: [u8; 32], peer: raft::PeerId, name: Option<String>) -> Result<()> {
    super::private_directory(profile, false)?;
    if let Some(name) = &name {
        super::settings::validate_name(name)?;
    }
    super::storage::update(profile, FILE, |labels: &mut Labels| {
        labels.validate()?;
        let index = labels
            .aliases
            .iter()
            .position(|alias| alias.room == room && alias.peer == peer);
        match (index, name) {
            (Some(index), Some(name)) => labels.aliases[index].name = name,
            (Some(index), None) => {
                labels.aliases.remove(index);
            }
            (None, Some(name)) => labels.aliases.push(Alias { room, peer, name }),
            (None, None) => {}
        }
        labels.validate()
    })?;
    Ok(())
}

#[cfg(all(test, unix))]
#[path = "labels_tests.rs"]
mod tests;
