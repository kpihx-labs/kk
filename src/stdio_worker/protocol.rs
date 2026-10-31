//! Private inherited-handle protocol shared by the supervised native workers.
use crate::config;
#[cfg(any(target_vendor = "apple", windows, test))]
use crate::{Error, Result};
#[cfg(windows)]
use std::io::IsTerminal;
#[cfg(any(target_vendor = "apple", windows, test))]
use std::{
    fs::File,
    io::{self, Read, Write},
};
#[cfg(any(target_vendor = "apple", windows, test))]
use zeroize::Zeroizing;

#[cfg(any(target_vendor = "apple", windows, test))]
pub(super) const MAGIC: &[u8] = b"KK-STDIO-1\n";
#[cfg(any(target_vendor = "apple", windows, test))]
pub(super) const STARTED: u8 = 127;
pub(super) const READ: u8 = 1;
pub(super) const WRITE: u8 = 2;
pub(super) const OUTPUT_LIMIT: usize = config::MAX_MESSAGE_BYTES * 6 + 8193;

#[cfg(any(target_vendor = "apple", windows, test))]
pub(super) fn serve(mut control: File, mut response: File, mut data: File) -> Result<()> {
    // Protocol IO never holds global stdio locks: supervisor EOF must be able
    // to terminate the process even during blocked data or response IO.
    response.write_all(MAGIC)?;
    let mut header = [0; 5];
    control.read_exact(&mut header)?;
    let size = u32::from_be_bytes(header[1..].try_into().expect("fixed header")) as usize;
    let operation = header[0];
    if !matches!((operation, size), (READ, n) if n == config::MAX_MESSAGE_BYTES)
        && !(operation == WRITE && size <= OUTPUT_LIMIT)
    {
        return Err(Error::Configuration("invalid stdio worker request"));
    }
    let mut bytes = Zeroizing::new(vec![0; if operation == WRITE { size } else { 0 }]);
    control.read_exact(&mut bytes)?;
    std::thread::scope(|scope| {
        let _supervisor = std::thread::Builder::new()
            .name("kk-stdio-watch".into())
            .spawn_scoped(scope, move || {
                let mut byte = [0];
                loop {
                    match control.read(&mut byte) {
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        _ => std::process::exit(125),
                    }
                }
            })?;
        if response.write_all(&[STARTED]).is_err() {
            drop(bytes);
            std::process::exit(1);
        }
        let result = if operation == READ {
            read_bounded(&mut data)
        } else {
            write_data(&mut data, &bytes)
                .map(|()| Zeroizing::new(Vec::new()))
                .map_err(Error::Io)
        };
        let (code, number) = match &result {
            Ok(bytes) => (0, bytes.len() as u32),
            Err(Error::MessageTooLarge) => (1, 0),
            Err(Error::Io(error)) => {
                #[cfg(windows)]
                let fallback = 31; // ERROR_GEN_FAILURE, not a fabricated access denial.
                #[cfg(not(windows))]
                let fallback = 5;
                (2, error.raw_os_error().unwrap_or(fallback) as u32)
            }
            Err(_) => (3, 0),
        };
        let sent = (|| -> io::Result<()> {
            response.write_all(&[code])?;
            response.write_all(&number.to_be_bytes())?;
            if let Ok(bytes) = &result {
                response.write_all(bytes)?;
            }
            response.flush()
        })();
        drop(bytes);
        drop(result);
        std::process::exit(if sent.is_ok() { 0 } else { 1 });
    })
}

#[cfg(any(target_vendor = "apple", windows, test))]
fn write_data(data: &mut File, bytes: &[u8]) -> io::Result<()> {
    #[cfg(windows)]
    if data.is_terminal() {
        // stderr is the explicitly inherited data handle in this worker. Rust's
        // console writer performs UTF-8/UTF-16 conversion; raw WriteFile does not.
        // It never holds the global stdout lock used by process exit cleanup.
        return io::stderr().write_all(bytes);
    }
    data.write_all(bytes)
}

#[cfg(any(target_vendor = "apple", windows, test))]
pub(super) fn read_bounded(data: &mut File) -> Result<Zeroizing<Vec<u8>>> {
    #[cfg(windows)]
    if data.is_terminal() {
        return Err(Error::Configuration("raw Windows console input refused"));
    }
    let mut bytes = Zeroizing::new(Vec::new());
    let mut chunk = Zeroizing::new([0; 1024]);
    loop {
        let count = match data.read(chunk.as_mut()) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            return Ok(bytes);
        }
        if bytes.len() + count > config::MAX_MESSAGE_BYTES {
            return Err(Error::MessageTooLarge);
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}
