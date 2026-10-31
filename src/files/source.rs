//! Retain a selected source before acknowledging an asynchronous file job.
use super::{FileInfo, SendSource};
use crate::{Error, Result};
use std::path::Path;
use tokio::fs::File;

impl SendSource {
    /// No bytes are read and no network effect occurs here. The opened regular,
    /// read-only inode survives deletion of a UI-owned upload tempfile pathname.
    pub(crate) fn capture(path: &Path) -> Result<Self> {
        Ok(Self {
            file: super::source_file(path)?,
            expected: None,
        })
    }

    pub(crate) async fn prepare(mut self) -> Result<(File, FileInfo)> {
        let info = match self.expected {
            Some(info) => {
                if self.file.metadata().await?.len() != info.bytes {
                    return Err(Error::Integrity);
                }
                info
            }
            None => super::info(&mut self.file).await?,
        };
        Ok((self.file, info))
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use std::io::Write;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn captured_upload_survives_its_tempfile_guard_and_is_rewound_by_sender_prefix() {
        let mut upload = tempfile::NamedTempFile::new().unwrap();
        upload.write_all(b"captured upload").unwrap();
        let source = SendSource::capture(upload.path()).unwrap();
        drop(upload);
        let (mut file, info) = source.prepare().await.unwrap();
        assert_eq!(info.bytes, 15);
        super::super::digest(&mut file, 0).await.unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).await.unwrap();
        assert_eq!(bytes, b"captured upload");
    }
}
