//! One-use private-file bootstrap, then origin/port-scoped browser storage and
//! bearer headers. No cookies: localhost cookies are not isolated by TCP port.
use crate::{Error, Result, profile};
use ring::{
    hmac,
    rand::{SecureRandom, SystemRandom},
};
use std::{
    io::Write,
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use zeroize::Zeroizing;

const BOOTSTRAP: &[u8] = b"K&K UI bootstrap v1\0";
const SESSION: &[u8] = b"K&K UI session v1\0";

pub(super) struct Auth {
    key: hmac::Key,
    used: AtomicBool,
    launch: Mutex<Option<tempfile::NamedTempFile>>,
}
impl Auth {
    pub fn new(directory: &Path, origin: &str) -> Result<Self> {
        profile::private_directory(directory, true)?;
        let mut secret = Zeroizing::new([0; 32]);
        SystemRandom::new()
            .fill(secret.as_mut())
            .map_err(|_| Error::Authentication)?;
        let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_ref());
        let tag = hex(hmac::sign(&key, BOOTSTRAP).as_ref());
        let html = Zeroizing::new(format!(
            "<!doctype html><meta charset=\"utf-8\"><meta name=\"referrer\" content=\"no-referrer\"><meta http-equiv=\"refresh\" content=\"0;url={origin}/#{tag}\"><title>K&amp;K private launch</title>Opening the local K&amp;K interface."
        ));
        let mut launch = tempfile::Builder::new()
            .prefix("ui-launch-")
            .suffix(".html")
            .tempfile_in(directory)?;
        launch.write_all(html.as_bytes())?;
        launch.as_file().sync_all()?;
        Ok(Self {
            key,
            used: AtomicBool::new(false),
            launch: Mutex::new(Some(launch)),
        })
    }
    pub fn launch_path(&self) -> Result<std::path::PathBuf> {
        self.launch
            .lock()
            .map_err(|_| Error::Authentication)?
            .as_ref()
            .map(|file| file.path().to_owned())
            .ok_or(Error::Authentication)
    }
    pub fn bootstrap(&self, token: &str) -> Result<Zeroizing<String>> {
        verify(&self.key, BOOTSTRAP, token)?;
        self.used
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| Error::Authentication)?;
        self.launch
            .lock()
            .map_err(|_| Error::Authentication)?
            .take();
        Ok(Zeroizing::new(hex(hmac::sign(&self.key, SESSION).as_ref())))
    }
    pub fn session(&self, token: &str) -> bool {
        self.used.load(Ordering::SeqCst) && verify(&self.key, SESSION, token).is_ok()
    }
}
fn verify(key: &hmac::Key, purpose: &[u8], token: &str) -> Result<()> {
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Authentication);
    }
    let mut bytes = Zeroizing::new([0; 32]);
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&token[index * 2..index * 2 + 2], 16)
            .map_err(|_| Error::Authentication)?;
    }
    hmac::verify(key, purpose, bytes.as_ref()).map_err(|_| Error::Authentication)
}
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut value, "{byte:02x}").expect("String formatting");
    }
    value
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn private_bootstrap_is_one_use_and_not_an_api_session_or_other_launch() {
        let root = tempfile::tempdir().unwrap();
        let auth = Auth::new(&root.path().join("private"), "http://127.0.0.1:1234").unwrap();
        let launch = auth.launch_path().unwrap();
        let bootstrap = hex(hmac::sign(&auth.key, BOOTSTRAP).as_ref());
        assert!(!auth.session(&bootstrap));
        assert!(!auth.session(&"0".repeat(64)));
        let session = auth.bootstrap(&bootstrap).unwrap();
        assert!(auth.session(&session));
        assert!(!launch.exists());
        assert!(auth.bootstrap(&bootstrap).is_err());
        let other = Auth::new(&root.path().join("other"), "http://127.0.0.1:1234").unwrap();
        assert!(!other.session(&session));
    }
}
