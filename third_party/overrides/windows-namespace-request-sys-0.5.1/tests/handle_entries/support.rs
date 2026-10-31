// Copyright (c) Mike Grier.

//! A real directory tree, and the audited request shapes built against it.

use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use windows_namespace_request_sys::OpenFile;
use windows_namespace_request_sys::watch::WatchDirectory;
use windows_sys::Win32::Foundation::FALSE;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle, OPEN_EXISTING,
};
use wtf_string::Wtf16String;

/// The share mode every audited consumer uses: an observer must not stop
/// anyone else reading, writing, renaming, or deleting.
pub const AUDITED_SHARE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// How many files the tree holds, enough that a directory is not trivially
/// empty.
const FILE_COUNT: usize = 12;

/// A temporary directory tree that removes itself.
pub struct Tree {
    root: PathBuf,
}

impl Tree {
    /// Creates a root holding several files and one child directory.
    pub fn new(label: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("wnrs-integration-{}-{label}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("child")).expect("create the tree");

        for index in 0..FILE_COUNT {
            std::fs::write(root.join(format!("f{index:02}.t")), b"contents").expect("write a file");
        }

        Self { root }
    }

    /// The tree's root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The child directory beneath the root.
    pub fn child(&self) -> PathBuf {
        self.root.join("child")
    }

    /// One of the files in the root.
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

/// The shape `windows-file-enumeration-sys` and Globazog use: a directory
/// opened for listing, with no overlapped flag.
pub fn unassociated_directory(path: &Path) -> OpenFile {
    open_file(path)
        .with_desired_access(FILE_LIST_DIRECTORY)
        .with_share_mode(AUDITED_SHARE)
        .with_creation_disposition(OPEN_EXISTING)
        .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS)
}

/// Opens a directory with std, for the cases that need a handle rather than a
/// request.
pub fn open_directory(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .expect("open a directory")
}

/// Reads a handle's 64-bit file reference number.
///
/// `GetFileInformationByHandle` becomes entry 6 in M26.2; until then, calling
/// it directly here is honest rather than building half of that entry early.
pub fn file_id_of(file: &File) -> u64 {
    // SAFETY: the handle is live for the call and the out-parameter is
    // writable.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let read = unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &raw mut info) };
    assert_ne!(read, FALSE, "read a file's identity");

    (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow)
}
