//! Linux/macOS descriptor-relative private staging and atomic no-replace publication.

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

struct CancelPublication(std::sync::Arc<std::sync::Mutex<bool>>);

impl Drop for CancelPublication {
    fn drop(&mut self) {
        // Dropping an async waiter cannot cancel spawn_blocking. Share a gate
        // with the namespace mutation instead, and recover poison to revoke infallibly.
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
    }
}

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

/// Open one completed inbox object, relative to a retained private directory.
/// The caller validates the object ID and verifies its content before exposure.
pub(super) fn open_received(directory: &Path, name: &str) -> Result<File> {
    received_at(&received_directory(directory)?, name)
}

pub(super) fn received_directory(directory: &Path) -> Result<OwnedFd> {
    let directory = fs::open(
        directory,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let attributes = fs::fstat(&directory)?;
    if attributes.st_uid != rustix::process::geteuid().as_raw()
        || attributes.st_mode & 0o777 != 0o700
    {
        return Err(Error::Destination(
            "inbox must be user-owned with mode 0700",
        ));
    }
    Ok(directory)
}

pub(super) fn received_at(directory: &OwnedFd, name: &str) -> Result<File> {
    let file = open(directory, &OsString::from(name), OFlags::RDONLY)?;
    private_file(&file)?;
    fs::flock(&file, fs::FlockOperation::NonBlockingLockShared)
        .map_err(|_| Error::Destination("inbox object is locked or cannot be locked"))?;
    Ok(File::from_std(file))
}

/// Keep the owner's work lease in the worker even if the async waiter leaves.
pub(super) async fn delete_received(
    directory: &Path,
    name: &str,
    lease: impl Send + 'static,
) -> Result<bool> {
    let directory = received_directory(directory)?;
    let name = OsString::from(name);
    let mut partial = name.clone();
    partial.push(".kk-part");
    let live = std::sync::Arc::new(std::sync::Mutex::new(true));
    let _cancel = CancelPublication(live.clone());
    tokio::task::spawn_blocking(move || {
        let _lease = lease;
        let no_staging = || -> Result<()> {
            match fs::statat(&directory, &partial, fs::AtFlags::SYMLINK_NOFOLLOW) {
                Err(rustix::io::Errno::NOENT) => Ok(()),
                Ok(_) => Err(Error::Destination("received copy has staging state")),
                Err(error) => Err(error.into()),
            }
        };
        no_staging()?;
        let file = match open(&directory, &name, OFlags::RDONLY) {
            Ok(file) => file,
            Err(rustix::io::Errno::NOENT) => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        private_file(&file)?;
        fs::flock(&file, fs::FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| Error::Destination("received copy is in use"))?;
        let active = live.lock().map_err(|_| Error::Protocol)?;
        if !*active {
            return Err(Error::Cancelled);
        }
        no_staging()?;
        private_file(&file)?;
        let opened = fs::fstat(&file)?;
        let named = fs::statat(&directory, &name, fs::AtFlags::SYMLINK_NOFOLLOW)?;
        if opened.st_dev != named.st_dev || opened.st_ino != named.st_ino {
            return Err(Error::Destination("received copy changed during deletion"));
        }
        fs::unlinkat(&directory, &name, fs::AtFlags::empty())?;
        drop(active);
        fs::fsync(&directory)?;
        Ok(true)
    })
    .await
    .map_err(|_| Error::Node("received copy deletion worker failed"))?
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

    pub async fn commit(self) -> Result<()> {
        self.file.sync_all().await?;
        self.commit_with_publication(()).await
    }

    // The stream has already flushed, synced and verified the staged file.
    // Only the no-replace rename belongs inside the consent gate; fsync and
    // metadata cleanup must not make synchronous withdrawal wait on durability.
    pub async fn commit_with_publication(
        mut self,
        publication: impl super::stream::Publication,
    ) -> Result<()> {
        let live = std::sync::Arc::new(std::sync::Mutex::new(true));
        let _cancel = CancelPublication(live.clone());
        tokio::task::spawn_blocking(move || {
            let was_complete = self.complete;
            publication.publish(|| {
                let active = live.lock().map_err(|_| Error::Protocol)?;
                if !*active {
                    return Err(Error::Cancelled);
                }
                self.publish()
            })?;
            self.finish_publication(!was_complete)
        })
        .await
        .map_err(|_| Error::Node("file publication worker failed"))?
    }

    fn publish(&mut self) -> Result<()> {
        if !self.complete {
            // rustix maps NOREPLACE to Linux RENAME_NOREPLACE or macOS
            // renameatx_np(RENAME_EXCL). Never fall back to clobbering rename.
            fs::renameat_with(
                &self.directory,
                &self.partial,
                &self.directory,
                &self.output,
                fs::RenameFlags::NOREPLACE,
            )
            .map_err(|_| Error::Destination("atomic no-replace publication failed"))?;
            self.complete = true;
        }
        Ok(())
    }

    fn finish_publication(&self, newly_published: bool) -> Result<()> {
        // An already-complete destination has no staging metadata to remove.
        // Newly published files retain their metadata until the rename is durable.
        if newly_published {
            fs::fsync(&self.directory)?;
            fs::unlinkat(&self.directory, &self.metadata, fs::AtFlags::empty())?;
        }
        fs::fsync(&self.directory)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::stream::Publication;
    use std::os::unix::fs::PermissionsExt;

    struct Gate<F>(F);

    impl<F> Publication for Gate<F>
    where
        F: FnOnce() -> Result<()> + Send + 'static,
    {
        fn publish(self, rename: impl FnOnce() -> Result<()>) -> Result<()> {
            (self.0)()?;
            rename()
        }
    }

    async fn staged() -> (tempfile::TempDir, std::path::PathBuf, Store) {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = root.path().join("output");
        let mut store = Store::open(&output, &[0; 73]).await.unwrap();
        store.file.write_all(b"verified fixture").await.unwrap();
        store.file.sync_all().await.unwrap();
        (root, output, store)
    }

    #[tokio::test]
    async fn refused_publication_keeps_private_staging() {
        let (_root, output, store) = staged().await;
        assert!(matches!(
            store
                .commit_with_publication(Gate(|| Err(Error::Cancelled)))
                .await,
            Err(Error::Cancelled)
        ));
        assert!(!output.exists());
        assert_eq!(
            std::fs::read(output.with_file_name("output.kk-part")).unwrap(),
            b"verified fixture"
        );
        assert!(output.with_file_name("output.kk-meta").exists());
    }

    #[tokio::test]
    async fn destination_created_at_publication_is_never_clobbered() {
        let (_root, output, store) = staged().await;
        let competing = output.clone();
        assert!(matches!(
            store
                .commit_with_publication(Gate(move || {
                    std::fs::write(competing, b"competing output")?;
                    Ok(())
                }))
                .await,
            Err(Error::Destination(_))
        ));
        assert_eq!(std::fs::read(output).unwrap(), b"competing output");
    }

    #[tokio::test]
    async fn renamed_parent_keeps_publication_bound_to_the_open_directory() {
        let (root, output, store) = staged().await;
        let container = tempfile::tempdir().unwrap();
        let moved = container.path().join("moved");
        std::fs::rename(root.path(), &moved).unwrap();
        // Both paths remain inside owned temporary directories for cleanup.
        std::fs::create_dir(root.path()).unwrap();
        store.commit_with_publication(()).await.unwrap();
        assert!(!output.exists());
        assert_eq!(
            std::fs::read(moved.join("output")).unwrap(),
            b"verified fixture"
        );
        assert!(!moved.join("output.kk-meta").exists());
    }

    struct PausedPublication {
        entered: tokio::sync::oneshot::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
        finished: tokio::sync::oneshot::Sender<bool>,
    }

    impl Publication for PausedPublication {
        fn publish(self, rename: impl FnOnce() -> Result<()>) -> Result<()> {
            self.entered.send(()).unwrap();
            self.release.recv().unwrap();
            let result = rename();
            self.finished
                .send(matches!(result, Err(Error::Cancelled)))
                .unwrap();
            result
        }
    }

    #[tokio::test]
    async fn dropping_async_waiter_revokes_a_queued_blocking_publication() {
        let (_root, output, store) = staged().await;
        let (entered, entered_rx) = tokio::sync::oneshot::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        let (finished, finished_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(store.commit_with_publication(PausedPublication {
            entered,
            release: release_rx,
            finished,
        }));
        entered_rx.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        assert!(finished_rx.await.unwrap());
        assert!(!output.exists());
        assert!(output.with_file_name("output.kk-part").exists());
        assert!(output.with_file_name("output.kk-meta").exists());
    }
}
