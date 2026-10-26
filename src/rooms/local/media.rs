//! Explicit same-user RTP sessions. Capture devices and executable arguments
//! never enter this IPC protocol or any network offer.
use super::{Command, Reply, ReplyEvent, directory};
use crate::{
    Error, Result, config,
    media::{Kind, Packet},
    node::ipc,
    rooms::control::raft::{PeerId, service::ControlNode},
};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

const OPEN_TIMEOUT: Duration = Duration::from_secs(30);

#[cfg(test)]
mod tests;

async fn open(profile: &Path, name: &str, command: &Command) -> Result<(UnixStream, Vec<PeerId>)> {
    let directory = directory(profile, name)?;
    tokio::time::timeout(OPEN_TIMEOUT, async {
        let mut stream = ipc::connect(&directory)
            .await?
            .ok_or(Error::Node("no active room"))?;
        ipc::write(&mut stream, command).await?;
        match ipc::read(&mut stream).await? {
            ReplyEvent::MediaReady { peers } => Ok((stream, peers)),
            ReplyEvent::Completed(reply) => {
                reply.check()?;
                Err(Error::Protocol)
            }
            _ => Err(Error::Protocol),
        }
    })
    .await
    .map_err(|_| Error::Timeout)?
}

pub struct Publisher {
    stream: UnixStream,
    kind: Kind,
    peers: Vec<PeerId>,
}
impl Publisher {
    pub async fn connect(
        profile: &Path,
        name: &str,
        kind: Kind,
        peers: Vec<PeerId>,
    ) -> Result<Self> {
        let (stream, peers) = open(profile, name, &Command::MediaPublish { kind, peers }).await?;
        Ok(Self {
            stream,
            kind,
            peers,
        })
    }
    pub fn recipients(&self) -> &[PeerId] {
        &self.peers
    }
    pub async fn send(&mut self, packet: &Packet) -> Result<()> {
        if packet.codec() != self.kind.codec() {
            return Err(Error::Protocol);
        }
        tokio::time::timeout(config::MEDIA_SOURCE_IDLE, packet.write(&mut self.stream))
            .await
            .map_err(|_| Error::Timeout)?
    }
    /// Relay bounded RTP with pacing. Use `stop` for a graceful boundary: an
    /// in-progress IPC packet is completed before this returns `(packets, true)`.
    pub async fn relay(
        &mut self,
        input: &mut (impl tokio::io::AsyncRead + Unpin),
        stop: impl std::future::Future<Output = Result<()>>,
    ) -> Result<(u64, bool)> {
        tokio::pin!(stop);
        let start = tokio::time::Instant::now();
        let mut origin = None;
        let mut ssrc = None;
        let mut count = 0;
        loop {
            let packet = tokio::select! {
                biased;
                result = &mut stop => { result?; return Ok((count, true)); },
                packet = Packet::read(self.kind.codec(), input) => packet?,
            };
            let Some(packet) = packet else {
                break;
            };
            let first = *origin.get_or_insert(packet.timestamp());
            if *ssrc.get_or_insert(packet.ssrc()) != packet.ssrc() {
                return Err(Error::Protocol);
            }
            let offset = Duration::from_nanos(
                packet.timestamp().wrapping_sub(first) as u64 * 1_000_000_000
                    / self.kind.codec().clock_rate() as u64,
            );
            let due = start + offset;
            if offset > config::MEDIA_MAX_DURATION
                || due > tokio::time::Instant::now() + config::MEDIA_SOURCE_IDLE
            {
                return Err(Error::Protocol);
            }
            tokio::select! {
                biased;
                result = &mut stop => { result?; return Ok((count, true)); },
                _ = tokio::time::sleep_until(due) => {},
            }
            // Do not select cancellation around this framed write. Its own idle
            // deadline remains bounded; finish() may only follow a whole packet.
            self.send(&packet).await?;
            count += 1;
        }
        if count == 0 {
            return Err(Error::Protocol);
        }
        Ok((count, false))
    }
    pub async fn finish(mut self) -> Result<Reply> {
        let closed = self.stream.shutdown().await;
        let result = tokio::time::timeout(OPEN_TIMEOUT, ipc::read(&mut self.stream))
            .await
            .map_err(|_| Error::Timeout)??;
        match result {
            ReplyEvent::Completed(reply) => {
                if reply.ok {
                    closed?;
                }
                Ok(reply)
            }
            _ => Err(Error::Protocol),
        }
    }
}

