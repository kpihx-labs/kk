//! Fixed native pipeline vocabulary. User input is a validated property, never
//! a GStreamer program, shell fragment, remote URI or implicit capture fallback.
use super::*;

fn property(value: &str) -> Result<&str> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-:/".contains(&c))
    {
        return Err(Error::Configuration(
            "media device/display must be an explicit bounded identifier, not pipeline syntax",
        ));
    }
    Ok(value)
}
fn local_display(display: &str) -> Result<&str> {
    if !display.starts_with(':')
        || display.len() < 2
        || !display.as_bytes().get(1).is_some_and(u8::is_ascii_digit)
        || display.ends_with('.')
        || display.bytes().filter(|c| *c == b'.').count() > 1
        || !display[1..]
            .bytes()
            .all(|c| c.is_ascii_digit() || c == b'.')
    {
        return Err(Error::Configuration(
            "display requires an explicit local X display, not a remote server",
        ));
    }
    property(display)
}
fn native_source(source: &Source) -> Result<Vec<String>> {
    if !cfg!(target_os = "linux") {
        return Err(Error::Configuration(
            "native capture on this platform is not implemented yet",
        ));
    }
    Ok(match source {
        Source::Microphone { device } => vec![
            "pulsesrc".into(),
            format!("device={}", property(device)?),
            "do-timestamp=true".into(),
        ],
        Source::Camera { device } => {
            if !device.starts_with("/dev/video")
                || !device[10..].bytes().all(|c| c.is_ascii_digit())
                || device.len() <= 10
            {
                return Err(Error::Configuration(
                    "camera requires an explicit /dev/videoN device",
                ));
            }
            vec![
                "v4l2src".into(),
                format!("device={}", property(device)?),
                "do-timestamp=true".into(),
            ]
        }
        Source::ScreenX11 { display, window } => {
            vec![
                "ximagesrc".into(),
                format!("display-name={}", local_display(display)?),
                format!("xid={window}"),
                "use-damage=false".into(),
                "show-pointer=true".into(),
            ]
        }
    })
}
fn encoder(codec: Codec) -> Vec<String> {
    let args = match codec {
        Codec::Opus => vec![
            "audioconvert".into(),
            "!".into(),
            "audioresample".into(),
            "!".into(),
            "audio/x-raw,rate=48000,channels=1".into(),
            "!".into(),
            "opusenc".into(),
            format!("bitrate={}", config::MEDIA_AUDIO_BITRATE),
            "audio-type=voice".into(),
            "inband-fec=true".into(),
            "!".into(),
            "rtpopuspay".into(),
            "pt=96".into(),
        ],
        Codec::Vp8 => vec![
            "videoconvert".into(),
            "!".into(),
            "videoscale".into(),
            "!".into(),
            "videorate".into(),
            "!".into(),
            format!(
                "video/x-raw,format=I420,width={},height={},framerate={}/1",
                config::MEDIA_VIDEO_WIDTH,
                config::MEDIA_VIDEO_HEIGHT,
                config::MEDIA_VIDEO_FPS
            ),
            "!".into(),
            "vp8enc".into(),
            "deadline=1".into(),
            "cpu-used=8".into(),
            "threads=2".into(),
            "lag-in-frames=0".into(),
            format!("target-bitrate={}", config::MEDIA_VIDEO_BITRATE),
            format!("keyframe-max-dist={}", config::MEDIA_VIDEO_FPS as u16 * 2),
            "!".into(),
            "rtpvp8pay".into(),
            "pt=97".into(),
        ],
    };
    args.into_iter()
        .chain([format!("mtu={}", config::MEDIA_PACKET_BYTES)])
        .collect()
}
fn decoder(codec: Codec, x11: bool) -> Vec<String> {
    match codec {
        Codec::Opus => [
            "rtpopusdepay",
            "!",
            "opusdec",
            "!",
            "audioconvert",
            "!",
            "audioresample",
            "!",
            "autoaudiosink",
        ]
        .map(String::from)
        .to_vec(),
        Codec::Vp8 => [
            "rtpvp8depay",
            "!",
            "vp8dec",
            "!",
            "videoconvert",
            "!",
            if x11 { "ximagesink" } else { "autovideosink" },
        ]
        .map(String::from)
        .to_vec(),
    }
}
fn encoded(source: &Source) -> Result<Vec<String>> {
    let mut args = native_source(source)?;
    args.extend(
        [
            "!",
            "queue",
            "max-size-buffers=2",
            "max-size-bytes=0",
            "max-size-time=0",
            "leaky=downstream",
            "!",
        ]
        .map(String::from),
    );
    args.extend(encoder(source.codec()));
    Ok(args)
}
pub(super) fn capture(source: &Source) -> Result<Vec<String>> {
    let mut args = encoded(source)?;
    args.extend(["!", "rtpstreampay", "!", "fdsink", "fd=1", "sync=false"].map(String::from));
    Ok(args)
}
pub(super) fn preview(source: &Source) -> Result<Vec<String>> {
    let mut args = encoded(source)?;
    args.push("!".into());
    args.extend(decoder(source.codec(), false));
    Ok(args)
}
pub(super) fn preview_on(source: &Source, target: &OutputTarget) -> Result<Vec<String>> {
    let mut args = encoded(source)?;
    args.push("!".into());
    args.extend(output_decoder(source.codec(), target)?);
    Ok(args)
}
pub(super) fn playback(codec: Codec, x11: bool) -> Vec<String> {
    let caps = match codec {
        Codec::Opus => {
            "application/x-rtp-stream,media=audio,clock-rate=48000,encoding-name=OPUS,payload=96"
        }
        Codec::Vp8 => {
            "application/x-rtp-stream,media=video,clock-rate=90000,encoding-name=VP8,payload=97"
        }
    };
    let mut args = [
        "fdsrc",
        "fd=0",
        "!",
        caps,
        "!",
        "rtpstreamdepay",
        "!",
        "rtpjitterbuffer",
        "latency=100",
        "drop-on-latency=true",
        "!",
    ]
    .map(String::from)
    .to_vec();
    args.extend(decoder(codec, x11));
    args
}

