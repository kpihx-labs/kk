// Copyright (c) Mike Grier.

//! A real directory tree for the acceptance target.

use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use windows_namespace_request_sys::OpenFile;
use windows_namespace_request_sys::watch::WatchDirectory;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use wtf_string::Wtf16String;

/// The share mode every audited consumer uses.
pub const AUDITED_SHARE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// Enough files that a small batch cannot drain the directory.
const FILE_COUNT: usize = 24;

/// A temporary directory tree that removes itself.
pub struct Tree {
    root: PathBuf,
}

impl Tree {
    pub fn new(label: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("wnrs-acceptance-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("child")).expect("create the tree");
        for index in 0..FILE_COUNT {
            std::fs::write(root.join(format!("f{index:02}.t")), b"contents").expect("write a file");
        }
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn file(&self, index: usize) -> PathBuf {
        self.root.join(format!("f{index:02}.t"))
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The path as a submitting thread would give it.
fn wide(path: &Path) -> Wtf16String {
    let text = path.to_str().expect("the tree's paths are valid UTF-8");
    Wtf16String::from(text)
}

/// A request to open `path`, built the way a submitting thread would build it.
pub fn open_file(path: &Path) -> OpenFile {
    OpenFile::new(wide(path)).expect("build the open request")
}

/// A request to watch `path`, built the way a submitting thread would build it.
pub fn watch_directory(path: &Path) -> WatchDirectory {
    WatchDirectory::new(wide(path)).expect("build the watch request")
}

/// The directory open shape shared by every audited consumer.
pub fn audited_directory_open(path: &Path) -> OpenFile {
    open_file(path)
        .with_desired_access(FILE_LIST_DIRECTORY)
        .with_share_mode(AUDITED_SHARE)
        .with_creation_disposition(OPEN_EXISTING)
        .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS)
}

/// Opens a directory with std, where a handle rather than a request is wanted.
pub fn open_directory(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .expect("open a directory")
}
