// Copyright (c) Mike Grier.

//! The `GetFileInformationByHandle` entry.
//!
//! Entry 6 of the audited catalogue: the **non-`Ex`** call, returning a
//! `BY_HANDLE_FILE_INFORMATION`.
//!
//! # Why this is not a class of the `Ex` entry
//!
//! It is a distinct Win32 call with its own signature, its own out-parameter,
//! and no class argument at all -- so the one-entry-per-Win32-call rule makes
//! it its own entry. The two overlap in what they report and are not
//! interchangeable: this call yields the link count and a 64-bit file index in
//! one shot, where the `Ex` form's `FileIdInfo` gives a 128-bit id and no link
//! count. The watcher uses this one where the `Ex` form would not do.
//!
//! # A pure read
//!
//! Measured: this call does **not** disturb a directory enumeration in
//! progress, on the handle or on a duplicate of it. It composes freely with
//! [`crate::query`]'s enumeration classes.

use std::mem::MaybeUninit;

use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
};

use win_shared_os_owned_handle::SharedHandle;

use crate::handle::SharedRaw;
use crate::outcome::{Outcome, perform_bool};

/// An owned, marshalable parameter set for `GetFileInformationByHandle`.
///
/// # Example
///
/// ```
/// use std::fs;
///
/// use windows_namespace_request_sys::file_info::QueryFileInformationByHandle;
///
/// let path = std::env::temp_dir().join(format!("wnrs-fi-{}.tmp", std::process::id()));
/// fs::write(&path, b"example")?;
/// let file = fs::File::open(&path)?;
///
/// let information = QueryFileInformationByHandle::new(file).perform()?;
///
/// // The 64-bit file index this call reports in one shot, which the Ex form's
/// // FileIdInfo does not give in this shape.
/// let index = (u64::from(information.nFileIndexHigh) << 32)
///     | u64::from(information.nFileIndexLow);
/// assert_ne!(index, 0);
/// assert_eq!(information.nFileSizeLow, b"example".len() as u32);
/// # fs::remove_file(&path)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug)]
#[must_use = "an unperformed request queries nothing"]
pub struct QueryFileInformationByHandle {
    handle: SharedHandle,
}

impl QueryFileInformationByHandle {
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
    pub fn perform(&self) -> Outcome<BY_HANDLE_FILE_INFORMATION> {
        let mut information = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();

        perform_bool(|| {
            // SAFETY: the handle is one this request holds and keeps
            // open across the call, and the out-parameter points at writable
            // storage of exactly the right size.
            unsafe { GetFileInformationByHandle(self.handle.raw(), information.as_mut_ptr()) }
        })?;

        // SAFETY: a successful call fully initialises the structure.
        Ok(unsafe { information.assume_init() })
    }
}

impl crate::request::Request for QueryFileInformationByHandle {
    type Error = crate::Win32Error;
    type Output = BY_HANDLE_FILE_INFORMATION;

    fn perform(&self) -> Outcome<BY_HANDLE_FILE_INFORMATION> {
        Self::perform(self)
    }
}

#[cfg(test)]
mod tests;
