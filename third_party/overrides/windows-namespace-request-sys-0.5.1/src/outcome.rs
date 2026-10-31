// Copyright (c) Mike Grier.

//! The faithful-execution contract every entry follows.
//!
//! An entry reports what Windows reported. It does not normalise a code, map it
//! onto a friendlier taxonomy, or decide that one failure "really means"
//! something else.
//!
//! # Why preservation is a constraint, not a preference
//!
//! `ERROR_FILE_NOT_FOUND` means three different things depending on which call
//! produced it and when: a missing directory from an open, an **empty**
//! directory from a first query, and a genuine failure from a later one. Only a
//! consumer holding that context can tell them apart. Any reclassification here
//! destroys information no layer above can reconstruct.
//!
//! # Why the code is snapshotted rather than read later
//!
//! `GetLastError` is thread state, and it is *volatile* thread state: almost
//! any subsequent Win32 call overwrites it, including cleanup a caller does not
//! think of as a call at all -- a `CloseHandle` in a `Drop`, a buffer being
//! released, a restoration guard unwinding. Reading it a few statements after
//! the failure is a race against the entry's own tidying up.
//!
//! So the read is not left to the caller's discipline. [`perform`] and its
//! convention-specific forms take the call as a closure and snapshot the code
//! **in the statement after it returns**, before anything else can run. Binding
//! to these functions is what makes the guarantee structural rather than a rule
//! each entry has to remember.
//!
//! # Scope: entries, not capture
//!
//! This governs the Win32 call an entry *performs*. It does not govern capture
//! failures -- [`crate::security`] and [`PathCaptureError`](crate::PathCaptureError)
//! report a named stage plus a code, because there the useful question is which
//! part of building the request went wrong. Those happen on the calling thread,
//! before any entry runs.

use windows_sys::Win32::Foundation::{
    ERROR_NOT_ENOUGH_MEMORY, FALSE, HANDLE, INVALID_HANDLE_VALUE,
};

/// A raw Win32 error code, exactly as Windows produced it: [`win_error_codes::Win32Error`], which
/// carries a code of 2^31 or more through its `io::Error` form, where this crate's own earlier type
/// did not.
///
/// Re-exported under the name this crate has always used, so every path keeps working. See the
/// shared type for its documentation.
///
/// # Example
///
/// ```
/// use windows_namespace_request_sys::Win32Error;
/// use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
///
/// let error = Win32Error::from_code(ERROR_FILE_NOT_FOUND);
/// assert_eq!(error.code(), ERROR_FILE_NOT_FOUND);
///
/// // The io::Error form is a re-presentation, not a reclassification: the raw
/// // code survives it, however large.
/// assert_eq!(error.to_io_error().raw_os_error(), Some(ERROR_FILE_NOT_FOUND as i32));
/// let virtdisk = Win32Error::from_code(0xC03A_0015);
/// assert_eq!(Win32Error::from_io(&virtdisk.to_io_error()), Some(virtdisk));
/// ```
pub use win_error_codes::Win32Error;

/// This crate's own report that it could not allocate memory for a call, which was then not made:
/// `ERROR_NOT_ENOUGH_MEMORY`. It is the one code here that the call did not produce.
pub(crate) fn out_of_memory() -> Win32Error {
    Win32Error::from_code(ERROR_NOT_ENOUGH_MEMORY)
}

/// What an entry's Win32 call produced: its result, or the raw code.
pub type Outcome<T> = Result<T, Win32Error>;

