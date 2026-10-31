// Copyright (c) Mike Grier.

//! The `ReOpenFile` entry.
//!
//! One entry for one call (D-3): a **new handle to the same file, directory or
//! volume** a handle already names, with access, share mode and flags that the caller states anew.
//! It is a third open primitive beside [`crate::open::OpenFile`] and
//! [`crate::open_by_id::OpenFileByIdentifier`], and unlike them it names nothing by path or
//! identifier: it can only reach what an open handle already reaches, which is what makes it the
//! way to *change how* a file is open, such as adding `FILE_FLAG_NO_BUFFERING` to a handle that was
//! opened without it, without the race a second open by path would run.
//!
//! # The original is an input handle, and is shared
//!
//! As with the volume hint of [`crate::open_by_id::OpenFileByIdentifier`], the request holds a share
//! of the handle it reopens, so the request cannot be left naming a handle its originator has
//! closed. It is the original's own handle, so what the call sees of it is the open the original
//! belongs to.
//!
//! # Nothing is defaulted
//!
//! The access, the share mode and the flags all start at zero, "the caller said nothing", and are
//! carried verbatim (D-12). What a combination means, and which of the flags
//! `ReOpenFile` accepts, is the call's to say, and a combination it refuses reaches the caller as its
//! own code. In particular an access or sharing that conflicts with an open still open is the
//! kernel's sharing violation, and nothing here infers anything from it.
//!
//! # Directories
//!
//! Every reopen of a directory handle that was tried on the machine this was written on, whatever
//! the access, share mode and flags, came back as `ERROR_ACCESS_DENIED`. Why is not established,
//! and the entry does not special-case it: the code is the call's.

use std::ffi::c_void;
use std::os::windows::io::{FromRawHandle, OwnedHandle};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_MODE, ReOpenFile as Win32ReOpenFile,
};

use win_shared_os_owned_handle::SharedHandle;

use crate::handle::SharedRaw;
use crate::outcome::{Outcome, perform_handle};

/// An owned, marshalable parameter set for `ReOpenFile`.
///
/// # Example
///
/// A read-only handle to a file that was opened for writing, the same file through a new handle that
/// says what it wants anew:
///
/// ```
/// use std::fs;
///
/// use windows_namespace_request_sys::reopen::ReOpenFile;
/// use windows_sys::Win32::Storage::FileSystem::{
///     FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
/// };
///
/// let path = std::env::temp_dir().join(format!("wnrs-reopen-{}.tmp", std::process::id()));
/// fs::write(&path, b"example")?;
/// let original = fs::OpenOptions::new().read(true).write(true).open(&path)?;
///
/// let request = ReOpenFile::new(original)
///     .with_desired_access(FILE_GENERIC_READ)
///     .with_share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
/// let reopened = fs::File::from(request.perform()?);
///
/// assert_eq!(reopened.metadata()?.len(), b"example".len() as u64);
/// # drop(reopened);
/// # fs::remove_file(&path)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug)]
#[must_use = "an unperformed request reopens nothing"]
pub struct ReOpenFile {
    original: SharedHandle,
    desired_access: u32,
    share_mode: FILE_SHARE_MODE,
    flags_and_attributes: FILE_FLAGS_AND_ATTRIBUTES,
}

impl ReOpenFile {
    /// Begins a request to reopen the file, directory or volume `original` names.
    ///
    /// The request holds `original` as one more holder of it. The remaining parameters start at
    /// "the caller said nothing" and are set explicitly.
    pub fn new(original: impl Into<SharedHandle>) -> Self {
        Self {
            original: original.into(),
            desired_access: 0,
            share_mode: 0,
            flags_and_attributes: 0,
        }
    }

    /// Sets `dwDesiredAccess`.
    pub fn with_desired_access(mut self, desired_access: u32) -> Self {
        self.desired_access = desired_access;
        self
    }

    /// Sets `dwShareMode`.
    pub fn with_share_mode(mut self, share_mode: FILE_SHARE_MODE) -> Self {
        self.share_mode = share_mode;
        self
    }

    /// Sets `dwFlagsAndAttributes`, carried verbatim as in [`crate::open::OpenFile`].
    pub fn with_flags_and_attributes(
        mut self,
        flags_and_attributes: FILE_FLAGS_AND_ATTRIBUTES,
    ) -> Self {
        self.flags_and_attributes = flags_and_attributes;
        self
    }

    /// The handle being reopened, shared with its other holders.
    pub fn original(&self) -> &SharedHandle {
        &self.original
    }

    /// The requested access mask.
    #[must_use]
    pub fn desired_access(&self) -> u32 {
        self.desired_access
    }

    /// The requested share mode.
    #[must_use]
    pub fn share_mode(&self) -> FILE_SHARE_MODE {
        self.share_mode
    }

    /// The requested flags and attributes.
    #[must_use]
    pub fn flags_and_attributes(&self) -> FILE_FLAGS_AND_ATTRIBUTES {
        self.flags_and_attributes
    }

    /// Performs the reopen on the calling thread.
    ///
    /// The handle comes back owned and is a new handle each time, however often the request is
    /// performed.
    ///
    /// # Errors
    ///
    /// Returns the raw Win32 code, unaltered. An access or sharing that conflicts with a previous
    /// open still open is a sharing violation, which is the kernel's error and passes through.
    pub fn perform(&self) -> Outcome<OwnedHandle> {
        let raw = perform_handle(|| {
            // SAFETY: the original is one this request holds and keeps open across the
            // call; the other three arguments are plain values.
            unsafe {
                Win32ReOpenFile(
                    self.original.raw(),
                    self.desired_access,
                    self.share_mode,
                    self.flags_and_attributes,
                )
            }
        })?;

        // SAFETY: a successful ReOpenFile returns a handle this process owns exclusively and must
        // release with CloseHandle, which OwnedHandle does.
        Ok(unsafe { OwnedHandle::from_raw_handle(raw.cast::<c_void>()) })
    }
}

impl crate::request::Request for ReOpenFile {
    type Error = crate::Win32Error;
    type Output = OwnedHandle;

    fn perform(&self) -> Outcome<OwnedHandle> {
        Self::perform(self)
    }
}

#[cfg(test)]
mod tests;
