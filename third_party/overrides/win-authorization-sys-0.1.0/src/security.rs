// Copyright (c) Mike Grier.
// Split from crates/windows-namespace-request-sys/src/security.rs at 11020848.

//! Owned capture of a caller's `lpSecurityAttributes`.
//!
//! A `SECURITY_ATTRIBUTES` is not a value. It points at a security descriptor,
//! and that descriptor may itself be **absolute**: a structure of raw pointers
//! to an owner SID, a group SID, a DACL and a SACL that are quite possibly on
//! the caller's stack. Carrying one to another thread by copying the struct
//! would carry a set of dangling pointers.
//!
//! Capture therefore normalises to the **self-relative** form, in which every
//! part lives at an offset inside one contiguous blob, and owns that blob.

use std::ffi::c_void;
use std::fmt;
use std::io;
use std::ptr;

use windows_sys::Win32::Foundation::{
    ERROR_INSUFFICIENT_BUFFER, ERROR_NOT_ENOUGH_MEMORY, FALSE, TRUE,
};
use windows_sys::Win32::Security::{
    ACL, GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetSecurityDescriptorSacl,
    IsValidSecurityDescriptor, MakeAbsoluteSD, MakeSelfRelativeSD, PSECURITY_DESCRIPTOR,
    SE_SELF_RELATIVE, SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR_CONTROL,
};

use io_buffers::AlignedBuf;

mod query;
mod sddl;

pub use query::{ObjectType, get_security_info, get_security_info_unchecked};
pub use sddl::SecurityInformation;

/// A self-relative security descriptor must be DWORD-aligned.
///
/// Windows states this as a requirement on the buffer a self-relative
/// descriptor lives in, not as a property of any one field, which is why it is
/// enforced on the whole blob.
const SELF_RELATIVE_ALIGNMENT: usize = align_of::<u32>();

/// The alignment of the buffers an absolute descriptor is taken apart into: its header holds
/// pointers, and its ACLs and SIDs need no more than that.
const ABSOLUTE_ALIGNMENT: usize = align_of::<usize>();

// `AlignedBuf` refuses an alignment that is not a power of two, and `allocate` turns every refusal
// into an allocation failure, so this is checked where it cannot be mistaken for one.
const _: () = assert!(SELF_RELATIVE_ALIGNMENT.is_power_of_two());
const _: () = assert!(ABSOLUTE_ALIGNMENT.is_power_of_two());

/// Why a caller's security attributes could not be captured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SecurityCaptureFailure {
    /// Windows rejected the descriptor as malformed.
    ///
    /// Reported at construction, on the calling thread, rather than left for
    /// the eventual call to fail with it on a worker.
    InvalidDescriptor,
    /// Windows could not report the descriptor's control flags, so whether it
    /// was absolute or self-relative could not be established.
    ReadControl,
    /// Windows could not size the self-relative form of an absolute
    /// descriptor.
    SizeSelfRelative,
    /// Windows could not convert an absolute descriptor to self-relative form.
    MakeSelfRelative,
    /// Windows could not size the absolute form of a self-relative descriptor.
    SizeAbsolute,
    /// Windows could not take a self-relative descriptor apart into absolute form.
    MakeAbsolute,
    /// The captured copy did not validate, which would mean the conversion
    /// produced something Windows will not accept later.
    InvalidCopy,
    /// Windows could not report the descriptor's DACL or SACL.
    ReadAcl,
    /// The memory for the owned copy could not be allocated.
    ///
    /// The error's code is `ERROR_NOT_ENOUGH_MEMORY`, which this crate supplies; no Win32 call
    /// produced it.
    Allocate,
    /// Windows refused an SDDL string. The error's code is Windows' own.
    InvalidSddl,
    /// An SDDL string contained an interior NUL, which Windows would read as its end. Refused
    /// before any call is made; the error's code is `ERROR_INVALID_PARAMETER`, which this crate
    /// supplies.
    InteriorNul,
    /// Windows refused to write a descriptor as SDDL. The error's code is Windows' own.
    ToSddl,
    /// Windows refused to read the security of an object by its handle. The error's code is
    /// Windows' own.
    ReadSecurity,
    /// [`get_security_info`] was given an object kind it does not pass with a handle of any kind.
    /// Refused before any call is made, as this crate's own narrowing; the error's code is
    /// `ERROR_INVALID_PARAMETER`, which this crate supplies.
    UnsupportedObjectType,
}

