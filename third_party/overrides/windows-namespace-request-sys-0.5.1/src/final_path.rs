// Copyright (c) Mike Grier.

//! The `GetFinalPathNameByHandleW` entry.
//!
//! Entry 7 of the audited catalogue, and the one with the **strongest offload
//! evidence**. Globazog reaches it through `std::fs::canonicalize`, which it
//! performs on its *submitting* thread once per root -- a full `CreateFileW`
//! plus this call plus `CloseHandle`, with unbounded latency on a network path
//! -- and repeats per reparse-point candidate on a worker. It is first-class
//! here, not second-tier.
//!
//! # The call reports a length, and the caller grows the buffer
//!
//! Unlike [`crate::query`], this call *does* report a length -- but with a
//! twist that is easy to get wrong. On success it returns the number of
//! characters written, **excluding** the terminating NUL. When the buffer is
//! too small it returns the size required **including** the NUL, and does not
//! set a failure code the caller would recognise as "try again bigger". The
//! two returns are distinguished by comparing against the buffer size, and this
//! entry does that retry itself rather than handing a caller a raw length it
//! must interpret.
//!
//! The retry is bounded. A path cannot grow without limit between attempts, so
//! an unbounded loop could only spin on a pathological or hostile filesystem;
//! [`FinalPathError::Unstable`] reports that rather than hanging a worker.

use std::fmt;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_NAME_NORMALIZED, FILE_NAME_OPENED, GETFINALPATHNAMEBYHANDLE_FLAGS,
    GetFinalPathNameByHandleW, VOLUME_NAME_DOS, VOLUME_NAME_GUID, VOLUME_NAME_NONE, VOLUME_NAME_NT,
};
use wtf_string::Wtf16String;

use win_shared_os_owned_handle::SharedHandle;

use crate::handle::SharedRaw;
use crate::outcome::{Win32Error, perform_nonzero};

/// How many times the buffer is grown before the result is called unstable.
///
/// One retry is the expected path: the first attempt learns the size and the
/// second uses it. More than this means the answer changed under us repeatedly.
const MAX_ATTEMPTS: usize = 8;

/// The buffer size the first attempt uses, in characters.
///
/// `MAX_PATH`, which is enough for the overwhelming majority of paths, so the
/// common case costs one call rather than two.
const FIRST_ATTEMPT_CHARS: usize = 260;

/// Which form of the final path to report.
///
/// A newtype over the flags rather than an enum, because the value combines a
/// volume-name choice with a path-form choice and Windows may define more.
///
/// # Example
///
/// ```
/// use windows_namespace_request_sys::final_path::FinalPathFlags;
///
/// // What the watcher uses, and the default here.
/// let default = FinalPathFlags::DEFAULT;
/// assert_eq!(default, FinalPathFlags::VOLUME_NAME_DOS | FinalPathFlags::NAME_NORMALIZED);
///
/// // VOLUME_NAME_DOS and FILE_NAME_NORMALIZED are both zero: they are the
/// // defaults Windows applies when no opposing bit is set, not flags that can
/// // be observed as present.
/// assert_eq!(default.bits(), 0);
///
/// // The alternatives are the ones that carry bits.
/// assert_ne!(FinalPathFlags::VOLUME_NAME_GUID.bits(), 0);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FinalPathFlags(GETFINALPATHNAMEBYHANDLE_FLAGS);

impl FinalPathFlags {
    /// A drive letter, if the volume has one. Windows' default.
    pub const VOLUME_NAME_DOS: Self = Self(VOLUME_NAME_DOS);
    /// A volume GUID path, which has no dependence on drive-letter mappings.
    pub const VOLUME_NAME_GUID: Self = Self(VOLUME_NAME_GUID);
    /// The NT device path.
    pub const VOLUME_NAME_NT: Self = Self(VOLUME_NAME_NT);
    /// No volume component at all.
    pub const VOLUME_NAME_NONE: Self = Self(VOLUME_NAME_NONE);
    /// The normalised form of the path. Windows' default.
    pub const NAME_NORMALIZED: Self = Self(FILE_NAME_NORMALIZED);
    /// The path as it was opened, which may not be normalised.
    pub const NAME_OPENED: Self = Self(FILE_NAME_OPENED);

    /// `VOLUME_NAME_DOS | FILE_NAME_NORMALIZED`, which is what the audited
    /// watcher relies on.
    pub const DEFAULT: Self = Self(VOLUME_NAME_DOS | FILE_NAME_NORMALIZED);

    /// Wraps a raw flags value.
    #[must_use]
    pub const fn from_bits(bits: GETFINALPATHNAMEBYHANDLE_FLAGS) -> Self {
        Self(bits)
    }

    /// The raw flags value.
    #[must_use]
    pub const fn bits(self) -> GETFINALPATHNAMEBYHANDLE_FLAGS {
        self.0
    }
}

