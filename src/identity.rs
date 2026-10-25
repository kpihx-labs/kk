use std::{fs, io::Read, path::Path};

#[cfg(unix)]
use rustls::pki_types::PrivatePkcs8KeyDer;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sha2::{Digest, Sha256};

use crate::{Error, Result, config};

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
        for byte in Sha256::digest(self.certificate.as_ref()) {
            write!(fingerprint, "{byte:02x}").expect("writing into a String");
        }
        fingerprint
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
        use std::{
            io::Write,
            os::unix::fs::{DirBuilderExt, OpenOptionsExt},
        };

        let generated = rcgen::generate_simple_self_signed(vec![config::TLS_NAME.to_owned()])
            .map_err(|_| Error::Identity("certificate generation failed"))?;
        let certificate = generated.cert.der().to_vec();
        let key = PrivatePkcs8KeyDer::from(generated.signing_key.serialize_der());
        fs::DirBuilder::new().mode(0o700).create(directory)?;
        // A partial failure leaves a private directory, never a silently regenerated key.
        let mut key_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.join("private-key.der"))?;
        key_file.write_all(key.secret_pkcs8_der())?;
        key_file.sync_all()?;
        let mut certificate_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(directory.join("certificate.der"))?;
        certificate_file.write_all(&certificate)?;
        certificate_file.sync_all()?;
        Self::load(directory)
    }

    #[cfg(not(unix))]
    pub fn create(_directory: &Path) -> Result<Self> {
        Err(Error::UnsupportedPlatform)
    }

    /// Load an existing protected identity; no implicit creation or repair.
    #[cfg(unix)]
    pub fn load(directory: &Path) -> Result<Self> {
        use std::os::unix::fs::PermissionsExt;

        let metadata = fs::symlink_metadata(directory)?;
        if !metadata.is_dir() || metadata.permissions().mode() & 0o777 != 0o700 {
            return Err(Error::Identity(
                "directory must be a non-symlink with mode 0700",
            ));
        }
        let key_path = directory.join("private-key.der");
        let metadata = fs::symlink_metadata(&key_path)?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o777 != 0o600 {
            return Err(Error::Identity(
                "private key must be a regular file with mode 0600",
            ));
        }
        let public = Peer::load(&directory.join("certificate.der"))?;
        let key: PrivateKeyDer<'static> = PrivatePkcs8KeyDer::from(read_bounded(&key_path)?).into();
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
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(Error::Identity(
            "expected a regular non-symlink identity file",
        ));
    }
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > config::MAX_IDENTITY_BYTES as u64 {
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