/// A synchronous failure while capturing a caller's security attributes.
#[derive(Debug)]
pub struct SecurityCaptureError {
    failure: SecurityCaptureFailure,
    source: io::Error,
}

impl SecurityCaptureError {
    fn new(failure: SecurityCaptureFailure, source: io::Error) -> Self {
        Self { failure, source }
    }

    fn last_os(failure: SecurityCaptureFailure) -> Self {
        Self::new(failure, io::Error::last_os_error())
    }

    /// The memory for an owned copy could not be allocated.
    fn allocation() -> Self {
        Self::new(
            SecurityCaptureFailure::Allocate,
            io::Error::from_raw_os_error(
                i32::try_from(ERROR_NOT_ENOUGH_MEMORY)
                    .expect("ERROR_NOT_ENOUGH_MEMORY fits in i32"),
            ),
        )
    }

    /// The underlying error, for a caller that reports the same failure under its own kind.
    #[must_use]
    pub fn into_source(self) -> io::Error {
        self.source
    }

    /// Why the capture failed.
    #[must_use]
    pub fn failure(&self) -> SecurityCaptureFailure {
        self.failure
    }

    /// The underlying Win32 error code.
    #[must_use]
    pub fn raw_os_error(&self) -> Option<i32> {
        self.source.raw_os_error()
    }
}

impl fmt::Display for SecurityCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let stage = match self.failure {
            SecurityCaptureFailure::InvalidDescriptor => "IsValidSecurityDescriptor",
            SecurityCaptureFailure::ReadControl => "GetSecurityDescriptorControl",
            SecurityCaptureFailure::SizeSelfRelative => "MakeSelfRelativeSD (sizing)",
            SecurityCaptureFailure::MakeSelfRelative => "MakeSelfRelativeSD",
            SecurityCaptureFailure::SizeAbsolute => "MakeAbsoluteSD (sizing)",
            SecurityCaptureFailure::MakeAbsolute => "MakeAbsoluteSD",
            SecurityCaptureFailure::InvalidCopy => "IsValidSecurityDescriptor (captured copy)",
            SecurityCaptureFailure::ReadAcl => "GetSecurityDescriptorDacl",
            SecurityCaptureFailure::Allocate => "allocating the descriptor copy",
            SecurityCaptureFailure::InvalidSddl => {
                "ConvertStringSecurityDescriptorToSecurityDescriptorW"
            }
            SecurityCaptureFailure::InteriorNul => "an SDDL string with an interior NUL",
            SecurityCaptureFailure::ToSddl => {
                "ConvertSecurityDescriptorToStringSecurityDescriptorW"
            }
            SecurityCaptureFailure::ReadSecurity => "GetSecurityInfo",
            SecurityCaptureFailure::UnsupportedObjectType => {
                "an object kind get_security_info does not take with any handle"
            }
        };

        write!(f, "{stage}: {}", self.source)
    }
}

impl std::error::Error for SecurityCaptureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// What a descriptor says about one of its access-control lists.
///
/// The three states that look alike and are not are kept apart here, because
/// collapsing any pair of them silently changes what the resulting object
/// permits.
///
/// # Example
///
/// The pair that matters most: a **NULL** DACL grants everyone complete access
/// and an **empty** one grants nobody anything. They are opposites, so an
/// `Option<Acl>` that flattened them would not lose detail -- it would invert
/// the grant.
///
/// ```
/// use win_authorization_sys::AclState;
///
/// let absent = AclState::Absent;
/// let null = AclState::Null;
/// let empty = AclState::Empty;
///
/// assert_ne!(null, empty, "NULL allows all; empty allows none");
/// assert_ne!(absent, null);
/// assert_ne!(absent, empty);
///
/// // A populated list also reports how many entries it carries.
/// assert_eq!(AclState::Populated(1), AclState::Populated(1));
/// assert_ne!(AclState::Populated(1), AclState::Populated(2));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AclState {
    /// The descriptor carries no list at all. The object takes its default.
    Absent,
    /// The descriptor carries a **NULL** list. For a DACL this grants everyone
    /// complete access -- the opposite of what [`Empty`](Self::Empty) does.
    Null,
    /// The descriptor carries a list with no entries. For a DACL this grants
    /// nobody any access.
    Empty,
    /// The descriptor carries a list with this many entries.
    Populated(u32),
}

