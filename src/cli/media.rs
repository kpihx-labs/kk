use clap::{Args, Subcommand, ValueEnum};
use kk::{
    Error, Result,
    media::{Codec, OutputTarget, Source},
};
use std::io::{self, Write};

#[derive(Clone, Copy, ValueEnum)]
pub enum TrackKind {
    Audio,
    Camera,
    Screen,
}

#[derive(Args, Default)]
pub struct OutputOptions {
    /// Explicit local PulseAudio sink; audio only, never falls back to default
    #[arg(long, conflicts_with = "output_display")]
    output_device: Option<String>,
    /// Explicit local X11 playback display, e.g. :101; video only
    #[arg(long, conflicts_with = "output_device")]
    output_display: Option<String>,
}
impl OutputOptions {
    fn target(&self, codec: Codec) -> Result<OutputTarget> {
        let target = match (&self.output_device, &self.output_display) {
            (Some(device), None) => OutputTarget::AudioDevice {
                device: device.clone(),
            },
            (None, Some(display)) => OutputTarget::DisplayX11 {
                display: display.clone(),
            },
            (None, None) => OutputTarget::Default,
            _ => {
                return Err(Error::Configuration(
                    "choose exactly one native output target",
                ));
            }
        };
        target.validate(codec)?;
        Ok(target)
    }
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
        #[arg(long, conflicts_with_all = ["x11", "output_device", "output_display"])]
        inspect: bool,
        /// Render video on the explicit DISPLAY X11 server, not an automatic sink
        #[arg(long, conflicts_with_all = ["inspect", "output_device", "output_display"])]
        x11: bool,
        #[command(flatten)]
        output: OutputOptions,
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
    use clap::Parser;
    #[derive(Parser)]
    struct NativeArgs {
        #[command(subcommand)]
        command: super::Command,
    }
    #[derive(Parser)]
    struct RoomArgs {
        #[command(subcommand)]
        command: super::RoomCommand,
    }

