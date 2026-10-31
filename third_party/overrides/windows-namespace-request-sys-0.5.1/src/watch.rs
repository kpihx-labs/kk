// Copyright (c) Mike Grier.

//! The `FindFirstChangeNotificationW` entry.
//!
//! Entry 3 of the audited catalogue. It watches a directory for a class of
//! changes and produces a handle that becomes signalled when one occurs.
//!
//! # The handle it produces is not an ordinary handle
//!
//! A change-notification handle is closed with `FindCloseChangeNotification`,
//! **not** `CloseHandle`. Passing it to `CloseHandle` is a resource leak that
//! nothing reports: the call may even appear to succeed. That is why this entry
//! returns [`ChangeNotification`] rather than a bare `OwnedHandle` -- the type
//! is what remembers which routine closes it, so a caller cannot forget.
//!
//! # What it does and does not tell you
//!
//! The handle signals that *something* in the watched set changed. It carries
//! no record of what, and a burst of changes may signal once. A consumer
//! needing the individual changes wants `ReadDirectoryChangesW`, which is a
//! different call and therefore a different entry. This one is the cheap
//! "something happened, go look" primitive, and the crate does not blur them.

use std::ffi::c_void;
use std::fmt;

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_NOTIFY_CHANGE, FILE_NOTIFY_CHANGE_ATTRIBUTES, FILE_NOTIFY_CHANGE_CREATION,
    FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_ACCESS,
    FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SECURITY, FILE_NOTIFY_CHANGE_SIZE,
    FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
};

use wtf_string::{Wtf16Str, Wtf16String};

use crate::anchored::{AnchoredPath, PathCaptureError};
use crate::outcome::{Outcome, perform_bool, perform_handle};

/// An owned change-notification handle.
///
/// Closed with `FindCloseChangeNotification` on drop. The type exists so that
/// the close routine travels with the handle rather than being something each
/// call site has to remember, which is the same shape
/// [windows-threadpool-sys](https://docs.rs/windows-threadpool-sys) already
/// needed for wait targets.
#[derive(Debug)]
#[must_use = "dropping the notification stops the watch"]
pub struct ChangeNotification {
    /// Always a live handle produced by `FindFirstChangeNotificationW`, until
    /// `Drop` closes it.
    handle: HANDLE,
}

impl ChangeNotification {
    /// The raw handle, for passing to a wait.
    ///
    /// Borrowed, not transferred: it stays owned by this value. Do **not** pass
    /// it to `CloseHandle`.
    #[must_use]
    pub fn as_raw(&self) -> HANDLE {
        self.handle
    }

    /// Rearms the watch after it has signalled.
    ///
    /// A notification handle signals once and then stays signalled until it is
    /// rearmed, so a consumer that waits in a loop must call this between
    /// waits.
    ///
    /// # Errors
    ///
    /// Returns the raw Win32 code, unaltered.
    ///
    /// # Example
    ///
    /// Without the rearm, the second wait returns immediately on the *first*
    /// change forever, and a consumer's loop spins:
    ///
    /// ```
    /// use std::fs;
    ///
    /// use windows_namespace_request_sys::watch::{NotifyFilter, WatchDirectory};
    /// use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    /// use windows_sys::Win32::System::Threading::WaitForSingleObject;
    /// use wtf_string::Wtf16String;
    ///
    /// let directory = std::env::temp_dir().join(format!("wnrs-rearm-{}", std::process::id()));
    /// let _ = fs::remove_dir_all(&directory);
    /// fs::create_dir_all(&directory)?;
    ///
    /// let text = directory.to_str().expect("a temporary path is valid UTF-8");
    /// let notification = WatchDirectory::new(Wtf16String::from(text))?
    ///     .with_filter(NotifyFilter::FILE_NAME)
    ///     .perform()?;
    ///
    /// fs::write(directory.join("first.t"), b"x")?;
    /// // SAFETY: the handle is live for the notification's lifetime.
    /// assert_eq!(unsafe { WaitForSingleObject(notification.as_raw(), 5_000) }, WAIT_OBJECT_0);
    ///
    /// // The handle stays signalled until this runs.
    /// notification.rearm()?;
    ///
    /// fs::write(directory.join("second.t"), b"x")?;
    /// // SAFETY: as above.
    /// assert_eq!(unsafe { WaitForSingleObject(notification.as_raw(), 5_000) }, WAIT_OBJECT_0);
    /// # drop(notification);
    /// # let _ = fs::remove_dir_all(&directory);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn rearm(&self) -> Outcome<()> {
        // SAFETY: the handle is live for this value's lifetime.
        perform_bool(|| unsafe { FindNextChangeNotification(self.handle) })
    }
}

