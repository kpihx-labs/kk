// Copyright (c) Mike Grier.

//! The `GetVolumeInformationByHandleW` entry.
//!
//! Entry 8 of the audited catalogue: the **handle-based** volume query.
//!
//! # Why the path-based call is not here
//!
//! `GetVolumeInformationW` takes a root path rather than a handle and is a
//! different Win32 call, so it would be its own entry. No audited consumer
//! calls it, so it is deliberately out of round one -- recorded so a later
//! reader can tell a considered omission from an unexamined one.
//!
//! # Two buffers, one call
//!
//! The call fills a volume-label buffer and a filesystem-name buffer in the
//! same invocation, alongside three scalar out-parameters. Both buffers have
//! documented maximums, so unlike [`crate::final_path`] this entry needs no
//! grow-and-retry: it allocates the maximum once and is done.

use std::fmt;

use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationByHandleW;
use wtf_string::Wtf16String;

use win_shared_os_owned_handle::SharedHandle;

use crate::handle::SharedRaw;
use crate::outcome::{Outcome, perform_bool};

/// The longest volume label Windows supports, plus room for the terminator.
///
/// `MAX_PATH + 1`, which is what the documentation specifies for this buffer.
const LABEL_CAPACITY: usize = 261;

/// The longest filesystem name buffer, plus room for the terminator.
///
/// The same bound; a filesystem name is far shorter in practice, but the call's
/// contract is stated in these terms.
const FILESYSTEM_NAME_CAPACITY: usize = 261;

/// What a volume reported about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VolumeInformation {
    label: Wtf16String,
    serial_number: u32,
    maximum_component_length: u32,
    flags: u32,
    filesystem_name: Wtf16String,
}

impl VolumeInformation {
    /// The volume label, which is frequently empty and is not an identifier.
    #[must_use]
    pub fn label(&self) -> &Wtf16String {
        &self.label
    }

    /// The volume serial number.
    ///
    /// Not stable across reformatting, and not unique across machines, so it
    /// identifies a volume only in combination with something else.
    #[must_use]
    pub fn serial_number(&self) -> u32 {
        self.serial_number
    }

    /// The longest single path component the filesystem accepts.
    #[must_use]
    pub fn maximum_component_length(&self) -> u32 {
        self.maximum_component_length
    }

    /// The raw `FILE_*` capability flags, carried unaltered.
    ///
    /// A bitmask rather than an enum, so a capability Windows adds later still
    /// reaches a consumer.
    #[must_use]
    pub fn flags(&self) -> u32 {
        self.flags
    }

    /// The filesystem name, such as `NTFS` or `ReFS`.
    #[must_use]
    pub fn filesystem_name(&self) -> &Wtf16String {
        &self.filesystem_name
    }
}

impl fmt::Display for VolumeInformation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} volume {:08X}",
            self.filesystem_name.to_string_lossy(),
            self.serial_number
        )
    }
}

/// An owned, marshalable parameter set for `GetVolumeInformationByHandleW`.
///
/// # Example
///
/// ```
/// use std::fs;
///
/// use windows_namespace_request_sys::volume::QueryVolumeInformation;
///
/// let path = std::env::temp_dir().join(format!("wnrs-vol-{}.tmp", std::process::id()));
/// fs::write(&path, b"example")?;
/// let file = fs::File::open(&path)?;
///
/// let volume = QueryVolumeInformation::new(file).perform()?;
///
/// // A real volume names its filesystem and reports a component limit.
/// assert!(!volume.filesystem_name().is_empty());
/// assert!(volume.maximum_component_length() > 0);
/// # fs::remove_file(&path)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug)]
#[must_use = "an unperformed request queries nothing"]
pub struct QueryVolumeInformation {
    handle: SharedHandle,
}

impl QueryVolumeInformation {
    /// Begins a request against `handle`.
    ///
    /// The handle names any file or directory on the volume; the volume is what
    /// gets reported.
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
    pub fn perform(&self) -> Outcome<VolumeInformation> {
        let mut label = Wtf16String::with_capacity(LABEL_CAPACITY);
        let mut filesystem_name = Wtf16String::with_capacity(FILESYSTEM_NAME_CAPACITY);
        let mut serial_number = 0_u32;
        let mut maximum_component_length = 0_u32;
        let mut flags = 0_u32;

        perform_bool(|| {
            // SAFETY: the handle is one this request holds and keeps
            // open across the call; both buffers are writable for the
            // capacities passed; the three scalar out-parameters point at
            // writable storage. Each buffer's invariant is restored below
            // before it is observed.
            unsafe {
                GetVolumeInformationByHandleW(
                    self.handle.raw(),
                    label.as_mut_ptr(),
                    u32::try_from(LABEL_CAPACITY).expect("a small constant fits a u32"),
                    &raw mut serial_number,
                    &raw mut maximum_component_length,
                    &raw mut flags,
                    filesystem_name.as_mut_ptr(),
                    u32::try_from(FILESYSTEM_NAME_CAPACITY).expect("a small constant fits a u32"),
                )
            }
        })?;

        // SAFETY: a successful call NUL-terminates both buffers within the
        // capacities given, so the terminator search stays in bounds.
        unsafe {
            set_len_to_terminator(&mut label, LABEL_CAPACITY);
            set_len_to_terminator(&mut filesystem_name, FILESYSTEM_NAME_CAPACITY);
        }

        Ok(VolumeInformation {
            label,
            serial_number,
            maximum_component_length,
            flags,
            filesystem_name,
        })
    }
}

/// Restores a buffer's length from the NUL terminator Win32 wrote.
///
/// The call reports no length for either string buffer -- unlike
/// [`crate::final_path`], which returns one -- so the terminator is the only
/// signal available.
///
/// # Safety
///
/// `buffer` must have capacity for `capacity` characters, and Win32 must have
/// written a NUL-terminated string within it.
///
/// Note what this deliberately does **not** require: that all `capacity`
/// characters are initialised. Win32 writes only the string it produced plus a
/// terminator, so most of the buffer is untouched -- which is why the scan
/// below reads one element at a time through a raw pointer rather than forming
/// a slice over the whole capacity. A `&[u16]` spanning uninitialised elements
/// would be undefined behaviour the moment it was created, before `position`
/// ever short-circuited at the terminator.
unsafe fn set_len_to_terminator(buffer: &mut Wtf16String, capacity: usize) {
    let base = buffer.as_mut_ptr();
    let mut length = 0;

    while length < capacity {
        // SAFETY: `base` is valid for `capacity` characters of storage, and
        // every element up to and including the terminator was written by
        // Win32 per this function's contract, so each read is of an
        // initialised element.
        if unsafe { base.add(length).read() } == 0 {
            break;
        }
        length += 1;
    }

    // A terminator at the very end would mean Win32 filled the buffer without
    // room for one, which its contract forbids; treating that as an empty
    // string keeps the invariant rather than reporting content that was never
    // terminated.
    if length == capacity {
        length = 0;
    }

    // SAFETY: `length` is the terminator's index, so that many content
    // characters were written and it is within capacity.
    unsafe { buffer.set_len_from_ffi(length) };
}

impl crate::request::Request for QueryVolumeInformation {
    type Error = crate::Win32Error;
    type Output = VolumeInformation;

    fn perform(&self) -> Outcome<VolumeInformation> {
        Self::perform(self)
    }
}

#[cfg(test)]
mod tests;
