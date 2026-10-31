// Copyright (c) Mike Grier.

//! A path as the caller gave it, with a relative one fixed to the current directory of the moment
//! the request was built (D-19).
//!
//! The process current directory is shared and mutable, and a request is performed later, on any
//! thread. So a relative path is grafted onto the current directory **when the request is built**
//! and the result is what `perform` hands to Win32, however the current directory has changed since.
//!
//! # The graft
//!
//! The captured directory, a backslash unless it already ends in one (a drive root does), then the
//! child. Nothing is combined, canonicalised, length-checked or prefixed with `\\?\`:
//! `CreateFileW` normalises `.`, `..` and trailing dots and spaces on the final path itself, and
//! Win32 decides whether a long result is acceptable to the host.
//!
//! # Only a clearly safe child is grafted
//!
//! All of these must hold, and a child that fails any of them is kept exactly as given, for Win32 to
//! resolve when the call is made:
//!
//! 1. it is not empty;
//! 2. it does not begin with `\` or `/`, which rules out root-relative, UNC, `\\?\` and `\\.\`;
//! 3. it contains no `:`, which rules out drive-relative `C:a` and `C:` and a stream `name:stream`;
//! 4. if the captured directory is a UNC path, the child has no `..` component;
//! 5. no component of the child is a reserved device name: `CON`, `PRN`, `AUX`, `NUL`, `COM` or
//!    `LPT` with a digit, `CONIN$` or `CONOUT$`, compared without regard to ASCII case, after
//!    removing everything from the first `.` and any trailing spaces;
//! 6. the captured directory does not begin with `\\?\` or `\\.\`. A path with the `\\?\` prefix is
//!    not normalised by Win32, so a child joined onto one would be taken literally, `.` and `..`
//!    components, forward slashes and trailing dots and spaces among what would then mean something
//!    else than they meant relative to that directory. A `\\.\` path is normalised (measured, in
//!    D-28), so for it the rule is wider than its reason, which errs toward not grafting.
//!
//! The rules err toward not grafting. Where they are wider than Microsoft's list, the child is
//! passed through, which is what happens to every form that is not grafted. The digits of rule 5
//! are `0` to `9` and the superscripts `U+00B9`, `U+00B2` and `U+00B3` that the documentation also
//! names. A directory that begins with two separators of either kind counts as UNC for rule 4.
//!
//! The current directory is read only when it is needed: a path that rules 1, 2, 3 and 5 already
//! refuse makes no call. Rules 4 and 6 are about the directory itself, so they are decided after it
//! is read.
//!
//! # The one thing refused
//!
//! An interior NUL, which a `PCWSTR` cannot carry: Win32 would read the path as ending there and
//! act on a shorter path than the caller named. It is refused when the request is built, where the
//! caller can still act on it, and so is a failure to read the current directory. Nothing else about
//! a path is checked here.

use std::fmt;

use windows_sys::Win32::System::Environment::GetCurrentDirectoryW;
use wtf_string::{Wtf16Str, Wtf16String};

use crate::outcome::{Win32Error, perform_nonzero};

/// How many times the buffer is grown before giving up, as in [`crate::full_path`].
const MAX_ATTEMPTS: usize = 8;

/// The buffer size the first attempt uses, in characters.
const FIRST_ATTEMPT_CHARS: usize = 260;

const BACKSLASH: u16 = b'\\' as u16;
const SLASH: u16 = b'/' as u16;
const COLON: u16 = b':' as u16;
const DOT: u16 = b'.' as u16;
const QUESTION: u16 = b'?' as u16;
const SPACE: u16 = b' ' as u16;
const DIGIT_ZERO: u16 = b'0' as u16;
const DIGIT_NINE: u16 = b'9' as u16;

/// The superscript one, two and three, which Microsoft's list also names after `COM` and `LPT`.
const SUPERSCRIPT_DIGITS: [u16; 3] = [0x00B9, 0x00B2, 0x00B3];

/// The names that are reserved whole. Changing one changes which children are grafted.
const RESERVED_NAMES: [&[u8]; 6] = [b"CON", b"PRN", b"AUX", b"NUL", b"CONIN$", b"CONOUT$"];

/// The names that are reserved with a digit after them.
const RESERVED_NAMES_WITH_DIGIT: [&[u8]; 2] = [b"COM", b"LPT"];

