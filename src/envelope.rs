//! Standard age envelopes. Only ciphertext is staged while encrypting.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use age::{
    secrecy::{ExposeSecret, SecretString},
    x25519,
};
use futures_util::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, Cursor};
use tokio::{fs::File, io::AsyncReadExt as TokioReadExt};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use zeroize::Zeroizing;

use crate::{Error, Result, config, profile};

pub enum Lock {
    Recipient(String),
    Passphrase(SecretString),
}
pub enum Unlock {
    Identity(PathBuf),
    Passphrase(SecretString),
}

fn encryptor(lock: Lock) -> Result<age::Encryptor> {
    match lock {
        Lock::Recipient(text) => {
            let recipient: x25519::Recipient = text.parse().map_err(|_| Error::Encryption)?;
            age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
                .map_err(|_| Error::Encryption)
        }
        Lock::Passphrase(passphrase) => Ok(age::Encryptor::with_user_passphrase(passphrase)),
    }
}

async fn publish(temporary: tempfile::NamedTempFile, destination: &Path) -> Result<()> {
    File::from_std(temporary.reopen()?).sync_all().await?;
    temporary
        .persist_noclobber(destination)
        .map_err(|_| Error::Destination("output exists or cannot be published"))?;
    File::open(profile::parent(destination)?)
        .await?
        .sync_all()
        .await?;
    Ok(())
}

pub async fn keygen(destination: &Path) -> Result<String> {
    use std::io::Write;
    let parent = profile::parent(destination)?;
    let identity = x25519::Identity::generate();
    let recipient = identity.to_public().to_string();
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    writeln!(temporary, "{}", identity.to_string().expose_secret())?;
    publish(temporary, destination).await?;
    Ok(recipient)
}

async fn encrypt<R: AsyncRead + Unpin>(
    mut input: R,
    destination: &Path,
    lock: Lock,
) -> Result<u64> {
    let parent = profile::parent(destination)?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    let output = File::from_std(temporary.reopen()?).compat_write();
    let mut writer = encryptor(lock)?
        .wrap_async_output(output)
        .await
        .map_err(|_| Error::Encryption)?;
    let mut buffer = Zeroizing::new(vec![0; config::FILE_CHUNK_BYTES]);
    let mut total = 0;
    loop {
        let count = input.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > config::MAX_FILE_BYTES {
            return Err(Error::FileTooLarge);
        }
        writer.write_all(&buffer[..count]).await?;
    }
    writer.close().await?;
    drop(writer);
    publish(temporary, destination).await?;
    Ok(total)
}

pub async fn seal_bytes(
    bytes: &[u8],
    destination: &Path,
    lock: Lock,
    deadline: Duration,
) -> Result<u64> {
    if bytes.len() as u64 > config::MAX_FILE_BYTES {
        return Err(Error::FileTooLarge);
    }
    tokio::time::timeout(deadline, encrypt(Cursor::new(bytes), destination, lock))
        .await
        .map_err(|_| Error::Timeout)?
}

pub async fn seal_file(
    source: &Path,
    destination: &Path,
    lock: Lock,
    deadline: Duration,
) -> Result<u64> {
    let file = crate::files::source_file(source)?;
    if file.metadata().await?.len() > config::MAX_FILE_BYTES {
        return Err(Error::FileTooLarge);
    }
    tokio::time::timeout(deadline, encrypt(file.compat(), destination, lock))
        .await
        .map_err(|_| Error::Timeout)?
}

pub async fn open(
    source: &Path,
    destination: &Path,
    unlock: Unlock,
    deadline: Duration,
) -> Result<u64> {
    tokio::time::timeout(deadline, async {
        let mut input = tokio::io::BufReader::new(crate::files::source_file(source)?);
        // Bound the binary age header before handing it to the standard parser.
        let mut prefix = Vec::new();
        let mut line_start = 0;
        loop {
            let mut byte = [0];
            if input.read(&mut byte).await? == 0 || prefix.len() == config::MAX_AGE_HEADER_BYTES {
                return Err(Error::Encryption);
            }
            prefix.push(byte[0]);
            if byte[0] == b'\n' {
                if prefix[line_start..].starts_with(b"--- ") {
                    break;
                }
                line_start = prefix.len();
            }
        }
        if !prefix.starts_with(b"age-encryption.org/v1\n") {
            return Err(Error::Encryption);
        }
        let input = Cursor::new(prefix).chain(input.compat());
        let decryptor = age::Decryptor::new_async(input)
            .await
            .map_err(|_| Error::Encryption)?;
        let key: Box<dyn age::Identity> = match unlock {
            Unlock::Identity(path) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    let metadata = std::fs::symlink_metadata(&path)?;
                    if !metadata.is_file()
                        || metadata.mode() & 0o777 != 0o600
                        || metadata.nlink() != 1
                        || metadata.uid() != rustix::process::geteuid().as_raw()
                    {
                        return Err(Error::Identity(
                            "private decryption key must be owned, regular and mode 0600",
                        ));
                    }
                }
                let mut key = crate::files::source_file(&path)?;
                let mut bytes = Zeroizing::new(Vec::new());
                TokioReadExt::take(&mut key, (config::MAX_IDENTITY_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)
                    .await?;
                if bytes.len() > config::MAX_IDENTITY_BYTES {
                    return Err(Error::Identity("key too large"));
                }
                let text = std::str::from_utf8(&bytes).map_err(|_| Error::Encryption)?;
                Box::new(
                    text.trim()
                        .parse::<x25519::Identity>()
                        .map_err(|_| Error::Encryption)?,
                )
            }
            Unlock::Passphrase(passphrase) => Box::new(age::scrypt::Identity::new(passphrase)),
        };
        let mut reader = decryptor
            .decrypt_async(std::iter::once(key.as_ref()))
            .map_err(|_| Error::Encryption)?;
        let temporary = tempfile::NamedTempFile::new_in(profile::parent(destination)?)?;
        let mut output = File::from_std(temporary.reopen()?).compat_write();
        let mut buffer = Zeroizing::new(vec![0; config::FILE_CHUNK_BYTES]);
        let mut total = 0;
        loop {
            let count = reader
                .read(&mut buffer)
                .await
                .map_err(|_| Error::Encryption)?;
            if count == 0 {
                break;
            }
            total += count as u64;
            if total > config::MAX_FILE_BYTES {
                return Err(Error::FileTooLarge);
            }
            output.write_all(&buffer[..count]).await?;
        }
        output.close().await?;
        drop(output);
        publish(temporary, destination).await?;
        Ok(total)
    })
    .await
    .map_err(|_| Error::Timeout)?
}
