//! Explicit private invitation export/import and bounded QR image roundtrip.

use super::{PairServer, Scope};
use crate::{Error, Identity, Result, profile};
use age::secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::{
    io::{Cursor, Write},
    net::SocketAddr,
    path::Path,
};
use zeroize::Zeroize;

const MAX_BUNDLE: usize = 4096;
const MAX_IMAGE: u64 = 4 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    version: u8,
    address: SocketAddr,
    id: [u8; 16],
    fingerprint: String,
    code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    room: Option<[u8; 32]>,
}
impl Drop for Wire {
    fn drop(&mut self) {
        self.code.zeroize();
    }
}

pub struct Invite {
    pub scope: Scope,
    pub address: SocketAddr,
    pub id: [u8; 16],
    pub fingerprint: String,
    code: SecretString,
}
impl Invite {
    pub fn from_server(server: &PairServer, identity: &Identity) -> Result<Self> {
        let address = server.local_addr()?;
        if address.ip().is_unspecified() || address.ip().is_multicast() {
            return Err(Error::Configuration(
                "pairing invitation needs a concrete bind address",
            ));
        }
        Ok(Self {
            scope: server.scope(),
            address,
            id: server.id(),
            fingerprint: identity.peer().fingerprint(),
            code: server.code()?.clone(),
        })
    }
    pub fn code(&self) -> &SecretString {
        &self.code
    }
    fn bytes(&self) -> Result<zeroize::Zeroizing<Vec<u8>>> {
        let wire = Wire {
            version: if self.scope == Scope::Contact { 1 } else { 2 },
            address: self.address,
            id: self.id,
            fingerprint: self.fingerprint.clone(),
            code: self.code.expose_secret().to_owned(),
            room: match self.scope {
                Scope::Contact => None,
                Scope::Room(room) => Some(room),
            },
        };
        let bytes = serde_json::to_vec(&wire).map_err(|_| Error::Pairing)?;
        if bytes.len() > MAX_BUNDLE {
            return Err(Error::Pairing);
        }
        Ok(zeroize::Zeroizing::new(bytes))
    }
    fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_BUNDLE {
            return Err(Error::Pairing);
        }
        let mut wire: Wire = serde_json::from_slice(bytes).map_err(|_| Error::Pairing)?;
        if !matches!((wire.version, wire.room), (1, None) | (2, Some(_)))
            || wire.address.port() == 0
            || wire.address.ip().is_unspecified()
            || wire.address.ip().is_multicast()
            || wire.fingerprint.len() != 64
            || !wire
                .fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || wire.code.is_empty()
            || wire.code.len() > 512
        {
            return Err(Error::Pairing);
        }
        Ok(Self {
            scope: match wire.room {
                Some(room) => Scope::Room(room),
                None => Scope::Contact,
            },
            address: wire.address,
            id: wire.id,
            fingerprint: wire.fingerprint.clone(),
            code: SecretString::from(std::mem::take(&mut wire.code)),
        })
    }
    pub fn export(&self, destination: &Path) -> Result<()> {
        write_private(destination, &self.bytes()?)
    }
    pub async fn load(source: &Path) -> Result<Self> {
        Self::parse(&read_private(source, MAX_BUNDLE as u64).await?)
    }
    pub fn qr(&self, destination: &Path) -> Result<()> {
        let bytes = self.bytes()?;
        let code = qrcode::QrCode::new(bytes.as_slice()).map_err(|_| Error::Pairing)?;
        let scale = 8usize;
        let margin = 4usize;
        let side = (code.width() + margin * 2) * scale;
        let mut image = image::GrayImage::from_pixel(side as u32, side as u32, image::Luma([255]));
        for y in 0..code.width() {
            for x in 0..code.width() {
                if code[(x, y)] == qrcode::Color::Dark {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            image.put_pixel(
                                ((x + margin) * scale + dx) as u32,
                                ((y + margin) * scale + dy) as u32,
                                image::Luma([0]),
                            );
                        }
                    }
                }
            }
        }
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageLuma8(image)
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|_| Error::Pairing)?;
        write_private(destination, &png.into_inner())
    }
    pub async fn load_qr(source: &Path) -> Result<Self> {
        let bytes = read_private(source, MAX_IMAGE).await?;
        let mut reader = image::ImageReader::new(Cursor::new(bytes.as_slice()))
            .with_guessed_format()
            .map_err(|_| Error::Pairing)?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(2048);
        limits.max_image_height = Some(2048);
        limits.max_alloc = Some(16 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().map_err(|_| Error::Pairing)?.to_luma8();
        let mut decoder = quircs::Quirc::default();
        let mut found = None;
        for code in decoder.identify(
            image.width() as usize,
            image.height() as usize,
            image.as_raw(),
        ) {
            let decoded = code
                .map_err(|_| Error::Pairing)?
                .decode()
                .map_err(|_| Error::Pairing)?;
            if found.is_some() {
                return Err(Error::Pairing);
            }
            found = Some(Self::parse(&decoded.payload)?);
        }
        found.ok_or(Error::Pairing)
    }
}