/// An owned, self-relative copy of a caller's security descriptor.
///
/// Whatever form the caller supplied, this holds the self-relative form in one
/// contiguous, DWORD-aligned blob, so it names nothing outside itself and can
/// be carried to another thread.
#[derive(Debug, PartialEq, Eq)]
pub struct SecurityDescriptor {
    blob: AlignedBuf,
}

/// Memory for `length` zeroed bytes of self-relative descriptor.
///
/// Every refusal of the allocation is reported as [`SecurityCaptureFailure::Allocate`]: the
/// alignment is a constant checked at build time, so what remains is a length the machine
/// cannot supply.
fn allocate(length: usize) -> Result<AlignedBuf, SecurityCaptureError> {
    AlignedBuf::zeroed(length, SELF_RELATIVE_ALIGNMENT)
        .map_err(|_| SecurityCaptureError::allocation())
}

/// Memory for one part of an absolute descriptor, as [`allocate`] for a self-relative one.
fn allocate_part(length: u32) -> Result<AlignedBuf, SecurityCaptureError> {
    AlignedBuf::zeroed(length as usize, ABSOLUTE_ALIGNMENT)
        .map_err(|_| SecurityCaptureError::allocation())
}

/// A part's address for `MakeAbsoluteSD`: null for a part the descriptor does not have.
fn part_pointer<T>(part: &mut AlignedBuf) -> *mut T {
    if part.is_empty() {
        ptr::null_mut()
    } else {
        part.as_mut_ptr().cast::<T>()
    }
}

/// The code a sizing call is expected to fail with.
fn insufficient_buffer() -> i32 {
    i32::try_from(ERROR_INSUFFICIENT_BUFFER).expect("ERROR_INSUFFICIENT_BUFFER fits in i32")
}

impl SecurityDescriptor {
    /// Copies the descriptor.
    ///
    /// Not `Clone`, because the copy allocates and the allocation can fail.
    ///
    /// # Errors
    ///
    /// Returns [`SecurityCaptureFailure::Allocate`] when the memory for the copy cannot be
    /// allocated.
    pub fn try_clone(&self) -> Result<Self, SecurityCaptureError> {
        self.blob
            .try_clone()
            .map(|blob| Self { blob })
            .map_err(|_| SecurityCaptureError::allocation())
    }

    /// Captures the descriptor at `descriptor`.
    ///
    /// An absolute descriptor is converted to self-relative form. A self-relative one is taken
    /// apart and built again, not copied byte for byte: its parts are found by their offsets, and
    /// two may share a SID or have space between them, so no one span of bytes is the descriptor.
    /// Either way the result owns every byte it refers to.
    ///
    /// # Errors
    ///
    /// Returns a [`SecurityCaptureError`] when Windows rejects the descriptor
    /// as malformed, when the conversion fails, or, as
    /// [`SecurityCaptureFailure::Allocate`], when the memory for the copy cannot be allocated.
    ///
    /// # Safety
    ///
    /// `descriptor` must be non-null and point to a security descriptor that
    /// stays valid for the duration of this call, including everything an
    /// absolute descriptor points at.
    pub unsafe fn capture(descriptor: *const c_void) -> Result<Self, SecurityCaptureError> {
        let descriptor = descriptor.cast_mut();

        // SAFETY: the caller guarantees a live descriptor.
        if unsafe { IsValidSecurityDescriptor(descriptor) } == FALSE {
            return Err(SecurityCaptureError::last_os(
                SecurityCaptureFailure::InvalidDescriptor,
            ));
        }

        let mut control: SECURITY_DESCRIPTOR_CONTROL = 0;
        let mut revision: u32 = 0;
        // SAFETY: as above; both out-parameters point to writable storage.
        if unsafe { GetSecurityDescriptorControl(descriptor, &raw mut control, &raw mut revision) }
            == FALSE
        {
            return Err(SecurityCaptureError::last_os(
                SecurityCaptureFailure::ReadControl,
            ));
        }

        let blob = if control & SE_SELF_RELATIVE == 0 {
            unsafe { Self::convert_absolute(descriptor) }?
        } else {
            unsafe { Self::rebuild_self_relative(descriptor) }?
        };

        let captured = Self { blob };

        // SAFETY: the blob holds the self-relative descriptor Windows wrote into it.
        if unsafe { IsValidSecurityDescriptor(captured.as_ptr().cast_mut()) } == FALSE {
            return Err(SecurityCaptureError::last_os(
                SecurityCaptureFailure::InvalidCopy,
            ));
        }

        Ok(captured)
    }

