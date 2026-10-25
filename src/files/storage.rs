//! Linux descriptor-relative private staging and atomic no-replace publication.

use std::{ffi::OsString, fs::File as StdFile, os::unix::fs::MetadataExt, path::Path};

use rustix::{
    fd::OwnedFd,
    fs::{self, Mode, OFlags},
};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
};

use crate::{Error, Result};

pub(super) struct Store {
    directory: OwnedFd,
    output: OsString,
    partial: OsString,
    metadata: OsString,
    pub file: File,
    pub complete: bool,
}

fn private_file(file: &StdFile) -> Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.nlink() != 1
        || metadata.mode() & 0o777 != 0o600
    {
        return Err(Error::Destination(
            "expected an owned, private, single-link regular file",
        ));
    }
    Ok(())
}

fn open(directory: &OwnedFd, name: &OsString, flags: OFlags) -> rustix::io::Result<StdFile> {
    fs::openat(
        directory,
        name,
        flags | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::RUSR | Mode::WUSR,
    )
    .map(StdFile::from)
}

impl Store {
    pub async fn open(destination: &Path, binding: &[u8; 73]) -> Result<Self> {
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let output = destination
            .file_name()
            .ok_or(Error::Destination("missing destination filename"))?
            .to_os_string();
        let directory = fs::open(
            parent,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        let attributes = fs::fstat(&directory)?;
        if attributes.st_uid != rustix::process::geteuid().as_raw()
            || attributes.st_mode & 0o777 != 0o700
        {
            return Err(Error::Destination(
                "parent must be user-owned with mode 0700",
            ));
        }
        let mut partial = output.clone();
        partial.push(".kk-part");
        let mut metadata = output.clone();
        metadata.push(".kk-meta");
        let (file, complete) = match open(&directory, &output, OFlags::RDONLY) {
            Ok(file) => (file, true),
            Err(rustix::io::Errno::NOENT) => (
                open(&directory, &partial, OFlags::RDWR | OFlags::CREATE)?,
                false,
            ),
            Err(_) => return Err(Error::Destination("cannot safely open destination")),
        };
        private_file(&file)?;
        fs::flock(&file, fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| Error::Destination("destination is locked or cannot be locked"))?;
        if !complete {
            match open(&directory, &metadata, OFlags::RDONLY) {
                Ok(meta) => {
                    private_file(&meta)?;
                    if meta.metadata()?.len() != binding.len() as u64 {
                        return Err(Error::Destination("invalid resume metadata"));
                    }
                    let mut actual = [0; 73];
                    File::from_std(meta).read_exact(&mut actual).await?;
                    if &actual != binding {
                        return Err(Error::Destination(
                            "partial belongs to different content or peer",
                        ));
                    }
                }
                Err(rustix::io::Errno::NOENT) => {
                    if file.metadata()?.len() != 0 {
                        return Err(Error::Destination("partial has no trusted resume metadata"));
                    }
                    let meta = open(
                        &directory,
                        &metadata,
                        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL,
                    )?;
                    private_file(&meta)?;
                    let mut meta = File::from_std(meta);
                    meta.write_all(binding).await?;
                    meta.sync_all().await?;
                    fs::fsync(&directory)?;
                }
                Err(_) => return Err(Error::Destination("cannot safely open resume metadata")),
            }
        }
        Ok(Self {
            directory,
            output,
            partial,
            metadata,
            file: File::from_std(file),
            complete,
        })
    }

    pub async fn commit(&mut self) -> Result<()> {
        if !self.complete {
            self.file.sync_all().await?;
            fs::renameat_with(
                &self.directory,
                &self.partial,
                &self.directory,
                &self.output,
                fs::RenameFlags::NOREPLACE,
            )
            .map_err(|_| Error::Destination("atomic no-replace publication failed"))?;
            fs::fsync(&self.directory)?;
            fs::unlinkat(&self.directory, &self.metadata, fs::AtFlags::empty())?;
            fs::fsync(&self.directory)?;
            self.complete = true;
        }
        fs::fsync(&self.directory)?;
        Ok(())
    }
}