impl Drop for ChangeNotification {
    fn drop(&mut self) {
        // SAFETY: the handle came from FindFirstChangeNotificationW and has not
        // been closed, since only this Drop closes it. The result is
        // deliberately ignored: a close failure during drop has nowhere to go,
        // and this crate does not panic on it.
        unsafe {
            FindCloseChangeNotification(self.handle);
        }
    }
}

// SAFETY: the value owns its handle exclusively and has no interior
// mutability. A Windows handle is process-wide rather than thread-affine, so
// moving it between threads and sharing a shared reference are both sound; the
// raw pointer is what blocks the automatic derivation.
unsafe impl Send for ChangeNotification {}
// SAFETY: as above. `rearm` takes `&self` and is a single Win32 call that
// Windows serialises internally.
unsafe impl Sync for ChangeNotification {}

/// Which changes a watch reports.
///
/// A thin newtype over `FILE_NOTIFY_CHANGE` rather than an enum, because the
/// value is a bitmask and Windows may define bits this crate has not heard of.
/// The named constants are provided for convenience; an unknown bit still
/// reaches Windows unaltered.
///
/// # Example
///
/// ```
/// use windows_namespace_request_sys::watch::NotifyFilter;
///
/// let combined = NotifyFilter::FILE_NAME | NotifyFilter::DIR_NAME;
///
/// assert!(combined.contains(NotifyFilter::FILE_NAME));
/// assert!(!combined.contains(NotifyFilter::SIZE));
///
/// // A bitmask, not an enum: a bit Windows defines and this crate has never
/// // heard of still reaches it.
/// let unknown = NotifyFilter::from_bits(0x8000_0000);
/// assert_eq!(unknown.bits(), 0x8000_0000);
///
/// // The empty filter watches nothing, and Windows -- not this crate --
/// // rejects it at the call.
/// assert_eq!(NotifyFilter::NONE.bits(), 0);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NotifyFilter(FILE_NOTIFY_CHANGE);

impl NotifyFilter {
    /// An empty filter, which watches for nothing.
    ///
    /// Windows rejects this at the call, which is left to happen rather than
    /// pre-empted here.
    pub const NONE: Self = Self(0);
    /// File name creation, deletion, or rename.
    pub const FILE_NAME: Self = Self(FILE_NOTIFY_CHANGE_FILE_NAME);
    /// Directory creation or deletion.
    pub const DIR_NAME: Self = Self(FILE_NOTIFY_CHANGE_DIR_NAME);
    /// Attribute changes.
    pub const ATTRIBUTES: Self = Self(FILE_NOTIFY_CHANGE_ATTRIBUTES);
    /// Size changes, reported when the file is flushed rather than on write.
    pub const SIZE: Self = Self(FILE_NOTIFY_CHANGE_SIZE);
    /// Last-write-time changes, likewise reported on flush.
    pub const LAST_WRITE: Self = Self(FILE_NOTIFY_CHANGE_LAST_WRITE);
    /// Last-access-time changes.
    pub const LAST_ACCESS: Self = Self(FILE_NOTIFY_CHANGE_LAST_ACCESS);
    /// Creation-time changes.
    pub const CREATION: Self = Self(FILE_NOTIFY_CHANGE_CREATION);
    /// Security-descriptor changes.
    pub const SECURITY: Self = Self(FILE_NOTIFY_CHANGE_SECURITY);

    /// Wraps a raw `FILE_NOTIFY_CHANGE` mask.
    ///
    /// Any bit pattern is accepted: the crate does not decide which bits
    /// Windows understands.
    #[must_use]
    pub const fn from_bits(bits: FILE_NOTIFY_CHANGE) -> Self {
        Self(bits)
    }

    /// The raw mask.
    #[must_use]
    pub const fn bits(self) -> FILE_NOTIFY_CHANGE {
        self.0
    }

    /// The union of two filters.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every bit of `other` is set in this filter.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for NotifyFilter {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
}

/// An owned, marshalable parameter set for `FindFirstChangeNotificationW`.
///
/// # Example
///
/// ```
/// use windows_namespace_request_sys::watch::{NotifyFilter, WatchDirectory};
/// use wtf_string::Wtf16String;
///
/// let directory = std::env::temp_dir();
/// let text = directory.to_str().expect("the temporary directory is valid UTF-8");
///
/// let request = WatchDirectory::new(Wtf16String::from(text))?
///     .with_subtree(true)
///     .with_filter(NotifyFilter::FILE_NAME | NotifyFilter::DIR_NAME);
///
/// let notification = request.perform()?;
/// // The handle is closed with FindCloseChangeNotification, which the type
/// // remembers on the caller's behalf.
/// drop(notification);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug)]
#[must_use = "an unperformed request watches nothing"]
pub struct WatchDirectory {
    path: AnchoredPath,
    subtree: bool,
    filter: NotifyFilter,
}

