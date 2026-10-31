// Copyright (c) Mike Grier.

//! The `FileBasicInfo` class of `GetFileInformationByHandleEx`, read as a typed value.
//!
//! [`QueryFileInformation`](crate::query::QueryFileInformation) makes the same call for every class
//! and returns the bytes it wrote. For this class the bytes are a `FILE_BASIC_INFO`, and reading one
//! out of them needs `unsafe`, so a consumer that forbids it cannot. This entry makes the call with a
//! `FILE_BASIC_INFO` as the buffer and returns its five fields as values. It is still one Win32 call:
//! a typed reading of one class, not a second operation, shaped as
//! [`QueryFileIdInfo`](crate::file_id::QueryFileIdInfo) is (`D-33` in the crate's design notes).
//!
//! # What the values are, and what they are not
//!
//! The `FILE_BASIC_INFO` page gives the four times "in FILETIME format, which is a 64-bit value
//! representing the number of 100-nanosecond intervals since January 1, 1601 (UTC)", and the
//! attributes as the file attribute constants. All five are carried as the call wrote them: the
//! times as `i64`, the attributes as their raw `u32`. Nothing here converts a time, names an
//! attribute, or says what either means beyond the page.

use std::mem::MaybeUninit;

use windows_sys::Win32::Storage::FileSystem::{FILE_BASIC_INFO, GetFileInformationByHandleEx};

use win_shared_os_owned_handle::SharedHandle;

use crate::handle::SharedRaw;
use crate::outcome::{Outcome, perform_bool};
use crate::query::FileInformationClass;

/// The four times and the attributes `FileBasicInfo` reports, as the call wrote them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileBasicInfo {
    creation_time: i64,
    last_access_time: i64,
    last_write_time: i64,
    change_time: i64,
    file_attributes: u32,
}

impl FileBasicInfo {
    /// A value with these fields, for a caller that holds them from elsewhere.
    #[must_use]
    pub const fn new(
        creation_time: i64,
        last_access_time: i64,
        last_write_time: i64,
        change_time: i64,
        file_attributes: u32,
    ) -> Self {
        Self {
            creation_time,
            last_access_time,
            last_write_time,
            change_time,
            file_attributes,
        }
    }

    /// `CreationTime`: when the file was created, in `FILETIME` units.
    #[must_use]
    pub const fn creation_time(&self) -> i64 {
        self.creation_time
    }

    /// `LastAccessTime`: when the file was last accessed, in `FILETIME` units.
    #[must_use]
    pub const fn last_access_time(&self) -> i64 {
        self.last_access_time
    }

    /// `LastWriteTime`: when the file was last written to, in `FILETIME` units. The page relates it
    /// to the data stream.
    #[must_use]
    pub const fn last_write_time(&self) -> i64 {
        self.last_write_time
    }

    /// `ChangeTime`: when the file was changed, in `FILETIME` units. The page relates it to the
    /// file's metadata, "such as renames and attribute changes".
    #[must_use]
    pub const fn change_time(&self) -> i64 {
        self.change_time
    }

    /// `FileAttributes`: the file attribute constants, as their raw bits.
    #[must_use]
    pub const fn file_attributes(&self) -> u32 {
        self.file_attributes
    }
}

/// The five fields of what the call wrote. Private, so the dependency's structure is not part of this
/// crate's surface.
fn from_raw(info: &FILE_BASIC_INFO) -> FileBasicInfo {
    FileBasicInfo::new(
        info.CreationTime,
        info.LastAccessTime,
        info.LastWriteTime,
        info.ChangeTime,
        info.FileAttributes,
    )
}

/// An owned, marshalable parameter set for `GetFileInformationByHandleEx` with `FileBasicInfo`.
///
/// # Example
///
/// ```
/// use std::fs;
///
/// use windows_namespace_request_sys::file_basic::QueryFileBasicInfo;
/// use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
///
/// let path = std::env::temp_dir().join(format!("wnrs-fb-{}.tmp", std::process::id()));
/// fs::write(&path, b"example")?;
///
/// let basic = QueryFileBasicInfo::new(fs::File::open(&path)?).perform()?;
///
/// // The attributes are the call's bits; a file is not a directory.
/// assert_eq!(basic.file_attributes() & FILE_ATTRIBUTE_DIRECTORY, 0);
/// # fs::remove_file(&path)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug)]
#[must_use = "an unperformed request queries nothing"]
pub struct QueryFileBasicInfo {
    handle: SharedHandle,
}

impl QueryFileBasicInfo {
    /// Begins a request against `handle`, which the request holds as one more holder of it.
    pub fn new(handle: impl Into<SharedHandle>) -> Self {
        Self {
            handle: handle.into(),
        }
    }

    /// The handle being queried, shared with its other holders.
    pub fn handle(&self) -> &SharedHandle {
        &self.handle
    }

    /// Performs the query on the calling thread.
    ///
    /// # Errors
    ///
    /// Returns the raw Win32 code, unaltered.
    pub fn perform(&self) -> Outcome<FileBasicInfo> {
        let mut information = MaybeUninit::<FILE_BASIC_INFO>::uninit();
        let size = u32::try_from(size_of::<FILE_BASIC_INFO>())
            .expect("this fixed, small struct's size always fits a u32");

        perform_bool(|| {
            // SAFETY: the handle is one this request holds and keeps open across the call, and
            // the buffer is writable `FILE_BASIC_INFO` storage of exactly the size passed, which is
            // the structure this class writes.
            unsafe {
                GetFileInformationByHandleEx(
                    self.handle.raw(),
                    FileInformationClass::BASIC.as_raw(),
                    information.as_mut_ptr().cast(),
                    size,
                )
            }
        })?;

        // SAFETY: a successful call of this class fills the whole structure.
        Ok(from_raw(unsafe { information.assume_init_ref() }))
    }
}

impl crate::request::Request for QueryFileBasicInfo {
    type Error = crate::Win32Error;
    type Output = FileBasicInfo;

    fn perform(&self) -> Outcome<FileBasicInfo> {
        Self::perform(self)
    }
}

#[cfg(test)]
mod tests;
