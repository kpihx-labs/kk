// Copyright (c) Mike Grier.

//! The `FileIdInfo` class of `GetFileInformationByHandleEx`, read as a typed value.
//!
//! [`QueryFileInformation`](crate::query::QueryFileInformation) makes the same call for every class
//! and returns the bytes it wrote. For this class the bytes are a `FILE_ID_INFO`, and reading one
//! out of them needs `unsafe`, so a consumer that forbids it cannot. This entry makes the call with a
//! `FILE_ID_INFO` as the buffer and returns the two fields as values. It is still one Win32 call:
//! a typed reading of one class, not a second operation (see `D-31` in the crate's design notes).
//!
//! # What the values are, and what they are not
//!
//! The `FILE_ID_INFO` page describes `VolumeSerialNumber` as "the serial number of the volume that
//! contains a file" and `FileId` as "the 128-bit file identifier for the file. The file identifier
//! and the volume serial number uniquely identify a file on a single computer." Both are carried as
//! the call wrote them: the serial as a `u64`, the identifier as its 16 bytes. Nothing here says
//! what either means beyond that, and the identifier is not folded into an integer, which would
//! impose a byte order the call does not state.

use std::mem::MaybeUninit;

use windows_sys::Win32::Storage::FileSystem::{FILE_ID_INFO, GetFileInformationByHandleEx};

use win_shared_os_owned_handle::SharedHandle;

use crate::handle::SharedRaw;
use crate::outcome::{Outcome, perform_bool};
use crate::query::FileInformationClass;

/// The volume serial number and the 128-bit file identifier `FileIdInfo` reports, as the call
/// wrote them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileIdInfo {
    volume_serial_number: u64,
    file_id: [u8; 16],
}

impl FileIdInfo {
    /// A value with these fields, for a caller that holds them from elsewhere.
    #[must_use]
    pub const fn new(volume_serial_number: u64, file_id: [u8; 16]) -> Self {
        Self {
            volume_serial_number,
            file_id,
        }
    }

    /// `VolumeSerialNumber`: the serial number of the volume that contains the file.
    #[must_use]
    pub const fn volume_serial_number(&self) -> u64 {
        self.volume_serial_number
    }

    /// `FileId`: the 128-bit file identifier, as its 16 bytes.
    #[must_use]
    pub const fn file_id(&self) -> [u8; 16] {
        self.file_id
    }
}

/// The two fields of what the call wrote. Private, so the dependency's structure is not part of this
/// crate's surface.
fn from_raw(info: &FILE_ID_INFO) -> FileIdInfo {
    FileIdInfo::new(info.VolumeSerialNumber, info.FileId.Identifier)
}

/// An owned, marshalable parameter set for `GetFileInformationByHandleEx` with `FileIdInfo`.
///
/// # Example
///
/// ```
/// use std::fs;
///
/// use windows_namespace_request_sys::file_id::QueryFileIdInfo;
///
/// let path = std::env::temp_dir().join(format!("wnrs-fid-{}.tmp", std::process::id()));
/// fs::write(&path, b"example")?;
///
/// let first = fs::File::open(&path)?;
/// let second = fs::File::open(&path)?;
/// let one = QueryFileIdInfo::new(first).perform()?;
/// let two = QueryFileIdInfo::new(second).perform()?;
///
/// // Two opens of one file report one identity.
/// assert_eq!(one, two);
/// # fs::remove_file(&path)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug)]
#[must_use = "an unperformed request queries nothing"]
pub struct QueryFileIdInfo {
    handle: SharedHandle,
}

impl QueryFileIdInfo {
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
    pub fn perform(&self) -> Outcome<FileIdInfo> {
        let mut information = MaybeUninit::<FILE_ID_INFO>::uninit();
        let size = u32::try_from(size_of::<FILE_ID_INFO>())
            .expect("this fixed, small struct's size always fits a u32");

        perform_bool(|| {
            // SAFETY: the handle is one this request holds and keeps open across the call, and
            // the buffer is writable `FILE_ID_INFO` storage of exactly the size passed, which is the
            // structure this class writes.
            unsafe {
                GetFileInformationByHandleEx(
                    self.handle.raw(),
                    FileInformationClass::ID.as_raw(),
                    information.as_mut_ptr().cast(),
                    size,
                )
            }
        })?;

        // SAFETY: a successful call of this class fills the whole structure.
        Ok(from_raw(unsafe { information.assume_init_ref() }))
    }
}

impl crate::request::Request for QueryFileIdInfo {
    type Error = crate::Win32Error;
    type Output = FileIdInfo;

    fn perform(&self) -> Outcome<FileIdInfo> {
        Self::perform(self)
    }
}

#[cfg(test)]
mod tests;
