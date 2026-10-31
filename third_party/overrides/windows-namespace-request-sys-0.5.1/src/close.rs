// Copyright (c) Mike Grier.

//! The close entries.
//!
//! Entry 4 of the audited catalogue, and the one whose membership surprises
//! people.
//!
//! # Why closing is a catalogue entry at all
//!
//! `CloseHandle` looks like bookkeeping, but it is a blocking namespace call.
//! It waits for outstanding I/O on the handle to complete, and on a dead
//! network path or an ejected removable device it can block hard -- which is
//! the whole reason this facility exists. A consumer that carefully moved its
//! opens onto a worker and then closed on its own thread would have moved the
//! wrong half.
//!
//! # A handle carries its close routine
//!
//! The audit found that a close entry **cannot assume its routine**:
//! `FindCloseChangeNotification` closes an
//! [`crate::watch::ChangeNotification`] and `CloseHandle` is wrong for it,
//! silently. So the routine travels with the handle rather than being chosen at
//! the call site, which is the same shape
//! [windows-threadpool-sys](https://docs.rs/windows-threadpool-sys) already
//! needed for wait targets.
//!
//! # A request is consumed by performing it
//!
//! [`CloseRequest::perform`] takes `self`, so a handle cannot be closed twice
//! through this type. An unperformed request still closes its handle when
//! dropped, because the alternative is a leak: a request that quietly did
//! nothing would be worse than one that closes late.

use std::ffi::c_void;
use std::fmt;
use std::mem::ManuallyDrop;
use std::os::windows::io::{AsRawHandle, OwnedHandle};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Storage::FileSystem::FindCloseChangeNotification;
use windows_sys::core::BOOL;

use crate::outcome::{Outcome, perform_bool};
use crate::watch::ChangeNotification;

/// A Win32 routine that closes a handle.
///
/// This is the shape Win32 close routines already have, so one can be passed
/// directly with no shim: `CloseHandle` and `FindCloseChangeNotification` both
/// match it.
pub type CloseFn = unsafe extern "system" fn(HANDLE) -> BOOL;

/// An owned, marshalable request to close one handle.
///
/// The request owns the handle it will close, so the handle cannot be closed by
/// anyone else in the meantime, and cannot outlive the request unclosed.
///
/// # Example
///
/// ```
/// use std::fs;
///
/// use windows_namespace_request_sys::close::CloseRequest;
///
/// let path = std::env::temp_dir().join(format!("wnrs-close-{}.tmp", std::process::id()));
/// fs::write(&path, b"example")?;
/// let file = fs::File::open(&path)?;
///
/// // The close is a value now, so it can be performed wherever blocking is
/// // acceptable rather than wherever the handle happens to be dropped.
/// let request = CloseRequest::for_handle(file.into());
/// request.perform()?;
/// # fs::remove_file(&path)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Example: the routine travels with the handle
///
/// A change notification is closed with `FindCloseChangeNotification`, and
/// `CloseHandle` is silently wrong for it. A caller never has to know that,
/// because the constructor pairs them:
///
/// ```
/// use std::fs;
///
/// use windows_namespace_request_sys::close::CloseRequest;
/// use windows_namespace_request_sys::watch::{NotifyFilter, WatchDirectory};
/// use wtf_string::Wtf16String;
///
/// let directory = std::env::temp_dir().join(format!("wnrs-cr-{}", std::process::id()));
/// let _ = fs::remove_dir_all(&directory);
/// fs::create_dir_all(&directory)?;
/// let text = directory.to_str().expect("a temporary path is valid UTF-8");
///
/// let notification = WatchDirectory::new(Wtf16String::from(text))?
///     .with_filter(NotifyFilter::FILE_NAME)
///     .perform()?;
///
/// let request = CloseRequest::for_change_notification(notification);
/// assert!(format!("{request:?}").contains("FindCloseChangeNotification"));
/// request.perform()?;
/// # let _ = fs::remove_dir_all(&directory);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Example: performing consumes the request
///
/// This is what makes closing twice through this type impossible -- the second
/// call does not compile:
///
/// ```compile_fail,E0382
/// use std::fs;
///
/// use windows_namespace_request_sys::close::CloseRequest;
///
/// let path = std::env::temp_dir().join("wnrs-doc-double-close.tmp");
/// fs::write(&path, b"x").unwrap();
/// let request = CloseRequest::for_handle(fs::File::open(&path).unwrap().into());
///
/// request.perform().unwrap();
/// request.perform().unwrap(); // error: use of moved value
/// ```
#[must_use = "dropping the request closes the handle immediately, on this thread"]
pub struct CloseRequest {
    /// Live until either `perform` or `Drop` closes it, exactly once.
    handle: HANDLE,
    close: CloseFn,
}