pub(super) fn playback_on(codec: Codec, target: &OutputTarget) -> Result<Vec<String>> {
    let mut args = playback(codec, false);
    let decoder_len = decoder(codec, false).len();
    args.truncate(args.len() - decoder_len);
    args.extend(output_decoder(codec, target)?);
    Ok(args)
}
fn output_decoder(codec: Codec, target: &OutputTarget) -> Result<Vec<String>> {
    let mut args = decoder(codec, false);
    match (codec, target) {
        (_, OutputTarget::Default) => {}
        (Codec::Opus, OutputTarget::AudioDevice { device }) => {
            args.pop();
            args.extend(["pulsesink".into(), format!("device={}", property(device)?)]);
        }
        (Codec::Vp8, OutputTarget::DisplayX11 { display }) => {
            args.pop();
            args.extend([
                "ximagesink".into(),
                format!("display={}", local_display(display)?),
            ]);
        }
        _ => {
            return Err(Error::Configuration(
                "native output target does not match the media codec",
            ));
        }
    }
    Ok(args)
}

/// Test-only real codec fixtures: fixed generated sources, never device capture.
#[cfg(test)]
pub(crate) async fn synthetic_packets(codec: Codec) -> Vec<Packet> {
    let source = if codec == Codec::Opus {
        "audiotestsrc"
    } else {
        "videotestsrc"
    };
    let mut args = vec![source.into(), "num-buffers=2".into()];
    if codec == Codec::Vp8 {
        args.push("pattern=black".into());
    }
    args.push("!".into());
    args.extend(encoder(codec));
    args.extend(["!", "rtpstreampay", "!", "fdsink", "fd=1", "sync=false"].map(String::from));
    let mut process = Pipeline::start(args, false, true).unwrap();
    let packets = tokio::time::timeout(config::DEFAULT_TIMEOUT, async {
        let mut packets = Vec::new();
        while let Some(packet) = Packet::read(codec, process.output.as_mut().unwrap())
            .await
            .unwrap()
        {
            assert!(
                packets.len() < 64,
                "synthetic fixture exceeded its packet budget"
            );
            packets.push(packet);
        }
        packets
    })
    .await
    .unwrap();
    let exit = process.finished().await.unwrap();
    assert!(
        exit.success,
        "{}",
        String::from_utf8_lossy(&exit.diagnostic)
    );
    assert!(!packets.is_empty());
    packets
}