fn write_private(destination: &Path, bytes: &[u8]) -> Result<()> {
    let parent = profile::parent(destination)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(destination)
        .map_err(|_| Error::Destination("invitation output already exists"))?;
    std::fs::File::open(parent)?.sync_all()?;
    Ok(())
}

async fn read_private(path: &Path, limit: u64) -> Result<zeroize::Zeroizing<Vec<u8>>> {
    use tokio::io::AsyncReadExt;
    profile::parent(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_file()
            || metadata.mode() & 0o777 != 0o600
            || metadata.nlink() != 1
            || metadata.uid() != rustix::process::geteuid().as_raw()
        {
            return Err(Error::Identity(
                "invitation must be an owned private regular file",
            ));
        }
    }
    let file = crate::files::source_file(path)?;
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    file.take(limit + 1).read_to_end(&mut bytes).await?;
    if bytes.len() as u64 > limit {
        return Err(Error::Pairing);
    }
    Ok(bytes)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[tokio::test]
    async fn private_bundle_and_qr_roundtrip_without_long_term_key_export() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("private");
        profile::private_directory(&folder, true).unwrap();
        let identity = Identity::create(&root.path().join("identity")).unwrap();
        let server = PairServer::bind(
            "127.0.0.1:0".parse().unwrap(),
            std::time::Duration::from_secs(60),
        )
        .await
        .unwrap();
        let invite = Invite::from_server(&server, &identity).unwrap();
        let wire: serde_json::Value = serde_json::from_slice(&invite.bytes().unwrap()).unwrap();
        assert_eq!(wire["version"], 1);
        assert!(wire.get("room").is_none()); // Preserve the contact-v1 envelope.
        let bundle = folder.join("invite.json");
        let qr = folder.join("invite.png");
        invite.export(&bundle).unwrap();
        invite.qr(&qr).unwrap();
        for loaded in [
            Invite::load(&bundle).await.unwrap(),
            Invite::load_qr(&qr).await.unwrap(),
        ] {
            assert_eq!(loaded.id, invite.id);
            assert_eq!(loaded.scope, Scope::Contact);
            assert_eq!(loaded.address, invite.address);
            assert_eq!(loaded.fingerprint, identity.peer().fingerprint());
            assert_eq!(loaded.code().expose_secret(), invite.code().expose_secret());
        }
        assert!(invite.export(&bundle).is_err());
    }
    #[tokio::test]
    async fn room_bundle_and_qr_keep_the_exact_room_scope_without_key_export() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("private");
        profile::private_directory(&folder, true).unwrap();
        let identity = Identity::create(&root.path().join("identity")).unwrap();
        let room = [17; 32];
        let server = PairServer::bind_room(
            "127.0.0.1:0".parse().unwrap(),
            std::time::Duration::from_secs(10),
            room,
        )
        .await
        .unwrap();
        let invite = Invite::from_server(&server, &identity).unwrap();
        let bundle = folder.join("invite.json");
        let qr = folder.join("invite.png");
        invite.export(&bundle).unwrap();
        invite.qr(&qr).unwrap();
        for loaded in [
            Invite::load(&bundle).await.unwrap(),
            Invite::load_qr(&qr).await.unwrap(),
        ] {
            assert_eq!(loaded.scope, Scope::Room(room));
            assert_eq!(loaded.id, invite.id);
            assert_eq!(loaded.fingerprint, identity.peer().fingerprint());
        }
        let mut wire: serde_json::Value = serde_json::from_slice(&invite.bytes().unwrap()).unwrap();
        wire["version"] = 1.into();
        assert!(Invite::parse(&serde_json::to_vec(&wire).unwrap()).is_err());
    }
}