    /// A self-relative descriptor rebuilt in memory this crate owns: taken apart into absolute form,
    /// which reads each part where its offset says, then made self-relative again.
    ///
    /// Not a copy of `GetSecurityDescriptorLength` bytes. That length counts the parts and says
    /// nothing about where they are: a descriptor whose owner and group share one SID reports more
    /// bytes than it occupies, and one with space before a part reports fewer than its last part
    /// ends at, so a copy would read past the source or name a part outside itself.
    ///
    /// # Safety
    ///
    /// `descriptor` must be a live, valid, self-relative descriptor.
    unsafe fn rebuild_self_relative(
        descriptor: PSECURITY_DESCRIPTOR,
    ) -> Result<AlignedBuf, SecurityCaptureError> {
        let mut header_size = 0_u32;
        let mut dacl_size = 0_u32;
        let mut sacl_size = 0_u32;
        let mut owner_size = 0_u32;
        let mut group_size = 0_u32;
        // SAFETY: null buffers with zero sizes are the documented way to ask for the sizes; the call
        // is expected to fail with `ERROR_INSUFFICIENT_BUFFER`, having written them.
        let sized = unsafe {
            MakeAbsoluteSD(
                descriptor,
                ptr::null_mut(),
                &raw mut header_size,
                ptr::null_mut(),
                &raw mut dacl_size,
                ptr::null_mut(),
                &raw mut sacl_size,
                ptr::null_mut(),
                &raw mut owner_size,
                ptr::null_mut(),
                &raw mut group_size,
            )
        };
        if sized != FALSE {
            // An absolute header is never zero bytes, so success with no buffer contradicts the
            // call's own contract; the sizes are not usable.
            return Err(SecurityCaptureError::new(
                SecurityCaptureFailure::SizeAbsolute,
                io::Error::from_raw_os_error(insufficient_buffer()),
            ));
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(insufficient_buffer()) {
            return Err(SecurityCaptureError::new(
                SecurityCaptureFailure::SizeAbsolute,
                error,
            ));
        }

        let mut header = allocate_part(header_size)?;
        let mut dacl = allocate_part(dacl_size)?;
        let mut sacl = allocate_part(sacl_size)?;
        let mut owner = allocate_part(owner_size)?;
        let mut group = allocate_part(group_size)?;
        // SAFETY: each buffer is writable for exactly the size Windows asked for, a part the
        // descriptor does not have is null with a size of zero, and all five outlive the call.
        let taken_apart = unsafe {
            MakeAbsoluteSD(
                descriptor,
                part_pointer(&mut header),
                &raw mut header_size,
                part_pointer(&mut dacl),
                &raw mut dacl_size,
                part_pointer(&mut sacl),
                &raw mut sacl_size,
                part_pointer(&mut owner),
                &raw mut owner_size,
                part_pointer(&mut group),
                &raw mut group_size,
            )
        };
        if taken_apart == FALSE {
            return Err(SecurityCaptureError::last_os(
                SecurityCaptureFailure::MakeAbsolute,
            ));
        }

        // SAFETY: `header` now holds a valid absolute descriptor whose parts point into the four
        // part buffers, all of which outlive this call.
        unsafe { Self::convert_absolute(header.as_mut_ptr().cast::<c_void>()) }
    }

    /// # Safety
    ///
    /// `descriptor` must be a live, valid, absolute descriptor, including
    /// everything it points at.
    unsafe fn convert_absolute(
        descriptor: PSECURITY_DESCRIPTOR,
    ) -> Result<AlignedBuf, SecurityCaptureError> {
        let mut length: u32 = 0;
        // SAFETY: a null destination with a zero length is the documented way
        // to ask for the required size; it is expected to fail.
        let sized = unsafe { MakeSelfRelativeSD(descriptor, ptr::null_mut(), &raw mut length) };
        if sized != FALSE {
            // Windows reported success without being given a buffer, which
            // contradicts its own contract; treat the size as unusable.
            return Err(SecurityCaptureError::new(
                SecurityCaptureFailure::SizeSelfRelative,
                io::Error::from_raw_os_error(
                    i32::try_from(ERROR_INSUFFICIENT_BUFFER)
                        .expect("ERROR_INSUFFICIENT_BUFFER fits in i32"),
                ),
            ));
        }

        let error = io::Error::last_os_error();
        if error.raw_os_error()
            != Some(
                i32::try_from(ERROR_INSUFFICIENT_BUFFER)
                    .expect("ERROR_INSUFFICIENT_BUFFER fits in i32"),
            )
        {
            return Err(SecurityCaptureError::new(
                SecurityCaptureFailure::SizeSelfRelative,
                error,
            ));
        }

        let mut blob = allocate(length as usize)?;
        // SAFETY: blob is writable for exactly the length Windows asked for.
        let converted = unsafe {
            MakeSelfRelativeSD(
                descriptor,
                blob.as_mut_ptr().cast::<c_void>(),
                &raw mut length,
            )
        };
        if converted == FALSE {
            return Err(SecurityCaptureError::last_os(
                SecurityCaptureFailure::MakeSelfRelative,
            ));
        }

        Ok(blob)
    }

    /// The captured descriptor's address, for handing to a Win32 call.
    ///
    /// The pointer borrows from this value and must not outlive it.
    #[must_use]
    pub fn as_ptr(&self) -> *const c_void {
        self.blob.as_ptr().cast::<c_void>()
    }

    /// The captured descriptor's length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.blob.len()
    }

