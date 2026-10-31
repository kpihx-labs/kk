// Copyright (c) Mike Grier.

//! The seam: what every entry has in common, as a trait.
//!
//! Each entry is already a value whose `perform` is the single point where
//! Win32 is touched. This module adds the trait over that, so a consumer's code
//! can be written against "a request that produces `T`" rather than against a
//! concrete entry -- and can therefore be exercised in that consumer's own
//! tests without a filesystem, a network path, or a device that may not be
//! present.
//!
//! # Why two traits rather than one
//!
//! The distinction is real, not cosmetic. An open is a **parameter set**: it
//! may be performed repeatedly, producing an independent handle each time, so
//! it takes `&self`. A close is **one-shot**: performing it consumes the
//! request, which is what makes closing twice through this crate impossible.
//!
//! Collapsing them into one trait would have to pick a side, and both choices
//! lie. A `&self` trait would make a close look repeatable; a `self` trait would
//! make every open look single-use and force a caller to rebuild a request it
//! could simply have performed again.
//!
//! # Why the error type is an associated type
//!
//! Most entries fail only as Windows failed, so their error is a
//! [`Win32Error`](crate::Win32Error). Two do not:
//! [`crate::final_path::QueryFinalPath`] and
//! [`crate::full_path::ResolveFullPath`] each retry a growing buffer, and "the
//! required size kept changing" is a failure Win32 has no code for. They report
//! it as [`FinalPathError::Unstable`](crate::FinalPathError::Unstable) and
//! [`FullPathError::Unstable`](crate::FullPathError::Unstable) respectively.
//!
//! Fixing the trait's error to `Win32Error` would have left those entries
//! outside the seam, which would make the seam not level -- a consumer could
//! substitute a fake for some entries and not the rest. An associated `Error`
//! keeps every entry reachable through one trait without any of them having to
//! invent a code it does not have.
//!
//! That last clause is load-bearing rather than decorative. `ResolveFullPath`
//! did invent one for a while, returning a synthesized
//! `ERROR_INSUFFICIENT_BUFFER` that Win32 can also produce by itself, so a
//! caller could not tell the crate's own retry giving up from a genuine Windows
//! failure. The rule stated here is what the entry now follows.
//!
//! # This is a seam, not an abstraction layer
//!
//! The traits exist so a *consumer* can substitute a fake. They are not a
//! plug-in point for alternative implementations of Windows, and nothing in
//! this crate dispatches through them: the entries keep their inherent
//! `perform` methods, which is what an ordinary caller uses.

/// A request that may be performed more than once.
///
/// Implemented by the entries that carry parameters and produce something new
/// each time: [`crate::open::OpenFile`],
/// [`crate::open_by_id::OpenFileByIdentifier`], and
/// [`crate::watch::WatchDirectory`].
///
/// # Example
///
/// A consumer writes its own code against the trait, then tests it against a
/// fake that never touches the filesystem:
///
/// ```
/// use windows_namespace_request_sys::outcome::Outcome;
/// use windows_namespace_request_sys::request::Request;
/// use windows_namespace_request_sys::Win32Error;
/// use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
///
/// // The consumer's code: generic over the request, so it can be exercised
/// // without opening anything.
/// fn count_successes<R: Request>(requests: &[R], attempts: usize) -> usize {
///     requests
///         .iter()
///         .flat_map(|request| (0..attempts).map(move |_| request.perform()))
///         .filter(Result::is_ok)
///         .count()
/// }
///
/// // The consumer's fake: a canned outcome, no Win32 anywhere.
/// struct AlwaysMissing;
///
/// impl Request for AlwaysMissing {
///     type Error = Win32Error;
///     type Output = ();
///
///     fn perform(&self) -> Outcome<()> {
///         Err(Win32Error::from_code(ERROR_FILE_NOT_FOUND))
///     }
/// }
///
/// struct AlwaysOpens;
///
/// impl Request for AlwaysOpens {
///     type Error = Win32Error;
///     type Output = u32;
///
///     fn perform(&self) -> Outcome<u32> {
///         Ok(7)
///     }
/// }
///
/// assert_eq!(count_successes(&[AlwaysMissing, AlwaysMissing], 3), 0);
/// assert_eq!(count_successes(&[AlwaysOpens], 3), 3, "a request may be performed repeatedly");
/// ```
pub trait Request {
    /// What performing the request produces.
    type Output;

    /// How performing it can fail.
    ///
    /// [`Win32Error`](crate::Win32Error) for every entry that fails only as
    /// Windows failed. The two that also retry a growing buffer --
    /// [`QueryFinalPath`](crate::final_path::QueryFinalPath) and
    /// [`ResolveFullPath`](crate::full_path::ResolveFullPath) -- carry their own
    /// error instead, as the module documentation explains.
    type Error;

    /// Performs the request on the calling thread.
    ///
    /// # Errors
    ///
    /// Returns the raw Win32 code, unaltered, per this crate's
    /// faithful-execution contract -- or, for an entry with a failure Win32 has
    /// no code for, that entry's own error.
    fn perform(&self) -> Result<Self::Output, Self::Error>;
}

/// A request that is consumed by performing it.
///
/// Implemented by [`crate::close::CloseRequest`], where performing twice would
/// mean closing a handle twice. The trait carries that property rather than
/// leaving it to a comment.
///
/// # Example
///
/// ```
/// use windows_namespace_request_sys::outcome::Outcome;
/// use windows_namespace_request_sys::request::ConsumingRequest;
/// use windows_namespace_request_sys::Win32Error;
///
/// // A consumer's cleanup step, written against the trait.
/// fn perform_all<R: ConsumingRequest>(requests: Vec<R>) -> usize {
///     requests
///         .into_iter()
///         .filter(|_| true)
///         .map(ConsumingRequest::perform)
///         .filter(Result::is_ok)
///         .count()
/// }
///
/// struct FakeClose;
///
/// impl ConsumingRequest for FakeClose {
///     type Error = Win32Error;
///     type Output = ();
///
///     fn perform(self) -> Outcome<()> {
///         Ok(())
///     }
/// }
///
/// assert_eq!(perform_all(vec![FakeClose, FakeClose]), 2);
/// ```
pub trait ConsumingRequest {
    /// What performing the request produces.
    type Output;

    /// How performing it can fail.
    type Error;

    /// Performs the request on the calling thread, consuming it.
    ///
    /// # Errors
    ///
    /// Returns the raw Win32 code, unaltered.
    fn perform(self) -> Result<Self::Output, Self::Error>;
}

#[cfg(test)]
mod tests;