/// Why a path could not be captured for a request.
///
/// The failures are the two a path can have at construction: it cannot be carried to Win32 as it
/// is, or the current directory it is to be fixed to could not be read. The second mirrors
/// [`crate::full_path::FullPathError`], which shares the retry shape and so the vocabulary.
#[derive(Debug)]
#[non_exhaustive]
pub enum PathCaptureError {
    /// The path contains a NUL before its end. Win32 reads a path up to its first NUL, so it would
    /// act on a shorter path than the one named.
    InteriorNul,
    /// `GetCurrentDirectoryW` was refused, with the raw code unaltered.
    CurrentDirectory(Win32Error),
    /// The size of the current directory kept changing, so the retry was abandoned.
    CurrentDirectoryUnstable {
        /// How many attempts were made before giving up.
        attempts: usize,
    },
}

impl fmt::Display for PathCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InteriorNul => f.write_str("the path contains an interior NUL"),
            Self::CurrentDirectory(error) => write!(f, "GetCurrentDirectoryW: {error}"),
            Self::CurrentDirectoryUnstable { attempts } => write!(
                f,
                "GetCurrentDirectoryW: the required size changed on each of {attempts} attempts"
            ),
        }
    }
}

impl std::error::Error for PathCaptureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CurrentDirectory(error) => Some(error),
            Self::InteriorNul | Self::CurrentDirectoryUnstable { .. } => None,
        }
    }
}

/// A path as given, and the path to hand to Win32 for it.
///
/// The two are the same unless the path was grafted, by the rules in this module's documentation. It is
/// what [`OpenFile`](crate::OpenFile) and [`WatchDirectory`](crate::WatchDirectory) hold, and is public
/// for a crate that builds a request of its own and performs it later on another thread, so that it
/// fixes the meaning of a relative path by the same contract and does not keep a second one (D-30).
///
/// # Example
///
/// ```
/// use windows_namespace_request_sys::AnchoredPath;
/// use wtf_string::Wtf16String;
///
/// // An absolute path is handed to Win32 as it was given.
/// let absolute = AnchoredPath::new(Wtf16String::from(r"C:\Windows"))?;
/// assert_eq!(absolute.given(), absolute.win32());
///
/// // A relative name is grafted onto the current directory as it is now.
/// let relative = AnchoredPath::new(Wtf16String::from("a-child"))?;
/// assert_eq!(relative.given().to_string_lossy(), "a-child");
/// let expected = std::env::current_dir()?.join("a-child");
/// assert_eq!(relative.win32().to_string_lossy(), expected.to_string_lossy());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AnchoredPath {
    given: Wtf16String,
    grafted: Option<Wtf16String>,
}

impl AnchoredPath {
    /// Anchors `given` to the process current directory as it is now.
    ///
    /// # Errors
    ///
    /// [`PathCaptureError::InteriorNul`] for a path with a NUL before its end, and otherwise the
    /// failure of `GetCurrentDirectoryW`, which is called only for a path whose shape permits a
    /// graft.
    pub fn new(given: Wtf16String) -> Result<Self, PathCaptureError> {
        if given.has_interior_nul() {
            return Err(PathCaptureError::InteriorNul);
        }
        if !has_graftable_shape(given.as_units()) {
            return Ok(Self {
                given,
                grafted: None,
            });
        }
        let root = capture_current_directory()?;
        Ok(Self::anchored_at(given, &root))
    }

    /// Anchors `given` to `root`, which stands for the current directory.
    ///
    /// No call is made, so what the rules do is testable without touching process state.
    pub(crate) fn anchored_at(given: Wtf16String, root: &Wtf16Str) -> Self {
        let grafted =
            graft(given.as_units(), root.as_units()).map(|units| Wtf16String::from_units(&units));
        Self { given, grafted }
    }

    /// The path exactly as the caller gave it.
    #[must_use]
    pub fn given(&self) -> &Wtf16Str {
        &self.given
    }

    /// The path that is handed to Win32.
    #[must_use]
    pub fn win32(&self) -> &Wtf16Str {
        self.grafted.as_ref().unwrap_or(&self.given)
    }

    /// Whether the path was grafted onto the captured directory.
    #[cfg(test)]
    pub(crate) fn is_grafted(&self) -> bool {
        self.grafted.is_some()
    }

    /// A NUL-terminated pointer to [`win32`](Self::win32), for a Win32 call.
    ///
    /// Borrows from this value and must not outlive it.
    pub(crate) fn win32_terminated(&self) -> *const u16 {
        self.grafted
            .as_ref()
            .unwrap_or(&self.given)
            .as_terminated_ptr()
    }
}

