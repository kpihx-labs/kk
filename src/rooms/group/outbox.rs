//! Durable encrypted delivery artifacts, not MLS secret snapshots. Welcome stays
//! available across process death until the runtime verifies recipient delivery.
use super::*;
use std::fmt::Write;

fn name(digest: [u8; 32]) -> String {
    let mut value = String::from("welcome-");
    for byte in digest {
        write!(value, "{byte:02x}").expect("writing into a String");
    }
    value
}
pub(super) fn validate(actor: &CryptoMember, outgoing: &[[u8; 32]], extra: usize) -> Result<()> {
    if outgoing.len() > config::ROOM_WELCOME_OUTBOX_COUNT
        || outgoing.iter().collect::<BTreeSet<_>>().len() != outgoing.len()
    {
        return Err(Error::Storage("invalid welcome outbox index"));
    }
    let mut bytes = extra;
    for digest in outgoing {
        let wire = actor
            .provider
            .read_blob(&name(*digest))?
            .ok_or(Error::Storage("encrypted welcome artifact is missing"))?;
        if wire.len() > config::ROOM_MAX_WIRE_BYTES {
            return Err(Error::Storage("invalid welcome artifact size"));
        }
        bytes = bytes
            .checked_add(wire.len())
            .ok_or(Error::Storage("welcome outbox size overflow"))?;
    }
    if bytes > config::ROOM_WELCOME_OUTBOX_BYTES {
        return Err(Error::Limit {
            resource: "welcome outbox",
            max: config::ROOM_WELCOME_OUTBOX_BYTES,
        });
    }
    Ok(())
}
impl Group {
    pub(super) fn check_outgoing_budget(&self, extra: usize) -> Result<()> {
        if extra != 0 && self.outgoing.len() >= config::ROOM_WELCOME_OUTBOX_COUNT {
            return Err(Error::Configuration(
                "welcome outbox is full; verified delivery is required",
            ));
        }
        validate(
            &self.actor,
            &self.outgoing.iter().copied().collect::<Vec<_>>(),
            extra,
        )
    }
    pub(super) fn save_outgoing(&mut self, digest: [u8; 32], wire: &[u8]) -> Result<()> {
        self.check_outgoing_budget(wire.len())?;
        self.actor.provider.write_blob(&name(digest), wire)?;
        self.outgoing.insert(digest);
        Ok(())
    }
    /// Only encrypted public delivery material is returned. The runtime must still
    /// authorize its recipient using the accepted membership record.
    pub fn committed_welcome(&self, commit: &[u8]) -> Result<Option<Vec<u8>>> {
        if self.poisoned {
            return Err(Error::Storage(
                "welcome delivery requires verified recovery",
            ));
        }
        bound(commit)?;
        let digest: [u8; 32] = Sha256::digest(commit).into();
        if !self.outgoing.contains(&digest) {
            return Ok(None);
        }
        self.actor
            .provider
            .read_blob(&name(digest))?
            .map_or(Err(Error::Storage("welcome artifact missing")), |wire| {
                Ok(Some(wire))
            })
    }
    /// A timeout is not delivery. Call only after authenticated recipient
    /// acknowledgement; removal and the index update commit atomically.
    pub fn acknowledge_welcome(&mut self, commit: &[u8]) -> Result<()> {
        bound(commit)?;
        let digest: [u8; 32] = Sha256::digest(commit).into();
        self.transactional(|group| {
            if group.outgoing.remove(&digest) {
                group.actor.provider.delete_blob(&name(digest))?;
            }
            Ok(())
        })
    }
}

#[cfg(all(test, unix))]
mod tests;