    /// Whether the captured descriptor is empty, which a valid one never is.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blob.is_empty()
    }

    /// The captured descriptor's bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.blob.as_slice()
    }

    /// What the descriptor says about its discretionary access-control list.
    ///
    /// # Errors
    ///
    /// Returns a [`SecurityCaptureError`] if Windows will not report the list,
    /// which a descriptor that validated at capture should not do.
    pub fn dacl(&self) -> Result<AclState, SecurityCaptureError> {
        let mut present = FALSE;
        let mut acl: *mut ACL = ptr::null_mut();
        let mut defaulted = FALSE;

        // SAFETY: the blob holds a descriptor that validated at capture, and
        // all three out-parameters point to writable storage.
        let read = unsafe {
            GetSecurityDescriptorDacl(
                self.as_ptr().cast_mut(),
                &raw mut present,
                &raw mut acl,
                &raw mut defaulted,
            )
        };

        Self::acl_state(read, present, acl)
    }

    /// What the descriptor says about its system access-control list.
    ///
    /// # Errors
    ///
    /// As [`dacl`](Self::dacl).
    pub fn sacl(&self) -> Result<AclState, SecurityCaptureError> {
        let mut present = FALSE;
        let mut acl: *mut ACL = ptr::null_mut();
        let mut defaulted = FALSE;

        // SAFETY: as dacl.
        let read = unsafe {
            GetSecurityDescriptorSacl(
                self.as_ptr().cast_mut(),
                &raw mut present,
                &raw mut acl,
                &raw mut defaulted,
            )
        };

        Self::acl_state(read, present, acl)
    }

    fn acl_state(
        read: i32,
        present: i32,
        acl: *const ACL,
    ) -> Result<AclState, SecurityCaptureError> {
        if read == FALSE {
            return Err(SecurityCaptureError::last_os(
                SecurityCaptureFailure::ReadAcl,
            ));
        }

        if present != TRUE {
            return Ok(AclState::Absent);
        }

        // SAFETY: Windows either left the pointer null or pointed it at an ACL
        // inside the descriptor blob this method borrows.
        let Some(acl) = (unsafe { acl.as_ref() }) else {
            return Ok(AclState::Null);
        };

        Ok(match u32::from(acl.AceCount) {
            0 => AclState::Empty,
            count => AclState::Populated(count),
        })
    }
}