#[cfg(test)]
pub(crate) async fn decode_synthetic_packets(codec: Codec, packets: &[Packet]) {
    let mut args = playback(codec, false);
    args.pop();
    args.extend(["fdsink".into(), "fd=1".into(), "sync=false".into()]);
    let mut process = Pipeline::start(args, true, true).unwrap();
    let mut input = process.input.take().unwrap();
    let mut output = process.output.take().unwrap();
    tokio::time::timeout(config::DEFAULT_TIMEOUT, async {
        let write = async {
            for packet in packets {
                packet.write(&mut input).await.unwrap();
            }
            drop(input);
        };
        let read = async {
            let limit =
                u64::from(config::MEDIA_VIDEO_WIDTH) * u64::from(config::MEDIA_VIDEO_HEIGHT) * 16;
            let mut decoded = Zeroizing::new(Vec::new());
            (&mut output)
                .take(limit + 1)
                .read_to_end(&mut decoded)
                .await
                .unwrap();
            assert!(
                !decoded.is_empty(),
                "bridge output must actually decode samples/pixels"
            );
            assert!(decoded.len() as u64 <= limit);
        };
        tokio::join!(write, read);
        let exit = process.finished().await.unwrap();
        assert!(
            exit.success,
            "{}",
            String::from_utf8_lossy(&exit.diagnostic)
        );
    })
    .await
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "current_thread")]
    #[ignore = "explicit private missing Pulse-server refusal; synthetic audio, no physical output"]
    async fn native_explicit_audio_output_cannot_fall_back_from_a_private_missing_server() {
        const PROBE: &str = "KK_MEDIA_PRIVATE_PULSE_PROBE";
        if std::env::var_os(PROBE).is_some() {
            let server = std::env::var("PULSE_SERVER").unwrap();
            let path = server.strip_prefix("unix:").unwrap();
            assert!(path.ends_with("/absent-pulse-server"));
            assert!(!std::path::Path::new(path).exists());
            let mut plugin = Pipeline::inspect("pulsesink").unwrap();
            assert!(
                plugin.finished().await.unwrap().success,
                "the refusal gate requires the actual Pulse sink plugin"
            );
            let packets = synthetic_packets(Codec::Opus).await;
            for _ in 0..2 {
                let mut output = Sink { codec: Codec::Opus }
                    .playback_on(OutputTarget::AudioDevice {
                        device: "kk-owned-null".into(),
                    })
                    .unwrap();
                for packet in &packets {
                    if output.write_packet(packet).await.is_err() {
                        break;
                    }
                }
                let exit = output.finish_input().await.unwrap();
                assert!(
                    !exit.success,
                    "missing explicit Pulse server must never play through the host default"
                );
                assert!(!exit.stopped);
                assert!(!exit.diagnostic.is_empty());
            }
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "media::pipeline::tests::native_explicit_audio_output_cannot_fall_back_from_a_private_missing_server", "--ignored", "--nocapture"])
            .env(PROBE, "1").env("PULSE_SERVER", format!("unix:{}", root.path().join("absent-pulse-server").display())).kill_on_drop(true);
        let output = tokio::time::timeout(
            config::DEFAULT_TIMEOUT + config::QUIC_SHUTDOWN_GRACE,
            command.output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    #[test]
    fn native_capability_discovery_is_adapter_support_not_permission_or_portal_capture() {
        let capabilities = native_capabilities();
        assert_eq!(capabilities.platform, std::env::consts::OS);
        assert_eq!(capabilities.adapter.is_some(), cfg!(target_os = "linux"));
        assert_eq!(
            capabilities.capture_sources.is_empty(),
            !cfg!(target_os = "linux")
        );
        assert_eq!(
            capabilities.playback_targets.is_empty(),
            !cfg!(target_os = "linux")
        );
        assert!(
            !capabilities
                .capture_sources
                .iter()
                .any(|source| source.contains("portal"))
        );
        let source = Source::ScreenX11 {
            display: ":101".into(),
            window: 0,
        };
        let preview = preview_on(
            &source,
            &OutputTarget::DisplayX11 {
                display: ":102".into(),
            },
        );
        if cfg!(target_os = "linux") {
            let args = preview.unwrap();
            assert!(args.iter().any(|arg| arg == "display-name=:101"));
            assert!(args.iter().any(|arg| arg == "display=:102"));
            assert!(!args.iter().any(|arg| arg == "autovideosink"));
        } else {
            assert!(preview.is_err());
        }
    }
    #[test]
    fn explicit_native_outputs_validate_before_launch_and_never_fall_back() {
        let audio = playback_on(
            Codec::Opus,
            &OutputTarget::AudioDevice {
                device: "owned-null.monitor".into(),
            },
        )
        .unwrap();
        assert_eq!(
            &audio[audio.len() - 2..],
            ["pulsesink", "device=owned-null.monitor"]
        );
        assert!(!audio.iter().any(|arg| arg == "autoaudiosink"));
        let video = playback_on(
            Codec::Vp8,
            &OutputTarget::DisplayX11 {
                display: ":123.0".into(),
            },
        )
        .unwrap();
        assert_eq!(&video[video.len() - 2..], ["ximagesink", "display=:123.0"]);
        for display in ["", "remote:0", ":0 ! filesink", ":1.", ":.1"] {
            // An omitted display number is invalid even though the remaining
            // characters look like a screen suffix.
            assert!(
                playback_on(
                    Codec::Vp8,
                    &OutputTarget::DisplayX11 {
                        display: display.into()
                    }
                )
                .is_err()
            );
        }
        assert!(
            playback_on(
                Codec::Opus,
                &OutputTarget::DisplayX11 {
                    display: ":1".into()
                }
            )
            .is_err()
        );
        assert!(
            playback_on(
                Codec::Vp8,
                &OutputTarget::AudioDevice {
                    device: "owned".into()
                }
            )
            .is_err()
        );
        assert!(
            playback_on(
                Codec::Opus,
                &OutputTarget::AudioDevice {
                    device: "default ! fakesink".into()
                }
            )
            .is_err()
        );
    }
    #[test]
    fn remote_uris_and_pipeline_injection_never_choose_a_capture_source() {
        for value in [
            "",
            "default ! filesink location=/tmp/leak",
            "$(command)",
            "a\nb",
        ] {
            assert!(
                capture(&Source::Microphone {
                    device: value.into()
                })
                .is_err()
            );
        }
        assert!(
            capture(&Source::Camera {
                device: "https://camera.invalid".into()
            })
            .is_err()
        );
        assert!(
            capture(&Source::ScreenX11 {
                display: "remote.example:0".into(),
                window: 0
            })
            .is_err()
        );
        let camera = capture(&Source::Camera {
            device: "/dev/video0".into(),
        });
        if cfg!(target_os = "linux") {
            assert!(camera.unwrap().contains(&"rtpstreampay".into()));
        } else {
            assert!(camera.is_err());
        }
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "explicit native GStreamer codec gate; synthetic sources, not physical capture"]
    async fn native_opus_and_vp8_roundtrip_real_bounded_rtp_packets_and_decode() {
        for codec in [Codec::Opus, Codec::Vp8] {
            let producer = if codec == Codec::Opus {
                "audiotestsrc"
            } else {
                "videotestsrc"
            };
            let mut args = vec![producer.into(), "num-buffers=15".into(), "!".into()];
            args.extend(encoder(codec));
            args.extend(
                ["!", "rtpstreampay", "!", "fdsink", "fd=1", "sync=false"].map(String::from),
            );
            let mut capture = Pipeline::start(args, false, true).unwrap();
            let caps = if codec == Codec::Opus {
                "application/x-rtp-stream,media=audio,clock-rate=48000,encoding-name=OPUS,payload=96"
            } else {
                "application/x-rtp-stream,media=video,clock-rate=90000,encoding-name=VP8,payload=97"
            };
            let mut args = ["fdsrc", "fd=0", "!", caps, "!", "rtpstreamdepay", "!"]
                .map(String::from)
                .to_vec();
            args.extend(
                if codec == Codec::Opus {
                    ["rtpopusdepay", "!", "opusdec"]
                } else {
                    ["rtpvp8depay", "!", "vp8dec"]
                }
                .map(String::from),
            );
            args.extend(["!", "fakesink", "sync=false"].map(String::from));
            let mut decode = Pipeline::start(args, true, false).unwrap();
            let transferred = tokio::time::timeout(config::DEFAULT_TIMEOUT, async {
                let mut count = 0;
                while let Some(packet) = Packet::read(codec, capture.output.as_mut().unwrap())
                    .await
                    .unwrap()
                {
                    assert!(packet.bytes().len() <= config::MEDIA_PACKET_BYTES);
                    packet.write(decode.input.as_mut().unwrap()).await.unwrap();
                    count += 1;
                }
                count
            })
            .await
            .unwrap();
            assert!(transferred > 0);
            decode.input.take();
            assert!(capture.finished().await.unwrap().success);
            let exit = tokio::time::timeout(config::DEFAULT_TIMEOUT, decode.finished())
                .await
                .unwrap()
                .unwrap();
            assert!(
                exit.success,
                "{}",
                String::from_utf8_lossy(&exit.diagnostic)
            );
        }
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "explicit native subprocess ownership gate; no physical capture"]
    async fn native_pipeline_cancellation_reaps_its_child_and_reports_bounded_diagnostics() {
        let pipeline = Pipeline::start(
            ["audiotestsrc", "is-live=true", "!", "fakesink"]
                .map(String::from)
                .to_vec(),
            false,
            false,
        )
        .unwrap();
        let exit = pipeline.shutdown().await.unwrap();
        assert!(!exit.success);
        assert!(exit.diagnostic.len() <= config::MEDIA_DIAGNOSTIC_BYTES);
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "simultaneous generated native sources and backpressure shutdown; no physical capture"]
    async fn native_stopping_backpressured_video_does_not_stop_simultaneous_audio() {
        let start = |codec, source: &str| {
            let mut args = vec![source.into(), "is-live=true".into(), "!".into()];
            args.extend(encoder(codec));
            args.extend(
                ["!", "rtpstreampay", "!", "fdsink", "fd=1", "sync=false"].map(String::from),
            );
            Pipeline::start(args, false, true).unwrap()
        };
        let mut audio = start(Codec::Opus, "audiotestsrc");
        let mut video = start(Codec::Vp8, "videotestsrc");
        let first = tokio::time::timeout(
            config::DEFAULT_TIMEOUT,
            Packet::read(Codec::Opus, audio.output.as_mut().unwrap()),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        // Consume only one video packet; deliberately leave the rest unread so
        // stdout backpressure cannot prevent cancellation/reaping.
        tokio::time::timeout(
            config::DEFAULT_TIMEOUT,
            Packet::read(Codec::Vp8, video.output.as_mut().unwrap()),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        let (video_exit, second) = tokio::time::timeout(config::DEFAULT_TIMEOUT, async {
            tokio::join!(
                video.shutdown(),
                Packet::read(Codec::Opus, audio.output.as_mut().unwrap())
            )
        })
        .await
        .unwrap();
        assert!(!video_exit.unwrap().success);
        let second = second.unwrap().unwrap();
        assert_eq!(first.ssrc(), second.ssrc());
        assert_ne!(first.timestamp(), second.timestamp());
        assert!(!audio.shutdown().await.unwrap().success);
        // A receiver that reaches a natural source boundary closes stdin and
        // flushes its decoder instead of misreporting forced kill as completion.
        let packets = synthetic_packets(Codec::Opus).await;
        let mut args = playback(Codec::Opus, false);
        args.pop();
        args.extend(["fakesink".into(), "sync=false".into()]);
        let mut sink = Pipeline::start(args, true, false).unwrap();
        for packet in packets {
            packet.write(sink.input.as_mut().unwrap()).await.unwrap();
        }
        let exit = sink.finish_input().await.unwrap();
        assert!(
            exit.success,
            "{}",
            String::from_utf8_lossy(&exit.diagnostic)
        );
        assert!(!exit.stopped);
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "explicit private Xwayland virtual-display capture; never reads the host desktop"]
    async fn native_screen_capture_reads_only_an_owned_virtual_display_and_decodes_its_pixels() {
        use tokio::io::AsyncBufReadExt;
        assert!(
            std::env::var_os("WAYLAND_DISPLAY").is_some(),
            "a Wayland session and Xwayland are required for the private virtual display"
        );
        let mut server = Pipeline::spawn(
            "Xwayland",
            [
                "-geometry",
                "640x360",
                "-shm",
                "-displayfd",
                "1",
                "-nolisten",
                "tcp",
                "-noreset",
                "-maxclients",
                "64",
            ]
            .map(String::from)
            .to_vec(),
            false,
            true,
        )
        .unwrap();
        let mut ready = tokio::io::BufReader::new(server.output.as_mut().unwrap().take(32));
        let mut number = String::new();
        tokio::time::timeout(config::DEFAULT_TIMEOUT, ready.read_line(&mut number))
            .await
            .unwrap()
            .unwrap();
        drop(ready);
        if number.is_empty() {
            let exit = server.finished().await.unwrap();
            panic!(
                "private virtual display failed: {}",
                String::from_utf8_lossy(&exit.diagnostic)
            );
        }
        let number: u16 = number
            .trim()
            .parse()
            .expect("private X server must publish its assigned display");
        let display = format!(":{number}");
        let mut draw = Pipeline::spawn(
            "xsetroot",
            vec![
                "-display".into(),
                display.clone(),
                "-solid".into(),
                "#336699".into(),
            ],
            false,
            false,
        )
        .unwrap();
        assert!(draw.finished().await.unwrap().success);
        let mut capture = Source::ScreenX11 {
            display: display.clone(),
            window: 0,
        }
        .capture()
        .unwrap();
        let mut decode = Pipeline::start(["fdsrc", "fd=0", "!", "application/x-rtp-stream,media=video,clock-rate=90000,encoding-name=VP8,payload=97", "!", "rtpstreamdepay", "!", "rtpvp8depay", "!", "vp8dec", "!", "videoconvert", "!", "video/x-raw,format=RGB", "!", "fdsink", "fd=1", "sync=false"].map(String::from).to_vec(), true, true).unwrap();
        let mut frame = Zeroizing::new(vec![
            0;
            config::MEDIA_VIDEO_WIDTH as usize
                * config::MEDIA_VIDEO_HEIGHT as usize
                * 3
        ]);
        let verified = tokio::time::timeout(config::DEFAULT_TIMEOUT, async {
            let pixels = decode.output.as_mut().unwrap().read_exact(&mut frame);
            tokio::pin!(pixels);
            loop {
                tokio::select! {
                    result = &mut pixels => { result.unwrap(); break; },
                    packet = Packet::read(Codec::Vp8, capture.output.as_mut().unwrap()) => {
                        packet.unwrap().expect("native virtual-display capture ended early").write(decode.input.as_mut().unwrap()).await.unwrap();
                    },
                }
            }
        }).await;
        capture.shutdown().await.unwrap();
        decode.shutdown().await.unwrap();
        verified.unwrap();
        for (actual, expected) in frame[..3].iter().zip([0x33u8, 0x66, 0x99]) {
            assert!(
                actual.abs_diff(expected) <= 8,
                "actual virtual-display RGB {:?}",
                &frame[..3]
            );
        }
        // Exercise the public explicit-output adapter and a fresh restart on
        // THIS owned server, never the workstation's ambient DISPLAY.
        let packets = synthetic_packets(Codec::Vp8).await;
        for _ in 0..2 {
            let mut output = Sink { codec: Codec::Vp8 }
                .playback_on(OutputTarget::DisplayX11 {
                    display: display.clone(),
                })
                .unwrap();
            for packet in &packets {
                output.write_packet(packet).await.unwrap();
            }
            let exit = output.finish_input().await.unwrap();
            assert!(
                exit.success,
                "{}",
                String::from_utf8_lossy(&exit.diagnostic)
            );
            assert!(!exit.stopped);
        }
        server.shutdown().await.unwrap();
    }
}
