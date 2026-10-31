// Copyright (c) Mike Grier.

//! The fixture the crate's handle-taking tests share.
//!
//! Some tests reason about a specific handle value after it has been closed, which is
//! only meaningful while that value stays unallocated: if another thread opened
//! something in the window, Windows could hand the value back out. Every test that
//! opens a handle therefore takes [`handle_allocation`] for read, and those tests
//! take it for write, so the window is closed against this binary's own concurrency
//! rather than left to timing.

use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

/// Serialises handle allocation across this test binary.
///
/// Read: "I may open handles." Write: "no other test may open a handle while I
/// reason about a specific handle value."
pub(crate) fn handle_allocation() -> &'static RwLock<()> {
    static LOCK: OnceLock<RwLock<()>> = OnceLock::new();
    LOCK.get_or_init(|| RwLock::new(()))
}

/// A temporary directory holding one file, removed on drop.
///
/// Named per process and per label so concurrent tests cannot collide.
pub(crate) struct Fixture {
    directory: PathBuf,
}

pub(crate) const FILE_CONTENTS: &[u8] = b"contents";

impl Fixture {
    pub(crate) fn new(label: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "windows-namespace-request-sys-{}-{label}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create the fixture directory");
        std::fs::write(directory.join("f.t"), FILE_CONTENTS).expect("write the fixture file");
        Self { directory }
    }

    /// A fixture holding `extra` further files beyond `f.t`.
    ///
    /// Needed by any test that must leave a directory enumeration *mid*
    /// directory: with only `.`, `..`, and `f.t`, every batch drains the
    /// directory in one call and no cursor question can arise.
    pub(crate) fn with_extra_files(label: &str, extra: usize) -> Self {
        let fixture = Self::new(label);
        for index in 0..extra {
            std::fs::write(
                fixture.directory.join(format!("e{index:03}.t")),
                FILE_CONTENTS,
            )
            .expect("write an extra fixture file");
        }
        fixture
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(crate) fn file(&self) -> PathBuf {
        self.directory.join("f.t")
    }

    pub(crate) fn open_file(&self) -> File {
        File::open(self.file()).expect("open the fixture file")
    }

    /// Opens the directory the way every audited consumer does.
    pub(crate) fn open_directory(&self) -> File {
        OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(self.directory())
            .expect("open the fixture directory")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
