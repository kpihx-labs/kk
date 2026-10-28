//! Bounded, synchronous, in-memory standard AGE records. No key or storage lifecycle.
//!
//! Keys are typed X25519 only; no paths, passphrases, plugins or ambient configuration.
//! Inputs are a restricted binary AGE subset: one X25519 stanza, at most one grease
//! stanza and one payload frame, not arbitrary AGE files or ASCII armor.
//! See `docs/history-records.md` for work bounds and upstream zeroization limitations.
//!
//! The pinned AGE X25519 identity deliberately has no `Debug` implementation. This
//! is not a claim that identity formatting produces a redacted string:
//! ```compile_fail,E0277
//! let synthetic_identity = age::x25519::Identity::generate();
//! let _ = format!("{synthetic_identity:?}");
//! ```

use std::io::{Cursor, Read, Write};

use age::x25519;
use zeroize::Zeroizing;

use crate::{Error, Result, config};

/// Consume owned plaintext, including on failure, and return a standard binary AGE file.
/// The caller must separately protect any copies it retained before this call.
pub fn seal(plaintext: Zeroizing<Vec<u8>>, recipient: &x25519::Recipient) -> Result<Vec<u8>> {
    if plaintext.len() > config::MAX_RECORD_PLAINTEXT_BYTES {
        return Err(Error::Limit {
            resource: "record plaintext",
            max: config::MAX_RECORD_PLAINTEXT_BYTES,
        });
    }
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(recipient as &dyn age::Recipient))
            .map_err(|_| Error::Encryption)?;
    // Fixed storage prevents unbounded output growth, even if the dependency changes.
    let mut ciphertext = vec![0; config::MAX_RECORD_CIPHERTEXT_BYTES];
    let mut output = Cursor::new(ciphertext.as_mut_slice());
    let mut writer = encryptor
        .wrap_output(&mut output)
        .map_err(|_| Error::Encryption)?;
    writer
        .write_all(&plaintext)
        .map_err(|_| Error::Encryption)?;
    writer.finish().map_err(|_| Error::Encryption)?;
    let length = output.position() as usize;
    ciphertext.truncate(length);
    header_bound(&ciphertext)?;
    Ok(ciphertext)
}

/// Authenticate the complete record before exposing any plaintext to the caller.
/// All codec-owned plaintext is zeroizing, including partially decrypted error paths.
pub fn open(ciphertext: &[u8], identity: &x25519::Identity) -> Result<Zeroizing<Vec<u8>>> {
    header_bound(ciphertext)?;
    // Cursor is a bounded BufRead; AGE owns syntax, stanza validation, MAC and STREAM.
    let decryptor =
        age::Decryptor::new_buffered(Cursor::new(ciphertext)).map_err(|_| Error::Encryption)?;
    let mut reader = decryptor
        .decrypt(std::iter::once(identity as &dyn age::Identity))
        .map_err(|_| Error::Encryption)?;
    // Allocate once: never grow/reallocate a plaintext Vec with unwiped old storage.
    let mut plaintext = Zeroizing::new(vec![0; config::MAX_RECORD_PLAINTEXT_BYTES + 1]);
    let mut length = 0;
    loop {
        let count = reader
            .read(&mut plaintext[length..])
            .map_err(|_| Error::Encryption)?;
        if count == 0 {
            break;
        }
        length += count;
        if length > config::MAX_RECORD_PLAINTEXT_BYTES {
            return Err(Error::Encryption);
        }
    }
    plaintext.truncate(length);
    Ok(plaintext)
}

/// Resource/policy screening only, not an alternative AGE parser. Like file `open`,
/// locate the MAC line at a line boundary before invoking AGE's standard parser.
/// One frame makes any suffix part of that frame's authenticated ciphertext.
fn header_bound(ciphertext: &[u8]) -> Result<()> {
    if ciphertext.len() > config::MAX_RECORD_CIPHERTEXT_BYTES {
        return Err(Error::Limit {
            resource: "record ciphertext",
            max: config::MAX_RECORD_CIPHERTEXT_BYTES,
        });
    }
    const MAGIC: &[u8] = b"age-encryption.org/v1\n";
    if !ciphertext.starts_with(MAGIC) {
        return Err(Error::Encryption);
    }
    let prefix = &ciphertext[..ciphertext.len().min(config::MAX_RECORD_HEADER_BYTES)];
    let mut position = MAGIC.len();
    let mut recipients = 0;
    let mut grease = 0;
    for line in prefix[MAGIC.len()..].split_inclusive(|byte| *byte == b'\n') {
        if !line.ends_with(b"\n") {
            return Err(Error::Encryption);
        }
        position += line.len();
        if line.starts_with(b"--- ") {
            let payload = ciphertext.len() - position;
            return if recipients == 1 && (32..=config::MAX_RECORD_PAYLOAD_BYTES).contains(&payload)
            {
                Ok(())
            } else {
                Err(Error::Encryption)
            };
        }
        if let Some(stanza) = line.strip_prefix(b"-> ") {
            let tag = stanza.split(|byte| *byte == b' ' || *byte == b'\n').next();
            match tag {
                Some(b"X25519") if recipients == 0 => recipients += 1,
                Some(tag) if tag.ends_with(b"-grease") && grease == 0 => grease += 1,
                _ => return Err(Error::Encryption),
            }
        }
    }
    Err(Error::Encryption)
}
