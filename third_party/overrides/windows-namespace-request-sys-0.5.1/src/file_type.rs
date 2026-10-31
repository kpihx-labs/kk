// Copyright (c) Mike Grier.

//! The `GetFileType` entry.
//!
//! Learn files `GetFileType` under File Management Functions, beside `CreateFile` and
//! `GetFinalPathNameByHandle`, which is why it is here.
//!
//! # What the call says, and what it does not
//!
//! The call's page lists five values and describes them: `FILE_TYPE_DISK` is "a disk file",
//! `FILE_TYPE_CHAR` is "a character file, typically an LPT device or a console", `FILE_TYPE_PIPE` is
//! "a socket, a named pipe, or an anonymous pipe", `FILE_TYPE_REMOTE` is "unused", and
//! `FILE_TYPE_UNKNOWN` is "either the type of the specified file is unknown, or the function failed".
//! The value is carried as the call returned it and is given no further meaning here: what a handle
//! that reports `FILE_TYPE_DISK` is, beyond what the page says, is not stated by this entry. A
//! volume and a physical disk were observed reporting it as a file does.
//!
//! # An unknown type is not always a failure
//!
//! The page says that `GetLastError` tells the two readings of `FILE_TYPE_UNKNOWN` apart: it is
//! `NO_ERROR` after a call that worked and returned an unknown type, and the error code after a call
//! that failed. A stale code from an earlier call would therefore read as a failure, so the last
//! error is cleared immediately before the call. An unknown type with `NO_ERROR` is
//! [`FileType::UNKNOWN`]; an unknown type with any other code is that code, unaltered.

use std::fmt;

use windows_sys::Win32::Foundation::{NO_ERROR, SetLastError};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_TYPE, FILE_TYPE_CHAR, FILE_TYPE_DISK, FILE_TYPE_PIPE, FILE_TYPE_REMOTE, FILE_TYPE_UNKNOWN,
    GetFileType,
};

use win_shared_os_owned_handle::SharedHandle;

use crate::handle::SharedRaw;
use crate::outcome::{Outcome, perform};

/// The value `GetFileType` returned, carried as it came.
///
/// The named constants are the five the call's page lists. A value that is none of them is held and
/// reported as it is, so a value a later system returns is not lost.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileType(FILE_TYPE);

impl FileType {
    /// `FILE_TYPE_DISK`: "The specified file is a disk file."
    pub const DISK: Self = Self(FILE_TYPE_DISK);
    /// `FILE_TYPE_CHAR`: "The specified file is a character file, typically an LPT device or a console."
    pub const CHAR: Self = Self(FILE_TYPE_CHAR);
    /// `FILE_TYPE_PIPE`: "The specified file is a socket, a named pipe, or an anonymous pipe."
    pub const PIPE: Self = Self(FILE_TYPE_PIPE);
    /// `FILE_TYPE_REMOTE`: "Unused."
    pub const REMOTE: Self = Self(FILE_TYPE_REMOTE);
    /// `FILE_TYPE_UNKNOWN` returned by a call that worked: the type of the file is unknown.
    pub const UNKNOWN: Self = Self(FILE_TYPE_UNKNOWN);

    /// The value as `GetFileType` returned it.
    #[must_use]
    pub const fn as_raw(self) -> FILE_TYPE {
        self.0
    }

    /// A value `GetFileType` returned.
    #[must_use]
    pub const fn from_raw(value: FILE_TYPE) -> Self {
        Self(value)
    }
}

impl fmt::Debug for FileType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::DISK => f.write_str("FileType::DISK"),
            Self::CHAR => f.write_str("FileType::CHAR"),
            Self::PIPE => f.write_str("FileType::PIPE"),
            Self::REMOTE => f.write_str("FileType::REMOTE"),
            Self::UNKNOWN => f.write_str("FileType::UNKNOWN"),
            Self(other) => write!(f, "FileType({other:#x})"),
        }
    }
}

/// An owned, marshalable parameter set for `GetFileType`.
///
/// # Example
///
/// ```
/// use std::fs;
///
/// use windows_namespace_request_sys::file_type::{FileType, QueryFileType};
///
/// let path = std::env::temp_dir().join(format!("wnrs-ft-{}.tmp", std::process::id()));
/// fs::write(&path, b"example")?;
/// let file = fs::File::open(&path)?;
///
/// let kind = QueryFileType::new(file).perform()?;
///
/// assert_eq!(kind, FileType::DISK);
/// # fs::remove_file(&path)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug)]
#[must_use = "an unperformed request queries nothing"]
pub struct QueryFileType {
    handle: SharedHandle,
}

impl QueryFileType {
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
    /// Returns the raw Win32 code, unaltered, when the call reported `FILE_TYPE_UNKNOWN` with a code
    /// other than `NO_ERROR`. An unknown type with `NO_ERROR` is [`FileType::UNKNOWN`].
    pub fn perform(&self) -> Outcome<FileType> {
        let returned = perform(
            || {
                // SAFETY: `SetLastError` writes only this thread's error slot.
                unsafe { SetLastError(NO_ERROR) };
                // SAFETY: the handle is one this request holds and keeps open across the call.
                unsafe { GetFileType(self.handle.raw()) }
            },
            |kind| *kind == FILE_TYPE_UNKNOWN,
        );
        unknown_with_no_error_is_a_result(returned)
    }
}

/// An unknown type that the call reported with `NO_ERROR` is a result, and with any other code is
/// that code; every other type is a result.
fn unknown_with_no_error_is_a_result(returned: Outcome<FILE_TYPE>) -> Outcome<FileType> {
    match returned {
        Ok(kind) => Ok(FileType(kind)),
        Err(error) if error.code() == NO_ERROR => Ok(FileType::UNKNOWN),
        Err(error) => Err(error),
    }
}

impl crate::request::Request for QueryFileType {
    type Error = crate::Win32Error;
    type Output = FileType;

    fn perform(&self) -> Outcome<FileType> {
        Self::perform(self)
    }
}

#[cfg(test)]
mod tests;
