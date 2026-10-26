//! Two bounded enrollment stages on the ALREADY proven room TLS connection.
//! This authenticates transport/purpose, not root/grant/application semantics.
use super::{Proposal, RoomPeer, Scope};
use crate::{Error, Result, config, transport};
use std::{future::Future, time::Duration};
use tokio::time::Instant;
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"KKENRL01";
const HEADER: usize = 8 + 32 + 1 + 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnrollmentStage {
    Authority,
    KeyPackage,
}
impl EnrollmentStage {
    fn index(self) -> u8 {
        match self {
            Self::Authority => 0,
            Self::KeyPackage => 1,
        }
    }
    fn limit(self) -> usize {
        match self {
            Self::Authority => config::ROOM_BOOTSTRAP_AUTHORITY_BYTES,
            Self::KeyPackage => config::ROOM_MAX_WIRE_BYTES,
        }
    }
}

/// Created only after PAKE, both approvals, exact-key TLS proof and both scoped
/// approval callbacks/receipts. No Serialize/Clone or implicit background task.
pub struct RoomSession {
    proposal: Proposal,
    peer: RoomPeer,
    endpoint: quinn::Endpoint,
    connection: quinn::Connection,
    expires: Instant,
    server: bool,
    next: u8,
    failed: bool,
}
impl RoomSession {
    pub(super) fn authenticated(
        proposal: Proposal,
        peer: RoomPeer,
        endpoint: quinn::Endpoint,
        connection: quinn::Connection,
        expires: Instant,
        server: bool,
    ) -> Result<Self> {
        if proposal.scope != Scope::Room(peer.room)
            || proposal.fingerprint != peer.peer.fingerprint()
        {
            return Err(Error::Authentication);
        }
        if expires <= Instant::now() {
            return Err(Error::Timeout);
        }
        Ok(Self {
            proposal,
            peer,
            endpoint,
            connection,
            expires,
            server,
            next: 0,
            failed: false,
        })
    }
    pub fn proposal(&self) -> &Proposal {
        &self.proposal
    }
    pub fn peer(&self) -> &RoomPeer {
        &self.peer
    }

    fn begin(
        &mut self,
        stage: EnrollmentStage,
        server: bool,
        deadline: Duration,
    ) -> Result<Duration> {
        transport::validate_duration(deadline, Duration::from_secs(300))?;
        if self.failed || self.server != server || self.next != stage.index() {
            return Err(Error::Protocol);
        }
        let remaining = self
            .expires
            .saturating_duration_since(Instant::now())
            .min(deadline);
        if remaining.is_zero() {
            return Err(Error::Timeout);
        }
        // Cancellation/errors leave this stage poisoned. Never silently retry a
        // handler that may already have committed an application effect.
        self.failed = true;
        Ok(remaining)
    }
    pub async fn request(
        &mut self,
        stage: EnrollmentStage,
        payload: &[u8],
        deadline: Duration,
    ) -> Result<Zeroizing<Vec<u8>>> {
        if payload.len() > stage.limit() {
            return Err(Error::Limit {
                resource: "room enrollment payload",
                max: stage.limit(),
            });
        }
        let remaining = self.begin(stage, false, deadline)?;
        let result = tokio::time::timeout(remaining, async {
            let (mut send, mut recv) = self
                .connection
                .open_bi()
                .await
                .map_err(|_| Error::Network)?;
            write(&mut send, self.peer.room, stage, payload).await?;
            read(&mut recv, self.peer.room, stage).await
        })
        .await
        .map_err(|_| Error::Timeout)?;
        if result.is_ok() {
            self.next += 1;
            self.failed = false;
        }
        result
    }
    /// Application validates and authorizes the request BEFORE producing a reply.
    /// An error/cancel can still follow an irreversible application COMMIT; the
    /// caller must preserve its recovery identity and never claim remote rollback.
    pub async fn respond<F, R>(
        &mut self,
        stage: EnrollmentStage,
        deadline: Duration,
        handle: F,
    ) -> Result<()>
    where
        F: FnOnce(Zeroizing<Vec<u8>>) -> R,
        R: Future<Output = Result<Vec<u8>>>,
    {
        let remaining = self.begin(stage, true, deadline)?;
        let result = tokio::time::timeout(remaining, async {
            let (mut send, mut recv) = self
                .connection
                .accept_bi()
                .await
                .map_err(|_| Error::Network)?;
            let payload = read(&mut recv, self.peer.room, stage).await?;
            let response = Zeroizing::new(handle(payload).await?);
            write(&mut send, self.peer.room, stage, &response).await
        })
        .await
        .map_err(|_| Error::Timeout)?;
        if result.is_ok() {
            self.next += 1;
            self.failed = false;
        }
        result
    }
    pub async fn close(self) -> Result<()> {
        let remaining = self
            .expires
            .saturating_duration_since(Instant::now())
            .min(config::DEFAULT_TIMEOUT);
        if remaining.is_zero() {
            self.connection.close(1u32.into(), b"");
            return Err(Error::Timeout);
        }
        if self.server && !self.failed {
            // Do not discard a final queued response by closing first. The
            // receiver closes after consuming it; this is lifecycle, not proof
            // of application validation or membership activation.
            tokio::time::timeout(remaining, transport::wait_closed(&self.connection))
                .await
                .map_err(|_| Error::Timeout)??;
        } else {
            self.connection
                .close(if self.failed { 1u32 } else { 0u32 }.into(), b"");
        }
        let remaining = self
            .expires
            .saturating_duration_since(Instant::now())
            .min(config::DEFAULT_TIMEOUT);
        tokio::time::timeout(remaining, self.endpoint.wait_idle())
            .await
            .map_err(|_| Error::Timeout)
    }
}
impl Drop for RoomSession {
    fn drop(&mut self) {
        self.connection.close(0u32.into(), b"");
    }
}

async fn write(
    send: &mut quinn::SendStream,
    room: [u8; 32],
    stage: EnrollmentStage,
    payload: &[u8],
) -> Result<()> {
    if payload.len() > stage.limit() {
        return Err(Error::Limit {
            resource: "room enrollment payload",
            max: stage.limit(),
        });
    }
    let mut header = [0; HEADER];
    header[..8].copy_from_slice(MAGIC);
    header[8..40].copy_from_slice(&room);
    header[40] = stage.index() + 1;
    header[41..].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    send.write_all(&header).await?;
    send.write_all(payload).await?;
    send.finish().map_err(|_| Error::Network)
}
async fn read(
    recv: &mut quinn::RecvStream,
    room: [u8; 32],
    stage: EnrollmentStage,
) -> Result<Zeroizing<Vec<u8>>> {
    let mut wire = Zeroizing::new(
        recv.read_to_end(stage.limit() + HEADER)
            .await
            .map_err(|_| Error::Protocol)?,
    );
    if wire.len() < HEADER
        || &wire[..8] != MAGIC
        || wire[8..40] != room
        || wire[40] != stage.index() + 1
    {
        return Err(Error::Protocol);
    }
    let length =
        u32::from_be_bytes(wire[41..HEADER].try_into().map_err(|_| Error::Protocol)?) as usize;
    if length > stage.limit() || length != wire.len() - HEADER {
        return Err(Error::Protocol);
    }
    Ok(Zeroizing::new(wire.split_off(HEADER)))
}

#[cfg(all(test, unix))]
mod tests;