    #[test]
    fn explicit_media_outputs_parse_without_changing_default_or_doctor_commands() {
        assert!(NativeArgs::try_parse_from(["kk", "capabilities"]).is_ok());
        assert!(NativeArgs::try_parse_from(["kk", "doctor"]).is_ok());
        let preview = NativeArgs::try_parse_from([
            "kk",
            "preview",
            "camera",
            "--device",
            "/dev/video7",
            "--output-display",
            ":101",
            "--consent",
        ])
        .unwrap();
        let super::Command::Preview { output, .. } = preview.command else {
            panic!("preview expected")
        };
        assert!(
            matches!(output.target(kk::media::Codec::Vp8).unwrap(), kk::media::OutputTarget::DisplayX11 { display } if display == ":101")
        );
        let args = RoomArgs::try_parse_from([
            "kk",
            "listen",
            "audio",
            "--from",
            "synthetic-peer",
            "--consent",
        ])
        .unwrap();
        let super::RoomCommand::Listen { output, .. } = args.command else {
            panic!("listen expected")
        };
        assert!(matches!(
            output.target(kk::media::Codec::Opus).unwrap(),
            kk::media::OutputTarget::Default
        ));
        for extra in [
            vec!["--inspect", "--output-device", "owned"],
            vec!["--x11", "--output-display", ":101"],
            vec!["--output-device", "owned", "--output-display", ":101"],
        ] {
            let mut args = vec![
                "kk",
                "listen",
                "audio",
                "--from",
                "synthetic-peer",
                "--consent",
            ];
            args.extend(extra);
            assert!(RoomArgs::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn explicit_output_kind_and_identifier_errors_are_inert() {
        let audio = super::OutputOptions {
            output_device: Some("owned-null".into()),
            output_display: None,
        };
        assert!(audio.target(kk::media::Codec::Opus).is_ok());
        assert!(audio.target(kk::media::Codec::Vp8).is_err());
        let video = super::OutputOptions {
            output_device: None,
            output_display: Some("remote.example:0".into()),
        };
        assert!(video.target(kk::media::Codec::Vp8).is_err());
        let injection = super::OutputOptions {
            output_device: Some("owned ! filesink".into()),
            output_display: None,
        };
        assert!(injection.target(kk::media::Codec::Opus).is_err());
    }
    #[test]
    fn native_output_flags_parse_without_capture_and_keep_defaults() {
        let args = NativeArgs::try_parse_from([
            "media",
            "preview",
            "microphone",
            "--device",
            "owned-input",
            "--consent",
        ])
        .unwrap();
        let super::Command::Preview { output, .. } = args.command else {
            panic!("preview expected")
        };
        assert!(matches!(
            output.target(kk::media::Codec::Opus).unwrap(),
            kk::media::OutputTarget::Default
        ));
        let args = NativeArgs::try_parse_from([
            "media",
            "preview",
            "screen-x11",
            "--device",
            ":101",
            "--output-display",
            ":102",
            "--consent",
        ])
        .unwrap();
        let super::Command::Preview { output, .. } = args.command else {
            panic!("preview expected")
        };
        assert!(
            matches!(output.target(kk::media::Codec::Vp8).unwrap(), kk::media::OutputTarget::DisplayX11 { display } if display == ":102")
        );
        assert!(output.target(kk::media::Codec::Opus).is_err());
        assert!(matches!(
            NativeArgs::try_parse_from(["media", "capabilities"])
                .unwrap()
                .command,
            super::Command::Capabilities
        ));
    }

    #[test]
    fn native_output_conflicts_and_untrusted_targets_fail_before_effects() {
        for flags in [
            vec!["--output-device", "owned", "--output-display", ":101"],
            vec!["--inspect", "--output-display", ":101"],
            vec!["--x11", "--output-device", "owned"],
        ] {
            let mut args = vec![
                "media",
                "listen",
                "screen",
                "--from",
                "synthetic",
                "--consent",
            ];
            args.extend(flags);
            assert!(RoomArgs::try_parse_from(args).is_err());
        }
        let invalid = super::OutputOptions {
            output_device: Some("sink ! filesink".into()),
            output_display: None,
        };
        assert!(invalid.target(kk::media::Codec::Opus).is_err());
        let remote = super::OutputOptions {
            output_device: None,
            output_display: Some("remote.example:0".into()),
        };
        assert!(remote.target(kk::media::Codec::Vp8).is_err());
        let args = RoomArgs::try_parse_from([
            "media",
            "listen",
            "audio",
            "--from",
            "synthetic",
            "--output-device",
            "owned-sink",
            "--consent",
        ])
        .unwrap();
        let super::RoomCommand::Listen { output, .. } = args.command else {
            panic!("listen expected")
        };
        assert!(
            matches!(output.target(kk::media::Codec::Opus).unwrap(), kk::media::OutputTarget::AudioDevice { device } if device == "owned-sink")
        );
    }
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
                output: Default::default(),
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

#[cfg(all(test, not(unix)))]
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
            let native_source = if rtp_file.is_none() {
                let device = device.ok_or(Error::Configuration(
                    "native media requires an explicit device/display",
                ))?;
                if window != 0 && kind != MediaKind::Screen {
                    return Err(Error::Configuration("--window requires a screen source"));
                }
                let source = match kind {
                    MediaKind::Audio => Source::Microphone { device },
                    MediaKind::Camera => Source::Camera { device },
                    MediaKind::Screen => Source::ScreenX11 {
                        display: device,
                        window,
                    },
                };
                source.validate()?;
                Some(source)
            } else {
                if device.is_some() || window != 0 {
                    return Err(Error::Configuration(
                        "RTP file replay cannot select capture devices or windows",
                    ));
                }
                None
            };
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
            let mut native = native_source.map(Source::capture).transpose()?;
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
            output,
            consent,
        } => {
            if !consent {
                return Err(Error::Denied);
            }
            let kind = kind.kind();
            if inspect && (x11 || output.output_device.is_some() || output.output_display.is_some())
            {
                return Err(Error::Configuration(
                    "inspection cannot select a playback output",
                ));
            }
            if x11 && (output.output_device.is_some() || output.output_display.is_some()) {
                return Err(Error::Configuration(
                    "--x11 cannot be combined with an explicit output target",
                ));
            }
            let output = if x11 {
                OutputTarget::DisplayX11 {
                    display: std::env::var("DISPLAY").map_err(|_| {
                        Error::Configuration("X11 playback requires DISPLAY or --output-display")
                    })?,
                }
            } else {
                output.target(kind.codec())?
            };
            output.validate(kind.codec())?;
            if !inspect && kk::media::native_capabilities().adapter.is_none() {
                return Err(Error::UnsupportedPlatform);
            }
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
            let mut natural_end = false;
            let interrupt = tokio::signal::ctrl_c();
            tokio::pin!(interrupt);
            let processing = async {
                loop {
                    let frame = tokio::select! {
                        biased;
                        signal = &mut interrupt => { signal?; break; },
                        frame = receiver.next() => frame?,
                    };
                    let Some(frame) = frame else {
                        natural_end = true;
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
                                .playback_on(output.clone())?,
                            );
                            track = Some(frame.track);
                        }
                        tokio::time::timeout(
                            kk::config::MEDIA_PACKET_MAX_AGE,
                            sink.as_mut()
                                .ok_or(Error::Protocol)?
                                .write_packet(&frame.packet),
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
            let cleanup = if let Some(sink) = sink {
                if natural_end && processing.is_ok() {
                    sink.finish_input_or_cancel(async {
                        tokio::select! {
                            biased;
                            signal = &mut interrupt => signal.map_err(Error::Io),
                            revoked = receiver.cancelled() => revoked,
                        }
                    })
                    .await
                    .and_then(playback_exit)
                } else {
                    sink.shutdown().await.and_then(playback_exit)
                }
            } else {
                Ok(())
            };
            drop(receiver);
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
    /// Report compiled native adapters, not installed devices or permission
    Capabilities,
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
        #[command(flatten)]
        output: OutputOptions,
        /// Authorize capture and local playback on this device
        #[arg(long, required = true)]
        consent: bool,
    },
}

pub(super) async fn run(command: Command) -> Result<()> {
    match command {
        Command::Capabilities => {
            writeln!(
                io::stdout(),
                "{}",
                serde_json::to_string(&kk::media::native_capabilities())
                    .map_err(|_| Error::Protocol)?
            )?;
            Ok(())
        }
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
            output,
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
            source.validate()?;
            if window != 0 && source.kind() != kk::media::Kind::Screen {
                return Err(Error::Configuration("--window requires screen-x11 preview"));
            }
            let target = output.target(source.codec())?;
            let mut process = source.preview_on(target)?;
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
