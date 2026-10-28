use clap::{Subcommand, ValueEnum};
use kk::{Error, Result, media::Source};
use std::io::{self, Write};

#[derive(Clone, Copy, ValueEnum)]
pub enum TrackKind {
    Audio,
    Camera,
    Screen,
}
#[cfg(unix)]
impl TrackKind {
    fn kind(self) -> kk::media::Kind {
        match self {
            Self::Audio => kk::media::Kind::Audio,
            Self::Camera => kk::media::Kind::Camera,
            Self::Screen => kk::media::Kind::Screen,
        }
    }
}

#[derive(Subcommand)]
pub enum RoomCommand {
    /// Encode a native source once, or replay an explicit RFC4571 interoperability file
    Publish {
        #[arg(value_enum)]
        kind: TrackKind,
        #[arg(
            long,
            required_unless_present = "rtp_file",
            conflicts_with = "rtp_file"
        )]
        device: Option<String>,
        #[arg(long)]
        rtp_file: Option<std::path::PathBuf>,
        #[arg(long, default_value_t = 0)]
        window: u64,
        #[arg(long)]
        to: Vec<String>,
        #[arg(long, required = true)]
        consent: bool,
    },
    /// Consent to exactly one member/source kind; no room-wide automatic playback
    Listen {
        #[arg(value_enum)]
        kind: TrackKind,
        #[arg(long)]
        from: String,
        /// Validate bounded RTP and report counts, without playback or recording
        #[arg(long)]
        inspect: bool,
        /// Render video on the explicit DISPLAY X11 server, not an automatic sink
        #[arg(long, conflicts_with = "inspect")]
        x11: bool,
        #[arg(long, required = true)]
        consent: bool,
    },
}

