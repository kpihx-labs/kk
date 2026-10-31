//! Native media processes, standardized RTP and explicit local capture sources.
//! No source is opened by discovery, admission or process construction alone.
use crate::{Error, Result, config};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use zeroize::Zeroizing;
mod pipeline;
mod process;
mod vp8;
#[cfg(all(test, unix))]
pub(crate) use pipeline::{decode_synthetic_packets, synthetic_packets};

/// Compiled adapter support, NOT installed plugin/device availability or granted
/// permission. Discovery is inert; `doctor` separately probes plugin presence.
#[derive(Debug, Serialize)]
pub struct NativeCapabilities {
    pub platform: &'static str,
    pub adapter: Option<&'static str>,
    pub capture_sources: &'static [&'static str],
    pub playback_targets: &'static [&'static str],
}
pub fn native_capabilities() -> NativeCapabilities {
    let supported = cfg!(target_os = "linux");
    NativeCapabilities {
        platform: std::env::consts::OS,
        adapter: supported.then_some("gstreamer-bounded-subprocess"),
        capture_sources: if supported {
            &["pulseaudio-device", "v4l2-device", "x11-display-window"]
        } else {
            &[]
        },
        playback_targets: if supported {
            &["default", "pulseaudio-device", "x11-display"]
        } else {
            &[]
        },
    }
}
pub use process::{Exit, Pipeline};

pub(crate) fn now_ms() -> Result<u64> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::Configuration("media IPC requires a valid local clock"))?;
    u64::try_from(elapsed.as_millis()).map_err(|_| Error::Protocol)
}

