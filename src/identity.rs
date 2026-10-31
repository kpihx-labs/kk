use std::{fs, io::Read, path::Path};

#[cfg(any(unix, windows))]
use rustls::pki_types::PrivatePkcs8KeyDer;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};

use crate::{Error, Result, config};

#[cfg(unix)]
#[path = "identity/unix.rs"]
mod unix;

/// A verified out-of-band public identity. Constructing it does not verify its origin.
#[derive(Clone)]
pub struct Peer {
    pub(crate) certificate: CertificateDer<'static>,
}

impl Peer {
    /// Load a bounded public DER certificate. The caller must verify its fingerprint.
    pub fn load(path: &Path) -> Result<Self> {
        Self::from_der(read_bounded(path)?)
    }

    pub(crate) fn from_der(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() > config::MAX_IDENTITY_BYTES {
            return Err(Error::Identity("certificate is too large"));
        }
        let certificate = CertificateDer::from(bytes);
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(certificate.clone())
            .map_err(|_| Error::Identity("invalid DER certificate"))?;
        Ok(Self { certificate })
    }

    /// SHA-256 of the public certificate, for comparison over a trusted channel.
    pub fn fingerprint(&self) -> String {
        use std::fmt::Write;
        let mut fingerprint = String::with_capacity(64);
        for byte in self.fingerprint_bytes() {
            write!(fingerprint, "{byte:02x}").expect("writing into a String");
        }
        fingerprint
    }
    pub fn fingerprint_bytes(&self) -> [u8; 32] {
        Sha256::digest(self.certificate.as_ref()).into()
    }
    /// Public identity export only; this exposes no device or room private key.
    pub fn certificate_der(&self) -> &[u8] {
        self.certificate.as_ref()
    }
}

/// Local certificate and private key. Deliberately has no Debug implementation.
pub struct Identity {
    pub(crate) public: Peer,
    pub(crate) key: PrivateKeyDer<'static>,
}

impl Identity {
    /// Create a new private identity directory. Never replaces existing state.
    #[cfg(unix)]
    pub fn create(directory: &Path) -> Result<Self> {
        let directory = unix::Directory::create(directory)?;
        let generated = rcgen::generate_simple_self_signed(vec![config::TLS_NAME.to_owned()])
            .map_err(|_| Error::Identity("certificate generation failed"))?;
        let certificate = generated.cert.der().to_vec();
        let key = PrivatePkcs8KeyDer::from(generated.signing_key.serialize_der());
        // A partial failure leaves a private directory, never a silently regenerated key.
        directory.write("private-key.der", key.secret_pkcs8_der(), 0o600)?;
        directory.write("certificate.der", &certificate, 0o644)?;
        directory.sync()?;
        Self::load_opened(&directory)
    }

    #[cfg(not(unix))]
    pub fn create(_directory: &Path) -> Result<Self> {
        Err(Error::UnsupportedPlatform)
    }

    /// Load an existing protected identity; no implicit creation or repair.
    #[cfg(unix)]
    pub fn load(directory: &Path) -> Result<Self> {
        Self::load_opened(&unix::Directory::open(directory)?)
    }

    #[cfg(unix)]
    fn load_opened(directory: &unix::Directory) -> Result<Self> {
        let public = Peer::from_der(directory.read("certificate.der", false)?)?;
        let key: PrivateKeyDer<'static> =
            PrivatePkcs8KeyDer::from(directory.read("private-key.der", true)?).into();
        Self::from_parts(public, key)
    }

    #[cfg(any(unix, windows))]
    fn from_parts(public: Peer, key: PrivateKeyDer<'static>) -> Result<Self> {
        // Rustls checks certificate/key consistency before an identity can be used.
        rustls::sign::CertifiedKey::from_der(
            vec![public.certificate.clone()],
            key.clone_key(),
            &rustls::crypto::ring::default_provider(),
        )
        .and_then(|certified_key| certified_key.keys_match())
        .map_err(|_| Error::Identity("invalid or mismatched private key"))?;
        Ok(Self { public, key })
    }

    /// Windows capability backend: reserve a fresh identity directory before
    /// generating anything. An existing/partial directory is never adopted.
    #[cfg(windows)]
    pub fn create_windows(
        parent: &crate::profile::windows::ProtectedDirectory,
        leaf: &std::ffi::OsStr,
    ) -> Result<Self> {
        let directory = parent.create_child(leaf)?;
        let generated = rcgen::generate_simple_self_signed(vec![config::TLS_NAME.to_owned()])
            .map_err(|_| Error::Identity("certificate generation failed"))?;
        let certificate = generated.cert.der().to_vec();
        let key = PrivatePkcs8KeyDer::from(generated.signing_key.serialize_der());
        let mut private = directory.create_private_file(std::ffi::OsStr::new("private-key.der"))?;
        private.write_bytes(key.secret_pkcs8_der())?;
        drop(private);
        // The certificate is the final publication, never a partial named DER.
        directory.publish_public(std::ffi::OsStr::new("certificate.der"), &certificate)?;
        Self::load_windows_opened(&directory)
    }

    /// Load through a retained parent capability without pathname bootstrap.
    #[cfg(windows)]
    pub fn load_windows(
        parent: &crate::profile::windows::ProtectedDirectory,
        leaf: &std::ffi::OsStr,
    ) -> Result<Self> {
        let directory = parent.open_child(leaf)?;
        Self::load_windows_opened(&directory)
    }

    #[cfg(windows)]
    fn load_windows_opened(
        directory: &crate::profile::windows::ProtectedDirectory,
    ) -> Result<Self> {
        let mut certificate =
            directory.read_private_file(std::ffi::OsStr::new("certificate.der"))?;
        let public = Peer::from_der(std::mem::take(&mut *certificate))?;
        let mut private = directory.read_private_file(std::ffi::OsStr::new("private-key.der"))?;
        let key = PrivatePkcs8KeyDer::from(std::mem::take(&mut *private)).into();
        Self::from_parts(public, key)
    }

    #[cfg(not(unix))]
    pub fn load(_directory: &Path) -> Result<Self> {
        Err(Error::UnsupportedPlatform)
    }

    /// Public identity only; never exports the private key.
    pub fn peer(&self) -> Peer {
        self.public.clone()
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    #[cfg(unix)]
    let file = fs::File::from(rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )?);
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        // OPEN_REPARSE_POINT inspects the opened object, not its link target.
        // Only read sharing is permitted while this bounded import is active.
        // Public certificates confer no trust until fingerprint verification.
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const FILE_SHARE_READ: u32 = 1;
        fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?
    };
    #[cfg(not(any(unix, windows)))]
    let file = {
        if !fs::symlink_metadata(path)?.is_file() {
            return Err(Error::Identity(
                "expected a regular non-symlink identity file",
            ));
        }
        fs::File::open(path)?
    };
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(Error::Identity(
                "public identity must not be a reparse point",
            ));
        }
    }
    if !metadata.is_file() {
        return Err(Error::Identity(
            "expected a regular non-symlink identity file",
        ));
    }
    if metadata.len() > config::MAX_IDENTITY_BYTES as u64 {
        return Err(Error::Identity("identity file is too large"));
    }
    let mut bytes = Vec::new();
    file.take((config::MAX_IDENTITY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > config::MAX_IDENTITY_BYTES {
        return Err(Error::Identity("identity file is too large"));
    }
    Ok(bytes)
}