#[cfg(any(unix, test))]
fn playback_exit(exit: kk::media::Exit) -> Result<()> {
    if !exit.diagnostic.is_empty() {
        writeln!(
            io::stderr(),
            "NATIVE PLAYBACK {}",
            String::from_utf8_lossy(&exit.diagnostic).escape_debug()
        )?;
    }
    if !exit.success && (!exit.stopped || exit.code.is_some()) {
        return Err(Error::Configuration(
            "native decoder or display output failed",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod exit_tests {
    #[cfg(not(unix))]
    #[tokio::test]
    async fn room_media_refuses_without_local_ipc() {
        for command in [
            super::RoomCommand::Publish {
                kind: super::TrackKind::Audio,
                device: None,
                rtp_file: None,
                window: 0,
                to: Vec::new(),
                consent: true,
            },
            super::RoomCommand::Listen {
                kind: super::TrackKind::Audio,
                from: String::new(),
                inspect: true,
                x11: false,
                consent: true,
            },
        ] {
            let root = tempfile::tempdir().unwrap();
            let profile = root.path().join("absent-profile");
            assert!(matches!(
                super::room(command, &profile, "room").await,
                Err(kk::Error::UnsupportedPlatform)
            ));
            assert!(!profile.exists());
        }
    }

    #[test]
    fn a_failed_decoder_is_not_reported_as_intentional_playback_shutdown() {
        assert!(
            super::playback_exit(kk::media::Exit {
                success: false,
                stopped: false,
                code: Some(1),
                diagnostic: Default::default()
            })
            .is_err()
        );
        assert!(
            super::playback_exit(kk::media::Exit {
                success: false,
                stopped: true,
                code: None,
                diagnostic: Default::default()
            })
            .is_ok()
        );
        assert!(
            super::playback_exit(kk::media::Exit {
                success: false,
                stopped: true,
                code: Some(1),
                diagnostic: Default::default()
            })
            .is_err()
        );
    }
}

#[cfg(not(unix))]
pub(super) async fn room(
    _command: RoomCommand,
    _profile: &std::path::Path,
    _name: &str,
) -> Result<()> {
    Err(Error::UnsupportedPlatform)
}

#[cfg(unix)]
pub(super) async fn room(
    command: RoomCommand,
    profile: &std::path::Path,
    name: &str,
) -> Result<()> {
    use kk::{
        media::{Kind as MediaKind, Sink},
        rooms::{
            control::raft::PeerId,
            local::media::{Publisher, Receiver},
        },
    };
    match command {
        RoomCommand::Publish {
            kind,
            device,
            rtp_file,
            window,
            to,
            consent,
        } => {
            if !consent {
                return Err(Error::Denied);
            }
            let kind = kind.kind();
            let peers = to
                .into_iter()
                .map(|peer| {
                    PeerId::try_from(peer).map_err(|_| {
                        Error::Configuration("media recipients require full certificate digests")
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let mut publication = Publisher::connect(profile, name, kind, peers).await?;
            writeln!(
                io::stderr(),
                "MEDIA PUBLISHING {:?} recipients={:?}; datagrams are not delivery receipts",
                kind,
                publication.recipients()
            )?;
            let mut native = if rtp_file.is_none() {
                let device = device.ok_or(Error::Configuration(
                    "native media requires an explicit device/display",
                ))?;
                let source = match kind {
                    MediaKind::Audio => Source::Microphone { device },
                    MediaKind::Camera => Source::Camera { device },
                    MediaKind::Screen => Source::ScreenX11 {
                        display: device,
                        window,
                    },
                };
                Some(source.capture()?)
            } else {
                None
            };
            let mut file = rtp_file
                .map(|path| kk::media::rtp_file(&path))
                .transpose()?;
            let mut stopped = false;
            let forwarded = {
                let mut input: &mut (dyn tokio::io::AsyncRead + Unpin) = match &mut native {
                    Some(pipeline) => pipeline.output.as_mut().ok_or(Error::Protocol)?,
                    None => file.as_mut().ok_or(Error::Protocol)?,
                };
                match publication
                    .relay(&mut input, async {
                        tokio::signal::ctrl_c().await.map_err(Error::Io)
                    })
                    .await
                {
                    Ok((count, requested)) => {
                        stopped = requested;
                        Ok(count)
                    }
                    Err(error) => Err(error),
                }
            };
            let cleanup = if let Some(native) = native {
                let exit = native.shutdown().await?;
                if !stopped && !exit.success {
                    Err(Error::Configuration(
                        "native capture stopped unexpectedly; check device and OS permission",
                    ))
                } else {
                    Ok(())
                }
            } else {
                Ok(())
            };
            let completed = publication.finish().await;
            let mut reply = completed?;
            if reply.ok {
                if let Err(error) = &forwarded {
                    reply.ok = false;
                    reply.code = Some(error.code().into());
                    reply.result["local_error"] = serde_json::json!(error.to_string());
                }
                if let Err(error) = &cleanup {
                    reply.ok = false;
                    reply.code = Some(error.code().into());
                    reply.result["capture_error"] = serde_json::json!(error.to_string());
                }
            }
            writeln!(
                io::stdout(),
                "{}",
                serde_json::to_string(&reply).map_err(|_| Error::Protocol)?
            )?;
            reply.check()?;
            forwarded?;
            cleanup
        }
        RoomCommand::Listen {
            kind,
            from,
            inspect,
            x11,
            consent,
        } => {
            if !consent {
                return Err(Error::Denied);
            }
            let kind = kind.kind();
            if x11 && kind == MediaKind::Audio {
                return Err(Error::Configuration("X11 playback requires a video source"));
            }
            let peer = PeerId::try_from(from).map_err(|_| {
                Error::Configuration("media source requires its full certificate digest")
            })?;
            let mut receiver = Receiver::connect(profile, name, kind, peer).await?;
            writeln!(
                io::stderr(),
                "MEDIA LISTENING {:?} source={} playback={}",
                kind,
                peer,
                !inspect
            )?;
            let mut sink: Option<kk::media::Pipeline> = None;
            let mut track = None;
            let mut packets = 0u64;
            let mut bytes = 0u64;
            let processing = async {
                loop {
                    let frame = tokio::select! {
                        signal = tokio::signal::ctrl_c() => { signal?; break; },
                        frame = receiver.next() => frame?,
                    };
                    let Some(frame) = frame else {
                        break;
                    };
                    if !inspect {
                        if track != Some(frame.track) {
                            if let Some(old) = sink.take() {
                                playback_exit(old.shutdown().await?)?;
                            }
                            sink = Some(
                                Sink {
                                    codec: kind.codec(),
                                }
                                .playback_with(x11)?,
                            );
                            track = Some(frame.track);
                        }
                        let input = sink
                            .as_mut()
                            .and_then(|sink| sink.input.as_mut())
                            .ok_or(Error::Protocol)?;
                        tokio::time::timeout(
                            kk::config::MEDIA_PACKET_MAX_AGE,
                            frame.packet.write(input),
                        )
                        .await
                        .map_err(|_| Error::Timeout)??;
                    }
                    packets += 1;
                    bytes += frame.packet.bytes().len() as u64;
                }
                Ok::<(), Error>(())
            }
            .await;
            drop(receiver);
            let cleanup = if let Some(sink) = sink {
                sink.shutdown().await.and_then(playback_exit)
            } else {
                Ok(())
            };
            processing?;
            cleanup?;
            writeln!(
                io::stdout(),
                "{}",
                serde_json::json!({"packets": packets, "bytes": bytes, "playback": !inspect, "recorded": false})
            )?;
            Ok(())
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Kind {
    Microphone,
    Camera,
    ScreenX11,
}

#[derive(Subcommand)]
pub enum Command {
    /// Inspect native plugin availability; never opens capture devices
    Doctor,
    /// Explicit local native encode/decode preview, not room network transmission
    Preview {
        #[arg(value_enum)]
        source: Kind,
        /// Pulse device, /dev/videoN or explicit X display; no implicit fallback
        #[arg(long)]
        device: String,
        /// Explicit X11 window ID; zero deliberately selects that display's root
        #[arg(long, default_value_t = 0)]
        window: u64,
        /// Authorize capture and local playback on this device
        #[arg(long, required = true)]
        consent: bool,
    },
}

pub(super) async fn run(command: Command) -> Result<()> {
    match command {
        Command::Doctor => {
            let plugins = kk::media::doctor().await?;
            writeln!(
                io::stdout(),
                "{}",
                serde_json::to_string(&plugins).map_err(|_| Error::Protocol)?
            )?;
            Ok(())
        }
        Command::Preview {
            source,
            device,
            window,
            consent,
        } => {
            if !consent {
                return Err(Error::Denied);
            }
            let source = match source {
                Kind::Microphone => Source::Microphone { device },
                Kind::Camera => Source::Camera { device },
                Kind::ScreenX11 => Source::ScreenX11 {
                    display: device,
                    window,
                },
            };
            let mut process = source.preview()?;
            let outcome = tokio::select! {
                signal = tokio::signal::ctrl_c() => Some(signal),
                result = process.finished() => {
                    let exit = result?;
                    if !exit.success { return Err(Error::Configuration("native preview failed; verify the device, plugins and OS permission")); }
                    None
                },
            };
            if let Some(signal) = outcome {
                let _exit = process.shutdown().await?;
                signal?;
            }
            Ok(())
        }
    }
}