impl CloseRequest {
    /// A request to close an ordinary handle with `CloseHandle`.
    pub fn for_handle(handle: OwnedHandle) -> Self {
        // The handle must not be closed by OwnedHandle's own drop, because this
        // request now owns it.
        let handle = ManuallyDrop::new(handle);

        Self {
            handle: handle.as_raw_handle().cast::<c_void>(),
            close: CloseHandle,
        }
    }

    /// A request to close a change notification with
    /// `FindCloseChangeNotification`.
    ///
    /// Provided by name so a caller never has to know which routine is right:
    /// this is precisely the pairing the audit found a close entry cannot
    /// assume.
    pub fn for_change_notification(notification: ChangeNotification) -> Self {
        let raw = notification.as_raw();
        // As above: this request takes over the close.
        let _ = ManuallyDrop::new(notification);

        Self {
            handle: raw,
            close: FindCloseChangeNotification,
        }
    }

    /// A request to close `handle` with a caller-supplied routine.
    ///
    /// The escape hatch for a handle whose close routine this crate does not
    /// know about, so an unanticipated variant needs no change here.
    ///
    /// # Safety
    ///
    /// `handle` must be a live handle that the caller owns and gives up, and
    /// `close` must be the correct routine for it.
    pub unsafe fn from_raw(handle: HANDLE, close: CloseFn) -> Self {
        Self { handle, close }
    }

    /// The handle this request will close.
    #[must_use]
    pub fn handle(&self) -> HANDLE {
        self.handle
    }

    /// Performs the close on the calling thread.
    ///
    /// Consumes the request, so a handle cannot be closed twice through this
    /// type.
    ///
    /// # Errors
    ///
    /// Returns the raw Win32 code, unaltered. The handle is closed either way:
    /// a failed close is not a close that can be retried.
    pub fn perform(self) -> Outcome<()> {
        // Drop must not run: it would close the handle a second time.
        let request = ManuallyDrop::new(self);

        // SAFETY: the handle is live and owned by this request, which is being
        // consumed, and `close` is its correct routine by construction.
        perform_bool(|| unsafe { (request.close)(request.handle) })
    }
}

impl Drop for CloseRequest {
    fn drop(&mut self) {
        // SAFETY: the handle is live and owned here, and this runs exactly once
        // -- `perform` consumes the request through a `ManuallyDrop`. The
        // result is ignored because a destructor has nowhere to report it.
        unsafe {
            (self.close)(self.handle);
        }
    }
}

impl fmt::Debug for CloseRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CloseRequest")
            .field("handle", &self.handle)
            .field("close", &self.close_routine_name())
            .finish()
    }
}

impl CloseRequest {
    /// Names the close routine, for diagnostics.
    ///
    /// Comparing function pointers is not generally meaningful, but these two
    /// are the ones this crate installs, so recognising them is worth the
    /// caveat that an unrecognised routine simply reports as custom.
    fn close_routine_name(&self) -> &'static str {
        let close = self.close as *const ();

        if close == CloseHandle as *const () {
            "CloseHandle"
        } else if close == FindCloseChangeNotification as *const () {
            "FindCloseChangeNotification"
        } else {
            "custom"
        }
    }
}

// SAFETY: the request owns its handle exclusively and has no interior
// mutability. A Windows handle is process-wide rather than thread-affine, so a
// close performed on another thread closes the same object; the raw pointer is
// what blocks the automatic derivation.
unsafe impl Send for CloseRequest {}
// SAFETY: as above. Every method that could close the handle takes `self`.
unsafe impl Sync for CloseRequest {}

impl crate::request::ConsumingRequest for CloseRequest {
    type Error = crate::Win32Error;
    type Output = ();

    fn perform(self) -> Outcome<()> {
        Self::perform(self)
    }
}

#[cfg(test)]
mod tests;