impl std::ops::BitOr for FinalPathFlags {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Why a final path could not be resolved.
#[derive(Debug)]
#[non_exhaustive]
pub enum FinalPathError {
    /// Windows refused the call, with the raw code unaltered.
    Win32(Win32Error),
    /// The required size kept changing, so the retry was abandoned.
    ///
    /// A path does not normally grow between two calls a microsecond apart, so
    /// this means something pathological rather than a transient. It is
    /// reported rather than looped on, because spinning here would hang the
    /// worker that a consumer moved this call onto in the first place.
    Unstable {
        /// How many attempts were made before giving up.
        attempts: usize,
    },
}

impl fmt::Display for FinalPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Win32(error) => write!(f, "GetFinalPathNameByHandleW: {error}"),
            Self::Unstable { attempts } => write!(
                f,
                "GetFinalPathNameByHandleW: the required size changed on each of {attempts} attempts"
            ),
        }
    }
}

impl std::error::Error for FinalPathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Win32(error) => Some(error),
            Self::Unstable { .. } => None,
        }
    }
}

impl From<Win32Error> for FinalPathError {
    fn from(error: Win32Error) -> Self {
        Self::Win32(error)
    }
}

/// An owned, marshalable parameter set for `GetFinalPathNameByHandleW`.
///
/// # Example
///
/// ```
/// use std::fs;
///
/// use windows_namespace_request_sys::final_path::QueryFinalPath;
///
/// let path = std::env::temp_dir().join(format!("wnrs-fp-{}.tmp", std::process::id()));
/// fs::write(&path, b"example")?;
/// let file = fs::File::open(&path)?;
///
/// // Built here; it would resolve identically on a worker, which is the point:
/// // Globazog performs this on its submitting thread today.
/// let resolved = QueryFinalPath::new(file)
///     .perform()?
///     .to_string_lossy();
///
/// // The result is a verbatim path, so it names the object unambiguously.
/// assert!(resolved.starts_with(r"\\?\"), "unexpected: {resolved}");
/// assert!(resolved.ends_with(".tmp"), "unexpected: {resolved}");
/// # fs::remove_file(&path)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug)]
#[must_use = "an unperformed request resolves nothing"]
pub struct QueryFinalPath {
    handle: SharedHandle,
    flags: FinalPathFlags,
}

impl QueryFinalPath {
    /// Begins a request against `handle`, in the form the audited watcher uses.
    pub fn new(handle: impl Into<SharedHandle>) -> Self {
        Self {
            handle: handle.into(),
            flags: FinalPathFlags::DEFAULT,
        }
    }

    /// Sets `dwFlags`.
    pub fn with_flags(mut self, flags: FinalPathFlags) -> Self {
        self.flags = flags;
        self
    }

    /// The handle being resolved, shared with its other holders.
    pub fn handle(&self) -> &SharedHandle {
        &self.handle
    }

    /// The flags the call will use.
    #[must_use]
    pub fn flags(&self) -> FinalPathFlags {
        self.flags
    }

    /// Performs the call on the calling thread, growing the buffer as needed.
    ///
    /// # Errors
    ///
    /// Returns [`FinalPathError::Win32`] with the raw code unaltered, or
    /// [`FinalPathError::Unstable`] if the required size kept changing.
    pub fn perform(&self) -> Result<Wtf16String, FinalPathError> {
        let mut capacity = FIRST_ATTEMPT_CHARS;

        for _ in 0..MAX_ATTEMPTS {
            let mut buffer = Wtf16String::with_capacity(capacity);
            let requested = u32::try_from(capacity).unwrap_or(u32::MAX);

            let written = perform_nonzero(|| {
                // SAFETY: the handle is one this request holds and keeps
                // open across the call, and the buffer is writable for
                // `requested` characters. The buffer's invariant is restored
                // below before it is observed.
                unsafe {
                    GetFinalPathNameByHandleW(
                        self.handle.raw(),
                        buffer.as_mut_ptr(),
                        requested,
                        self.flags.bits(),
                    )
                }
            })?;

            let written = written as usize;
            if written < capacity {
                // Success: `written` excludes the terminator, and Windows wrote
                // that many characters plus one.
                // SAFETY: exactly `written` content characters were written,
                // within the requested capacity.
                unsafe { buffer.set_len_from_ffi(written) };
                return Ok(buffer);
            }

            // The buffer was too small, and `written` is the size *including*
            // the terminator. Nothing usable was written, so the buffer is
            // dropped rather than observed.
            capacity = written;
        }

        Err(FinalPathError::Unstable {
            attempts: MAX_ATTEMPTS,
        })
    }
}

impl crate::request::Request for QueryFinalPath {
    type Error = FinalPathError;
    type Output = Wtf16String;

    fn perform(&self) -> Result<Wtf16String, FinalPathError> {
        Self::perform(self)
    }
}

#[cfg(test)]
mod tests;
