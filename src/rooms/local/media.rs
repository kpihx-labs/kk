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
use zeroize::{Zeroize, Zeroizing};

const OPEN_TIMEOUT: Duration = Duration::from_secs(30);
// Zero is not a live wall-clock packet timestamp. Reserve it for a bounded
// terminal reason; no source/device/output identifiers enter this protocol.
const SOURCE_EOF: u8 = 0;
const REVOKED: u8 = 1;
const DISCONNECTED: u8 = 2;
const TIMED_OUT: u8 = 3;

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
    pending: Zeroizing<Vec<u8>>,
}
impl Receiver {
    /// After SOURCE_EOF the IPC owner retains authorization until this receiver
    /// is dropped. Continue watching revocation while the native decoder drains.
    pub async fn cancelled(&mut self) -> Result<()> {
        match self.next().await {
            Err(error) => Err(error),
            _ => Err(Error::Protocol),
        }
    }
    pub async fn connect(profile: &Path, name: &str, kind: Kind, peer: PeerId) -> Result<Self> {
        let (stream, _) = open(profile, name, &Command::MediaListen { kind, peer }).await?;
        Ok(Self {
            stream,
            kind,
            pending: Zeroizing::new(Vec::new()),
        })
    }
    // read_buf is cancellation-safe; partial headers/payloads survive selects
    // (for example a browser ping) without losing local stream boundaries.
    async fn fill(&mut self, length: usize) -> Result<()> {
        while self.pending.len() < length {
            let needed = length - self.pending.len();
            if (&mut self.stream)
                .take(needed as u64)
                .read_buf(&mut *self.pending)
                .await?
                == 0
            {
                return Err(if self.pending.is_empty() {
                    Error::Cancelled
                } else {
                    Error::Protocol
                });
            }
        }
        Ok(())
    }
    pub async fn next(
        &mut self,
    ) -> Result<Option<crate::rooms::control::raft::network::MediaFrame>> {
        loop {
            self.fill(8).await?;
            let received_ms =
                u64::from_be_bytes(self.pending[..8].try_into().map_err(|_| Error::Protocol)?);
            if received_ms == 0 {
                self.fill(9).await?;
                let reason = self.pending[8];
                self.pending.zeroize();
                return match reason {
                    SOURCE_EOF => Ok(None),
                    REVOKED => Err(Error::Denied),
                    DISCONNECTED => Err(Error::Cancelled),
                    TIMED_OUT => Err(Error::Timeout),
                    _ => Err(Error::Protocol),
                };
            }
            let mut offset = 24;
            let mut packet_length = 0;
            for _ in 0..16 {
                self.fill(offset + 2).await?;
                packet_length =
                    u16::from_be_bytes([self.pending[offset], self.pending[offset + 1]]) as usize;
                offset += 2;
                if packet_length != 0 {
                    break;
                }
            }
            if packet_length == 0 || packet_length > config::MEDIA_PACKET_BYTES {
                return Err(Error::Protocol);
            }
            self.fill(offset + packet_length).await?;
            let track = self.pending[8..24]
                .try_into()
                .map_err(|_| Error::Protocol)?;
            let packet = Packet::parse(self.kind.codec(), self.pending.split_off(offset))?;
            self.pending.zeroize();
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
            next = subscription.next_frame() => next,
        };
        let frame = match next {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                tokio::select! {
                    biased;
                    _ = subscription.revoked() => return terminal(&mut stream, REVOKED).await,
                    written = terminal_event(&mut stream, SOURCE_EOF) => written?,
                }
                let revoked = tokio::select! {
                    biased;
                    revoked = subscription.revoked() => revoked,
                    closed = stream.read(&mut probe) => { let _ = closed?; return Ok(()); },
                    _ = tokio::time::sleep(config::DEFAULT_TIMEOUT + config::QUIC_SHUTDOWN_GRACE) => return terminal(&mut stream, TIMED_OUT).await,
                };
                return terminal(
                    &mut stream,
                    if matches!(revoked, Err(Error::Denied)) {
                        REVOKED
                    } else {
                        DISCONNECTED
                    },
                )
                .await;
            }
            Err(Error::Denied) => return terminal(&mut stream, REVOKED).await,
            Err(Error::Cancelled) => return terminal(&mut stream, DISCONNECTED).await,
            Err(Error::Timeout) => return terminal(&mut stream, TIMED_OUT).await,
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

async fn terminal(stream: &mut UnixStream, reason: u8) -> Result<()> {
    terminal_event(stream, reason).await?;
    stream.shutdown().await?;
    Ok(())
}
async fn terminal_event(stream: &mut UnixStream, reason: u8) -> Result<()> {
    tokio::time::timeout(config::MEDIA_PACKET_MAX_AGE, async {
        stream.write_all(&[0; 8]).await?;
        stream.write_u8(reason).await
    })
    .await
    .map_err(|_| Error::Timeout)??;
    Ok(())
}

#[cfg(test)]
mod terminal_tests {
    use super::*;
    #[tokio::test]
    async fn interrupted_media_reads_preserve_framing_through_eof_and_revocation() {
        let packet = Packet::parse(
            crate::media::Codec::Opus,
            vec![0x80, 96, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 7],
        )
        .unwrap();
        let mut wire = crate::media::now_ms().unwrap().to_be_bytes().to_vec();
        wire.extend([9; 16]);
        // Existing RFC4571 null-packet interoperability remains supported.
        wire.extend([0, 0]);
        packet.write(&mut wire).await.unwrap();
        for split in [1, 7, 8, 15, 23, 24, 25, 26, 27, wire.len() - 1] {
            let (client, mut server) = UnixStream::pair().unwrap();
            let mut receiver = Receiver {
                stream: client,
                kind: Kind::Audio,
                pending: Zeroizing::new(Vec::new()),
            };
            server.write_all(&wire[..split]).await.unwrap();
            {
                let mut next = Box::pin(receiver.next());
                tokio::select! {
                    biased;
                    _ = &mut next => panic!("partial local media frame completed prematurely"),
                    _ = tokio::task::yield_now() => {},
                }
            }
            server.write_all(&wire[split..]).await.unwrap();
            terminal_event(&mut server, SOURCE_EOF).await.unwrap();
            terminal_event(&mut server, REVOKED).await.unwrap();
            let frame = receiver.next().await.unwrap().unwrap();
            assert_eq!(frame.track, [9; 16]);
            assert_eq!(frame.packet.bytes(), packet.bytes());
            assert!(receiver.next().await.unwrap().is_none());
            assert!(matches!(receiver.cancelled().await, Err(Error::Denied)));
        }
        for split in 1..9 {
            let (client, mut server) = UnixStream::pair().unwrap();
            let mut receiver = Receiver {
                stream: client,
                kind: Kind::Audio,
                pending: Zeroizing::new(Vec::new()),
            };
            let mut marker = [0; 9];
            marker[8] = REVOKED;
            server.write_all(&marker[..split]).await.unwrap();
            {
                let mut next = Box::pin(receiver.next());
                tokio::select! { biased; _ = &mut next => panic!("partial marker completed"), _ = tokio::task::yield_now() => {} }
            }
            server.write_all(&marker[split..]).await.unwrap();
            assert!(matches!(receiver.next().await, Err(Error::Denied)));
        }
    }
    #[tokio::test]
    async fn media_terminal_markers_distinguish_eof_revocation_legacy_and_truncation() {
        for reason in [SOURCE_EOF, REVOKED, DISCONNECTED, TIMED_OUT, 255] {
            let (client, mut server) = UnixStream::pair().unwrap();
            let mut receiver = Receiver {
                stream: client,
                kind: Kind::Screen,
                pending: Zeroizing::new(Vec::new()),
            };
            terminal_event(&mut server, reason).await.unwrap();
            match reason {
                SOURCE_EOF => assert!(receiver.next().await.unwrap().is_none()),
                REVOKED => assert!(matches!(receiver.next().await, Err(Error::Denied))),
                DISCONNECTED => assert!(matches!(receiver.next().await, Err(Error::Cancelled))),
                TIMED_OUT => assert!(matches!(receiver.next().await, Err(Error::Timeout))),
                _ => assert!(matches!(receiver.next().await, Err(Error::Protocol))),
            }
        }
        for prefix in [vec![], vec![0; 4], vec![0; 8]] {
            let (client, mut server) = UnixStream::pair().unwrap();
            let mut receiver = Receiver {
                stream: client,
                kind: Kind::Audio,
                pending: Zeroizing::new(Vec::new()),
            };
            server.write_all(&prefix).await.unwrap();
            server.shutdown().await.unwrap();
            assert!(
                receiver.next().await.is_err(),
                "legacy/truncated EOF is cancellation, never a natural drain boundary"
            );
        }
    }
    #[tokio::test]
    async fn media_revocation_queued_after_eof_still_cancels_decoder_drain() {
        let (client, mut server) = UnixStream::pair().unwrap();
        let mut receiver = Receiver {
            stream: client,
            kind: Kind::Audio,
            pending: Zeroizing::new(Vec::new()),
        };
        terminal_event(&mut server, SOURCE_EOF).await.unwrap();
        terminal_event(&mut server, REVOKED).await.unwrap();
        assert!(receiver.next().await.unwrap().is_none());
        assert!(matches!(receiver.cancelled().await, Err(Error::Denied)));
    }
}