/// An owned capture of a caller's `lpSecurityAttributes` argument.
///
/// Three outcomes that a single nullable pointer runs together are kept apart
/// by this type and [`AclState`], because they are three different grants:
///
/// | Caller passed | Meaning | Represented as |
/// |---|---|---|
/// | `NULL` attributes | default security, non-inheritable handle | `None` where a `SecurityAttributes` is expected |
/// | attributes with a `NULL` descriptor | default security, caller's inheritance choice | [`descriptor`](Self::descriptor) is `None` |
/// | attributes with a descriptor | the caller's security | [`descriptor`](Self::descriptor) is `Some` |
///
/// and within a descriptor, an absent DACL, a NULL DACL, and an empty DACL are
/// three further distinct grants, reported by [`SecurityDescriptor::dacl`].
///
/// # Example
///
/// The distinction a single nullable pointer runs together. Passing no
/// attributes at all and passing attributes that carry no descriptor are
/// different requests -- the second still states an inheritance choice:
///
/// ```
/// use win_authorization_sys::SecurityAttributes;
///
/// // Attributes with no descriptor: default security, but the caller's
/// // inheritance choice is still carried.
/// let inheritable = SecurityAttributes::new(None, true);
/// assert!(inheritable.descriptor().is_none());
/// assert!(inheritable.inherit_handle());
///
/// let raw = inheritable.to_raw();
/// assert!(raw.lpSecurityDescriptor.is_null());
/// assert_ne!(raw.bInheritHandle, 0, "the choice survives into the Win32 struct");
///
/// // Passing *no* attributes is the third case, and is spelled `None` where a
/// // `SecurityAttributes` is expected rather than being confused with this.
/// let none: Option<SecurityAttributes> = None;
/// assert!(none.is_none());
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct SecurityAttributes {
    descriptor: Option<SecurityDescriptor>,
    inherit_handle: bool,
}

impl SecurityAttributes {
    /// Builds a capture from an already-owned descriptor and an inheritance
    /// choice.
    #[must_use]
    pub fn new(descriptor: Option<SecurityDescriptor>, inherit_handle: bool) -> Self {
        Self {
            descriptor,
            inherit_handle,
        }
    }

    /// Copies the attributes.
    ///
    /// Not `Clone`, because copying the descriptor allocates and the allocation can fail.
    ///
    /// # Errors
    ///
    /// Returns [`SecurityCaptureFailure::Allocate`] when the memory for the copy cannot be
    /// allocated.
    pub fn try_clone(&self) -> Result<Self, SecurityCaptureError> {
        Ok(Self {
            descriptor: self
                .descriptor
                .as_ref()
                .map(SecurityDescriptor::try_clone)
                .transpose()?,
            inherit_handle: self.inherit_handle,
        })
    }

    /// Captures the `SECURITY_ATTRIBUTES` at `attributes`.
    ///
    /// A null `attributes` is not an error: it is the caller declining to
    /// supply any, which is reported as `Ok(None)` rather than being confused
    /// with attributes that carry no descriptor.
    ///
    /// # Errors
    ///
    /// Returns a [`SecurityCaptureError`] when the referenced descriptor cannot
    /// be captured.
    ///
    /// # Safety
    ///
    /// `attributes`, if non-null, must point to a `SECURITY_ATTRIBUTES` that
    /// stays valid for the duration of this call, as must any descriptor it
    /// names.
    pub unsafe fn capture(
        attributes: *const SECURITY_ATTRIBUTES,
    ) -> Result<Option<Self>, SecurityCaptureError> {
        // SAFETY: the caller guarantees a live pointer or null.
        let Some(attributes) = (unsafe { attributes.as_ref() }) else {
            return Ok(None);
        };

        let descriptor = if attributes.lpSecurityDescriptor.is_null() {
            None
        } else {
            // SAFETY: non-null, and live for this call by the contract above.
            Some(unsafe { SecurityDescriptor::capture(attributes.lpSecurityDescriptor) }?)
        };

        Ok(Some(Self {
            descriptor,
            inherit_handle: attributes.bInheritHandle != FALSE,
        }))
    }

    /// The captured descriptor, if the caller supplied one.
    #[must_use]
    pub fn descriptor(&self) -> Option<&SecurityDescriptor> {
        self.descriptor.as_ref()
    }

    /// Whether the caller asked for the resulting handle to be inheritable.
    #[must_use]
    pub fn inherit_handle(&self) -> bool {
        self.inherit_handle
    }

    /// Rebuilds the `SECURITY_ATTRIBUTES` to pass to a Win32 call.
    ///
    /// The result points into this value and must not outlive it.
    #[must_use]
    pub fn to_raw(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>())
                .expect("SECURITY_ATTRIBUTES is far smaller than u32::MAX"),
            lpSecurityDescriptor: self
                .descriptor
                .as_ref()
                .map_or(ptr::null_mut(), |descriptor| descriptor.as_ptr().cast_mut()),
            bInheritHandle: if self.inherit_handle { TRUE } else { FALSE },
        }
    }
}

#[cfg(test)]
mod self_relative_tests;
#[cfg(test)]
mod tests;