pub struct Receiver {
    stream: UnixStream,
    kind: Kind,
}
impl Receiver {
    pub async fn connect(profile: &Path, name: &str, kind: Kind, peer: PeerId) -> Result<Self> {
        let (stream, _) = open(profile, name, &Command::MediaListen { kind, peer }).await?;
        Ok(Self { stream, kind })
    }
    pub async fn next(
        &mut self,
    ) -> Result<Option<crate::rooms::control::raft::network::MediaFrame>> {
        loop {
            let mut stamp = [0; 8];
            if self.stream.read(&mut stamp[..1]).await? == 0 {
                return Ok(None);
            }
            self.stream.read_exact(&mut stamp[1..]).await?;
            let received_ms = u64::from_be_bytes(stamp);
            let mut track = [0; 16];
            self.stream.read_exact(&mut track).await?;
            let packet = Packet::read(self.kind.codec(), &mut self.stream)
                .await?
                .ok_or(Error::Protocol)?;
            let now = crate::media::now_ms()?;
            if now >= received_ms
                && now - received_ms <= config::MEDIA_PACKET_MAX_AGE.as_millis() as u64
            {
                return Ok(Some(crate::rooms::control::raft::network::MediaFrame {
                    track,
                    packet,
                    received_ms,
                }));
            }
        }
    }
}

async fn refused(stream: &mut UnixStream, error: Error) -> Result<()> {
    let reply = Reply {
        ok: false,
        code: Some(error.code().into()),
        result: serde_json::json!({"error": error.to_string()}),
    };
    ipc::write(stream, &ReplyEvent::Completed(reply)).await
}

pub(super) async fn publish(
    mut stream: UnixStream,
    control: Arc<ControlNode>,
    kind: Kind,
    peers: Vec<PeerId>,
) -> Result<()> {
    let mut publication =
        match tokio::time::timeout(OPEN_TIMEOUT, control.publish_media(kind, peers)).await {
            Ok(Ok(publication)) => publication,
            Ok(Err(error)) => return refused(&mut stream, error).await,
            Err(_) => return refused(&mut stream, Error::Timeout).await,
        };
    ipc::write(
        &mut stream,
        &ReplyEvent::MediaReady {
            peers: publication.recipients(),
        },
    )
    .await?;
    let forwarding = tokio::time::timeout(config::MEDIA_MAX_DURATION, async {
        loop {
            let packet = tokio::time::timeout(
                config::MEDIA_SOURCE_IDLE,
                Packet::read(kind.codec(), &mut stream),
            )
            .await
            .map_err(|_| Error::Timeout)??;
            let Some(packet) = packet else {
                break;
            };
            publication.send(&packet).await?;
        }
        Ok::<(), Error>(())
    })
    .await
    .unwrap_or(Err(Error::Timeout));
    let statistics = publication.finish().await?;
    let reply = match forwarding {
        Ok(()) => Reply {
            ok: true,
            code: None,
            result: serde_json::json!({"statistics": statistics, "meaning": "queued-QUIC-datagrams-not-receipt-or-human-playback"}),
        },
        Err(error) => Reply {
            ok: false,
            code: Some(error.code().into()),
            result: serde_json::json!({"error": error.to_string(), "statistics": statistics}),
        },
    };
    ipc::write(&mut stream, &ReplyEvent::Completed(reply)).await
}

pub(super) async fn listen(
    mut stream: UnixStream,
    control: Arc<ControlNode>,
    kind: Kind,
    peer: PeerId,
) -> Result<()> {
    let mut subscription =
        match tokio::time::timeout(OPEN_TIMEOUT, control.subscribe_media(peer, kind)).await {
            Ok(Ok(subscription)) => subscription,
            Ok(Err(error)) => return refused(&mut stream, error).await,
            Err(_) => return refused(&mut stream, Error::Timeout).await,
        };
    ipc::write(&mut stream, &ReplyEvent::MediaReady { peers: vec![peer] }).await?;
    loop {
        let mut probe = [0];
        let next = tokio::select! {
            // Closing the local write half withdraws consent even with no source.
            closed = stream.read(&mut probe) => { let _ = closed?; return Ok(()); },
            next = subscription.next() => next,
        };
        let frame = match next {
            Ok(frame) => frame,
            Err(Error::Cancelled | Error::Denied) => return Ok(()),
            Err(error) => return Err(error),
        };
        tokio::time::timeout(config::MEDIA_PACKET_MAX_AGE, async {
            stream.write_all(&frame.received_ms.to_be_bytes()).await?;
            stream.write_all(&frame.track).await?;
            frame.packet.write(&mut stream).await
        })
        .await
        .map_err(|_| Error::Timeout)??;
    }
}