impl WatchDirectory {
    /// Begins a request to watch `path`.
    ///
    /// The path reaches the call as it was given, except that one Win32 would resolve against the
    /// current directory is fixed to the current directory **as it is now**, so that it still names
    /// the same place when the request is performed on another thread after the directory has
    /// changed; see [`OpenFile::new`](crate::open::OpenFile::new) and `D-19` in the crate's
    /// `DESIGN-NOTES.md`. [`path`](Self::path) is the path as given and
    /// [`win32_path`](Self::win32_path) is the one handed to the call.
    ///
    /// The watch starts non-recursive and watching for nothing; both are set
    /// explicitly, as everywhere else in this crate.
    ///
    /// # Errors
    ///
    /// [`PathCaptureError::InteriorNul`] if the path contains a NUL, which Win32 would read as its
    /// end, and the failure of `GetCurrentDirectoryW` if it had to be read and could not be.
    pub fn new(path: Wtf16String) -> Result<Self, PathCaptureError> {
        Ok(Self::from_anchored(AnchoredPath::new(path)?))
    }

    /// Begins a request to watch a path the caller has already anchored.
    ///
    /// As [`OpenFile::from_anchored`](crate::open::OpenFile::from_anchored): the path is not anchored
    /// again, and the request hands the call the anchored path's [`win32`](AnchoredPath::win32) form.
    /// The watch starts as [`new`](Self::new) starts it.
    ///
    /// # Example
    ///
    /// ```
    /// use windows_namespace_request_sys::{AnchoredPath, WatchDirectory};
    /// use wtf_string::Wtf16String;
    ///
    /// let anchored = AnchoredPath::new(Wtf16String::from("a-child"))?;
    ///
    /// let request = WatchDirectory::from_anchored(anchored.clone());
    /// assert_eq!(request.path(), anchored.given());
    /// assert_eq!(request.win32_path(), anchored.win32());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn from_anchored(path: AnchoredPath) -> Self {
        Self {
            path,
            subtree: false,
            filter: NotifyFilter::NONE,
        }
    }

    /// Sets `bWatchSubtree`.
    pub fn with_subtree(mut self, subtree: bool) -> Self {
        self.subtree = subtree;
        self
    }

    /// Sets `dwNotifyFilter`.
    pub fn with_filter(mut self, filter: NotifyFilter) -> Self {
        self.filter = filter;
        self
    }

    /// The path as it was given.
    #[must_use]
    pub fn path(&self) -> &Wtf16Str {
        self.path.given()
    }

    /// The path that is handed to `FindFirstChangeNotificationW`: the one given, or that one fixed
    /// to the current directory of the moment the request was built.
    #[must_use]
    pub fn win32_path(&self) -> &Wtf16Str {
        self.path.win32()
    }

    /// Whether the watch covers the whole subtree.
    #[must_use]
    pub fn subtree(&self) -> bool {
        self.subtree
    }

    /// The change classes the watch reports.
    #[must_use]
    pub fn filter(&self) -> NotifyFilter {
        self.filter
    }

    /// Performs the call on the calling thread.
    ///
    /// # Errors
    ///
    /// Returns the raw Win32 code, unaltered.
    pub fn perform(&self) -> Outcome<ChangeNotification> {
        let raw = perform_handle(|| {
            // SAFETY: the path has no interior NUL, since the request refused one when it was
            // built, is NUL-terminated and outlives the call, and both remaining arguments are
            // plain values.
            unsafe {
                FindFirstChangeNotificationW(
                    self.path.win32_terminated(),
                    i32::from(self.subtree),
                    self.filter.bits(),
                )
            }
        })?;

        Ok(ChangeNotification {
            handle: raw.cast::<c_void>(),
        })
    }
}

impl fmt::Display for NotifyFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FILE_NOTIFY_CHANGE({:#x})", self.0)
    }
}

impl crate::request::Request for WatchDirectory {
    type Error = crate::Win32Error;
    type Output = ChangeNotification;

    fn perform(&self) -> Outcome<ChangeNotification> {
        Self::perform(self)
    }
}

#[cfg(test)]
mod tests;