/// Probes plugin presence only. It does not open a device or grant capture rights.
pub async fn doctor() -> Result<Vec<(String, bool)>> {
    let mut available = Vec::new();
    for plugin in [
        "opusenc",
        "opusdec",
        "rtpopuspay",
        "rtpopusdepay",
        "vp8enc",
        "vp8dec",
        "rtpvp8pay",
        "rtpvp8depay",
        "rtpstreampay",
        "rtpstreamdepay",
        "pulsesrc",
        "pulsesink",
        "v4l2src",
        "ximagesrc",
        "ximagesink",
        "pipewiresrc",
    ] {
        let mut process = Pipeline::inspect(plugin)?;
        let result = tokio::time::timeout(config::DEFAULT_TIMEOUT, process.finished()).await;
        let exit = match result {
            Ok(exit) => exit?,
            Err(_) => {
                process.shutdown().await?;
                return Err(Error::Timeout);
            }
        };
        available.push((plugin.into(), exit.success));
    }
    Ok(available)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Codec {
    Opus,
    Vp8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Kind {
    Audio,
    Camera,
    Screen,
}
impl Kind {
    pub fn codec(self) -> Codec {
        if self == Self::Audio {
            Codec::Opus
        } else {
            Codec::Vp8
        }
    }
}
impl Codec {
    pub fn clock_rate(self) -> u32 {
        match self {
            Self::Opus => 48_000,
            Self::Vp8 => 90_000,
        }
    }
    pub fn payload_type(self) -> u8 {
        match self {
            Self::Opus => 96,
            Self::Vp8 => 97,
        }
    }
}

pub enum Source {
    Microphone {
        device: String,
    },
    Camera {
        device: String,
    },
    /// Explicit X display/window only. This is NOT a Wayland portal shortcut.
    ScreenX11 {
        display: String,
        window: u64,
    },
}
impl Source {
    /// Build/validate an inert adapter plan; never opens a source or requests OS permission.
    pub fn validate(&self) -> Result<()> {
        pipeline::capture(self).map(|_| ())
    }
    pub fn kind(&self) -> Kind {
        match self {
            Self::Microphone { .. } => Kind::Audio,
            Self::Camera { .. } => Kind::Camera,
            Self::ScreenX11 { .. } => Kind::Screen,
        }
    }
    pub fn codec(&self) -> Codec {
        if matches!(self, Self::Microphone { .. }) {
            Codec::Opus
        } else {
            Codec::Vp8
        }
    }
    pub fn capture(self) -> Result<Pipeline> {
        Pipeline::start(pipeline::capture(&self)?, false, true)
    }
    /// Explicit native local preview, including real encode/decode, not a network proof.
    pub fn preview(self) -> Result<Pipeline> {
        Pipeline::start(pipeline::preview(&self)?, false, false)
    }
    /// Explicit local preview destination, with the same no-fallback validation
    /// as received playback. Does not publish or claim room authorization.
    pub fn preview_on(self, target: OutputTarget) -> Result<Pipeline> {
        Pipeline::start(pipeline::preview_on(&self, &target)?, false, false)
    }
}

pub struct Sink {
    pub codec: Codec,
}
/// Local output coordinates only; never transmitted as room metadata. Explicit
/// targets do not fall back to the OS default when a device/display is missing.
#[derive(Clone, Debug)]
pub enum OutputTarget {
    Default,
    AudioDevice { device: String },
    DisplayX11 { display: String },
}
impl OutputTarget {
    /// Validate local selection before IPC, capture or playback effects.
    pub fn validate(&self, codec: Codec) -> Result<()> {
        pipeline::playback_on(codec, self).map(|_| ())
    }
}
impl Sink {
    pub fn playback_on(self, target: OutputTarget) -> Result<Pipeline> {
        if !cfg!(target_os = "linux") {
            return Err(Error::Configuration(
                "bounded native playback on this platform is not implemented yet",
            ));
        }
        Pipeline::start(pipeline::playback_on(self.codec, &target)?, true, false)
            .map(|process| process.decoder(self.codec))
    }
    pub fn playback(self) -> Result<Pipeline> {
        self.playback_with(false)
    }

    /// Select X11 video explicitly instead of automatic platform sink discovery.
    pub fn playback_with(self, x11: bool) -> Result<Pipeline> {
        if x11 && self.codec != Codec::Vp8 {
            return Err(Error::Configuration("X11 playback requires video"));
        }
        if !cfg!(target_os = "linux") {
            return Err(Error::Configuration(
                "bounded native playback on this platform is not implemented yet",
            ));
        }
        Pipeline::start(pipeline::playback(self.codec, x11), true, false)
            .map(|process| process.decoder(self.codec))
    }
}

/// A bounded RTP packet. Debug/serialization do not expose captured content.
pub struct Packet {
    codec: Codec,
    bytes: Zeroizing<Vec<u8>>,
}
impl Packet {
    fn payload(&self) -> &[u8] {
        let mut header = 12 + (self.bytes[0] & 15) as usize * 4;
        if self.bytes[0] & 16 != 0 {
            header += 4 + u16::from_be_bytes([self.bytes[header + 2], self.bytes[header + 3]])
                as usize
                * 4;
        }
        let padding = if self.bytes[0] & 32 != 0 {
            *self.bytes.last().unwrap() as usize
        } else {
            0
        };
        &self.bytes[header..self.bytes.len() - padding]
    }
    pub fn timestamp(&self) -> u32 {
        u32::from_be_bytes(self.bytes[4..8].try_into().expect("validated RTP header"))
    }
    pub fn ssrc(&self) -> u32 {
        u32::from_be_bytes(self.bytes[8..12].try_into().expect("validated RTP header"))
    }
    pub fn parse(codec: Codec, bytes: Vec<u8>) -> Result<Self> {
        let bytes = Zeroizing::new(bytes);
        if bytes.len() < 12
            || bytes.len() > config::MEDIA_PACKET_BYTES
            || bytes[0] >> 6 != 2
            || bytes[1] & 0x7f != codec.payload_type()
        {
            return Err(Error::Protocol);
        }
        let mut header = 12 + (bytes[0] & 15) as usize * 4;
        if header > bytes.len() {
            return Err(Error::Protocol);
        }
        if bytes[0] & 16 != 0 {
            if bytes.len() < header + 4 {
                return Err(Error::Protocol);
            }
            header += 4 + u16::from_be_bytes([bytes[header + 2], bytes[header + 3]]) as usize * 4;
        }
        let padding = if bytes[0] & 32 != 0 {
            *bytes.last().ok_or(Error::Protocol)? as usize
        } else {
            0
        };
        if header >= bytes.len()
            || padding > bytes.len() - header
            || (bytes[0] & 32 != 0 && padding == 0)
        {
            return Err(Error::Protocol);
        }
        Ok(Self { codec, bytes })
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn codec(&self) -> Codec {
        self.codec
    }
    /// RFC4571 length prefix, bounded before allocating. Null packets are skipped.
    pub async fn read(
        codec: Codec,
        input: &mut (impl tokio::io::AsyncRead + Unpin),
    ) -> Result<Option<Self>> {
        for _ in 0..16 {
            let mut length = [0; 2];
            if input.read(&mut length[..1]).await? == 0 {
                return Ok(None);
            }
            input.read_exact(&mut length[1..]).await?;
            let length = u16::from_be_bytes(length) as usize;
            if length == 0 {
                continue;
            }
            if length > config::MEDIA_PACKET_BYTES {
                return Err(Error::Protocol);
            }
            let mut bytes = Zeroizing::new(vec![0; length]);
            input.read_exact(&mut bytes).await?;
            return Self::parse(codec, std::mem::take(&mut *bytes)).map(Some);
        }
        Err(Error::Protocol)
    }
    pub async fn write(&self, output: &mut (impl tokio::io::AsyncWrite + Unpin)) -> Result<()> {
        output
            .write_all(&(self.bytes.len() as u16).to_be_bytes())
            .await?;
        output.write_all(&self.bytes).await?;
        Ok(())
    }
}

/// Explicit RFC4571 replay/interoperability input, never an implicit device.
pub fn rtp_file(path: &std::path::Path) -> Result<tokio::fs::File> {
    crate::files::source_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor = "current_thread")]
    async fn rtp_and_rfc4571_are_bounded_before_allocation_and_preserve_packet_boundaries() {
        let mut bytes = vec![0x80, 96, 0, 1, 0, 0, 0, 1, 1, 2, 3, 4, 5];
        let packet = Packet::parse(Codec::Opus, bytes.clone()).unwrap();
        let mut stream = Vec::new();
        packet.write(&mut stream).await.unwrap();
        packet.write(&mut stream).await.unwrap();
        let mut input = stream.as_slice();
        assert_eq!(
            Packet::read(Codec::Opus, &mut input)
                .await
                .unwrap()
                .unwrap()
                .bytes(),
            &bytes
        );
        assert!(
            Packet::read(Codec::Opus, &mut input)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            Packet::read(Codec::Opus, &mut input)
                .await
                .unwrap()
                .is_none()
        );
        assert!(Packet::parse(Codec::Vp8, bytes.clone()).is_err());
        bytes[0] |= 16;
        assert!(Packet::parse(Codec::Opus, bytes).is_err());
        assert!(
            Packet::read(Codec::Opus, &mut &[0xff, 0xff][..])
                .await
                .is_err()
        );
        assert!(Packet::read(Codec::Opus, &mut &[0; 32][..]).await.is_err());
    }
}