/// The root joined to the child, when the child may be grafted onto that root.
fn graft(child: &[u16], root: &[u16]) -> Option<Vec<u16>> {
    if root.is_empty() || !has_graftable_shape(child) {
        return None;
    }
    if is_verbatim_or_device(root) {
        return None;
    }
    if is_unc(root) && has_parent_component(child) {
        return None;
    }

    let needs_separator = root.last() != Some(&BACKSLASH);
    let mut joined = Vec::with_capacity(root.len() + 1 + child.len());
    joined.extend_from_slice(root);
    if needs_separator {
        joined.push(BACKSLASH);
    }
    joined.extend_from_slice(child);
    Some(joined)
}

/// Rules 1, 2, 3 and 5: what can be decided without the directory.
fn has_graftable_shape(child: &[u16]) -> bool {
    let Some(first) = child.first() else {
        return false;
    };
    if is_separator(*first) || child.contains(&COLON) {
        return false;
    }
    !components(child).any(is_reserved_device_name)
}

/// Rule 6: a directory with the `\\?\` or `\\.\` prefix, which only backslashes make.
fn is_verbatim_or_device(root: &[u16]) -> bool {
    matches!(root, [BACKSLASH, BACKSLASH, prefix, BACKSLASH, ..] if *prefix == QUESTION || *prefix == DOT)
}

/// Rule 4's two halves: a directory that is a UNC path, and a child with a `..` component.
fn is_unc(root: &[u16]) -> bool {
    matches!(root, [first, second, ..] if is_separator(*first) && is_separator(*second))
}

fn has_parent_component(child: &[u16]) -> bool {
    components(child).any(|component| component == [DOT, DOT])
}

fn is_separator(unit: u16) -> bool {
    unit == BACKSLASH || unit == SLASH
}

fn components(path: &[u16]) -> impl Iterator<Item = &[u16]> {
    path.split(|unit| is_separator(*unit))
}

/// Rule 5, for one component.
fn is_reserved_device_name(component: &[u16]) -> bool {
    let stem = component.split(|unit| *unit == DOT).next().unwrap_or(&[]);
    let end = stem
        .iter()
        .rposition(|unit| *unit != SPACE)
        .map_or(0, |last| last + 1);
    let stem = &stem[..end];

    if RESERVED_NAMES.iter().any(|name| equals_ascii(stem, name)) {
        return true;
    }
    let Some((digit, name)) = stem.split_last() else {
        return false;
    };
    let is_digit = (DIGIT_ZERO..=DIGIT_NINE).contains(digit) || SUPERSCRIPT_DIGITS.contains(digit);
    is_digit
        && RESERVED_NAMES_WITH_DIGIT
            .iter()
            .any(|reserved| equals_ascii(name, reserved))
}

/// Whether `units` is `name`, ignoring ASCII case. A unit that is not ASCII never matches.
fn equals_ascii(units: &[u16], name: &[u8]) -> bool {
    units.len() == name.len()
        && units.iter().zip(name).all(|(unit, expected)| {
            u8::try_from(*unit).is_ok_and(|unit| unit.eq_ignore_ascii_case(expected))
        })
}

fn capture_current_directory() -> Result<Wtf16String, PathCaptureError> {
    read_current_directory(|buffer| {
        let capacity = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        perform_nonzero(|| {
            // SAFETY: the buffer is writable for `capacity` characters.
            unsafe { GetCurrentDirectoryW(capacity, buffer.as_mut_ptr()) }
        })
    })
}

/// The retry around the call, with the call as a parameter so that its answers can be chosen.
fn read_current_directory(
    mut call: impl FnMut(&mut [u16]) -> Result<u32, Win32Error>,
) -> Result<Wtf16String, PathCaptureError> {
    let mut buffer = vec![0_u16; FIRST_ATTEMPT_CHARS];

    for _ in 0..MAX_ATTEMPTS {
        let written = call(&mut buffer).map_err(PathCaptureError::CurrentDirectory)? as usize;
        if written < buffer.len() {
            // Success: `written` excludes the terminator.
            return Ok(Wtf16String::from_units(&buffer[..written]));
        }
        // Too small: `written` is the size required including the terminator.
        buffer.resize(written, 0);
    }

    Err(PathCaptureError::CurrentDirectoryUnstable {
        attempts: MAX_ATTEMPTS,
    })
}

#[cfg(test)]
mod tests;
