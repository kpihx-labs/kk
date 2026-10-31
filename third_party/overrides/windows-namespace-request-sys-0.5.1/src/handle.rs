// Copyright (c) Mike Grier.

//! The handle the entries hold.
//!
//! Every entry that takes a handle holds a [`SharedHandle`], and a clone of a request shares it
//! (`D-32` in the crate's `DESIGN-NOTES.md`). Nothing in this crate duplicates a handle.

use std::ffi::c_void;
use std::os::windows::io::AsRawHandle;

use windows_sys::Win32::Foundation::HANDLE;

use win_shared_os_owned_handle::SharedHandle;

/// The raw value of a shared handle, for passing to a Win32 call. Borrowed, not transferred.
pub(crate) trait SharedRaw {
    fn raw(&self) -> HANDLE;
}

impl SharedRaw for SharedHandle {
    fn raw(&self) -> HANDLE {
        self.as_raw_handle().cast::<c_void>()
    }
}

// Visible to the crate's own cross-module tests, which reuse this module's
// fixture rather than standing up a second copy of it.
#[cfg(test)]
pub(crate) mod tests;