/// Performs `call` and, when `failed` says the result is a failure, snapshots
/// the thread's last error before anything else can run.
///
/// `failed` decides using only the returned value, because Win32's failure
/// conventions differ per call and none of them is inferable from the type. The
/// three the catalogue actually meets have named forms:
/// [`perform_bool`], [`perform_handle`], and [`perform_nonzero`]. Use this
/// general form for a call whose convention is none of those.
///
/// # Example
///
/// The guarantee this function exists for. Cleanup between the failing call and
/// the read is exactly what destroys a last-error code, and binding to
/// `perform` closes that window:
///
/// ```
/// use windows_namespace_request_sys::outcome::perform;
/// use windows_sys::Win32::Foundation::{
///     ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, SetLastError,
/// };
///
/// let outcome = perform(
///     || {
///         // SAFETY: SetLastError writes only this thread's error slot.
///         unsafe { SetLastError(ERROR_FILE_NOT_FOUND) };
///         -1_i32
///     },
///     |result| *result < 0,
/// );
///
/// // Cleanup runs afterwards and clobbers the thread's error slot -- a Drop, a
/// // buffer release, a restoration guard. The snapshot is already taken.
/// // SAFETY: as above.
/// unsafe { SetLastError(ERROR_ACCESS_DENIED) };
///
/// assert_eq!(
///     outcome.expect_err("a negative result is a failure").code(),
///     ERROR_FILE_NOT_FOUND
/// );
/// ```
pub fn perform<T>(call: impl FnOnce() -> T, failed: impl FnOnce(&T) -> bool) -> Outcome<T> {
    let result = call();

    // Nothing may go between these two statements. No drop runs here, no
    // cleanup, no second Win32 call -- which is the entire reason this function
    // exists rather than each entry reading GetLastError for itself.
    if failed(&result) {
        return Err(Win32Error::last());
    }

    Ok(result)
}

/// Performs a call whose `BOOL` return is `FALSE` on failure.
///
/// The successful value carries no information beyond "it worked", so it is
/// discarded rather than handed back as a bare integer.
///
/// # Errors
///
/// Returns the raw Win32 code when the call returns `FALSE`.
pub fn perform_bool(call: impl FnOnce() -> i32) -> Outcome<()> {
    perform(call, |result| *result == FALSE).map(|_| ())
}

/// Performs a call whose `HANDLE` return is `INVALID_HANDLE_VALUE` on failure.
///
/// This is the convention of `CreateFileW`, `OpenFileById`, and
/// `FindFirstChangeNotificationW`.
///
/// # Errors
///
/// Returns the raw Win32 code when the call returns `INVALID_HANDLE_VALUE`.
///
/// # Example
///
/// The two handle conventions disagree about the same values, which is why both
/// exist by name. Using one where the other belongs turns a failure into a
/// plausible-looking handle:
///
/// ```
/// use std::ptr;
///
/// use windows_namespace_request_sys::outcome::{perform_handle, perform_nonnull_handle};
/// use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
///
/// // Under the INVALID_HANDLE_VALUE convention, null is a *success*.
/// assert!(perform_handle(ptr::null_mut).is_ok());
/// assert!(perform_handle(|| INVALID_HANDLE_VALUE).is_err());
///
/// // Under the null convention, the two swap.
/// assert!(perform_nonnull_handle(|| INVALID_HANDLE_VALUE).is_ok());
/// assert!(perform_nonnull_handle(ptr::null_mut).is_err());
/// ```
pub fn perform_handle(call: impl FnOnce() -> HANDLE) -> Outcome<HANDLE> {
    perform(call, |result| *result == INVALID_HANDLE_VALUE)
}

/// Performs a call whose `HANDLE` return is **null** on failure.
///
/// A distinct convention from [`perform_handle`], and getting the two the wrong
/// way round turns a failure into a plausible-looking handle. Both are provided
/// because Windows uses both.
///
/// # Errors
///
/// Returns the raw Win32 code when the call returns a null handle.
pub fn perform_nonnull_handle(call: impl FnOnce() -> HANDLE) -> Outcome<HANDLE> {
    perform(call, |result| result.is_null())
}

/// Performs a call whose numeric return is `0` on failure.
///
/// This is the convention of the sizing and length calls, such as
/// `GetFullPathNameW` and `GetFinalPathNameByHandleW`.
///
/// # Errors
///
/// Returns the raw Win32 code when the call returns `0`.
pub fn perform_nonzero(call: impl FnOnce() -> u32) -> Outcome<u32> {
    perform(call, |result| *result == 0)
}

#[cfg(test)]
mod tests;
