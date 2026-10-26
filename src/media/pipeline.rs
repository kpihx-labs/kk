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
            if !display.starts_with(':')
                || display.len() < 2
                || display.ends_with('.')
                || display.bytes().filter(|c| *c == b'.').count() > 1
                || !display[1..]
                    .bytes()
                    .all(|c| c.is_ascii_digit() || c == b'.')
            {
                return Err(Error::Configuration(
                    "screen capture requires an explicit local X display, not a remote server",
                ));
            }
            vec![
                "ximagesrc".into(),
                format!("display-name={}", property(display)?),
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

#[cfg(test)]
mod tests {
    use super::*;
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
        let mut capture = Source::ScreenX11 { display, window: 0 }.capture().unwrap();
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
        server.shutdown().await.unwrap();
        verified.unwrap();
        for (actual, expected) in frame[..3].iter().zip([0x33u8, 0x66, 0x99]) {
            assert!(
                actual.abs_diff(expected) <= 8,
                "actual virtual-display RGB {:?}",
                &frame[..3]
            );
        }
    }
}
