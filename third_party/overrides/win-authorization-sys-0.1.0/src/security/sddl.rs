// Copyright (c) Mike Grier.
// Split from crates/windows-namespace-request-sys/src/security/sddl.rs at 11020848.

//! SDDL text to and from a [`SecurityDescriptor`].
//!
//! Both directions are public, so a caller can build a descriptor from the text form and read one
//! back as text, and neither needs a raw pointer. The conversion is the documented
//! `ConvertStringSecurityDescriptorToSecurityDescriptorW` and its reverse,
//! `ConvertSecurityDescriptorToStringSecurityDescriptorW`; what they return is memory Windows
//! allocated with `LocalAlloc`, which is copied into this crate's own owned type or string and then
//! freed with `LocalFree`, so nothing the caller holds points at it.
//!
//! The text form is not canonical: two strings can describe the same descriptor, and a descriptor
//! read back as text is Windows' rendering of it, not necessarily the string it was built from.

use std::ops::BitOr;
use std::ptr;
use std::slice;

use win_error_codes::Win32Error;
use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, FALSE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, LABEL_SECURITY_INFORMATION,
    OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    SACL_SECURITY_INFORMATION,
};
use wtf_string::Wtf16String;

use super::{SecurityCaptureError, SecurityCaptureFailure, SecurityDescriptor};

#[cfg(test)]
mod tests;

/// Which parts of a descriptor to write as text.
///
/// A newtype over `OBJECT_SECURITY_INFORMATION`, because the value is a combination and Windows
/// defines more bits than are named here. Nothing is defaulted on a caller's behalf: a mask that
/// selects nothing is Windows' to answer.
///
/// # Example
///
/// ```
/// use win_authorization_sys::SecurityInformation;
///
/// let both = SecurityInformation::OWNER | SecurityInformation::DACL;
/// assert!(both.contains(SecurityInformation::OWNER));
/// assert!(both.contains(SecurityInformation::DACL));
/// assert!(!both.contains(SecurityInformation::SACL));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SecurityInformation(OBJECT_SECURITY_INFORMATION);

impl SecurityInformation {
    /// No part: what Windows makes of an empty selection is its to say.
    pub const NONE: Self = Self(0);

    /// `OWNER_SECURITY_INFORMATION`: the owner SID.
    pub const OWNER: Self = Self(OWNER_SECURITY_INFORMATION);

    /// `GROUP_SECURITY_INFORMATION`: the primary group SID.
    pub const GROUP: Self = Self(GROUP_SECURITY_INFORMATION);

    /// `DACL_SECURITY_INFORMATION`: the discretionary access-control list.
    pub const DACL: Self = Self(DACL_SECURITY_INFORMATION);

    /// `SACL_SECURITY_INFORMATION`: the system access-control list.
    pub const SACL: Self = Self(SACL_SECURITY_INFORMATION);

    /// `LABEL_SECURITY_INFORMATION`: the mandatory integrity label.
    pub const LABEL: Self = Self(LABEL_SECURITY_INFORMATION);

    /// A selection from its raw bits, for a part Windows defines that this crate has no name for.
    #[must_use]
    pub const fn from_bits(bits: OBJECT_SECURITY_INFORMATION) -> Self {
        Self(bits)
    }

    /// The raw bits.
    #[must_use]
    pub const fn bits(self) -> OBJECT_SECURITY_INFORMATION {
        self.0
    }

    /// Whether every bit of `other` is set. Everything contains [`NONE`](Self::NONE).
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for SecurityInformation {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Memory Windows allocated with `LocalAlloc` and handed to this crate, freed when it goes out of
/// scope, so no return path can leak it.
pub(super) struct LocalAllocation(pub(super) *mut core::ffi::c_void);

impl Drop for LocalAllocation {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer is one Windows allocated with `LocalAlloc` for this crate to
            // free, and it is freed exactly once, here.
            unsafe { LocalFree(self.0) };
        }
    }
}

impl SecurityDescriptor {
    /// A descriptor from its SDDL text, such as `D:P(A;;FA;;;WD)`.
    ///
    /// The result is the self-relative form this type always holds, copied out of the memory
    /// Windows allocated for it, which is then freed.
    ///
    /// The string is not interpreted here: Windows parses it, accepts what it defines and refuses
    /// the rest, and a string with no parts, such as an empty one, is its to answer. A string with
    /// an interior NUL is refused before the call, because Windows would read only up to it and
    /// build a different descriptor than the string says.
    ///
    /// # Example
    ///
    /// A DACL granting everyone full access, read back through the descriptor:
    ///
    /// ```
    /// use win_authorization_sys::{AclState, SecurityDescriptor};
    /// use wtf_string::Wtf16String;
    ///
    /// let descriptor = SecurityDescriptor::from_sddl(&Wtf16String::from("D:P(A;;GA;;;WD)"))?;
    /// assert_eq!(descriptor.dacl()?, AclState::Populated(1));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`SecurityCaptureFailure::InvalidSddl`] with Windows' own code, unaltered, when it refuses
    /// the string; [`SecurityCaptureFailure::InteriorNul`] for a string with an interior NUL; or
    /// the failures of capturing the descriptor Windows returned, including
    /// [`SecurityCaptureFailure::Allocate`] when the memory for the copy cannot be allocated.
    pub fn from_sddl(sddl: &Wtf16String) -> Result<Self, SecurityCaptureError> {
        if sddl.has_interior_nul() {
            return Err(SecurityCaptureError::new(
                SecurityCaptureFailure::InteriorNul,
                Win32Error::from_code(ERROR_INVALID_PARAMETER).to_io_error(),
            ));
        }

        let mut raw: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: the string is terminated and has no interior NUL, and is borrowed for the call;
        // `raw` is a writable pointer for the descriptor, and a null size is accepted.
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_terminated_ptr(),
                SDDL_REVISION_1,
                &raw mut raw,
                ptr::null_mut(),
            )
        };
        if converted == FALSE {
            return Err(SecurityCaptureError::last_os(
                SecurityCaptureFailure::InvalidSddl,
            ));
        }

        let allocation = LocalAllocation(raw);
        // SAFETY: Windows returned a valid self-relative descriptor in memory that stays allocated
        // until `allocation` is dropped, after the capture has copied out of it.
        let captured = unsafe { Self::capture(allocation.0) };
        drop(allocation);
        captured
    }

    /// The descriptor's parts selected by `information`, as SDDL text.
    ///
    /// Windows' rendering of the descriptor, which is not necessarily the string it was built
    /// from: the text form is not canonical.
    ///
    /// # Example
    ///
    /// ```
    /// use win_authorization_sys::{SecurityDescriptor, SecurityInformation};
    /// use wtf_string::Wtf16String;
    ///
    /// let descriptor = SecurityDescriptor::from_sddl(&Wtf16String::from("D:P(A;;GA;;;WD)"))?;
    /// let text = descriptor.to_sddl(SecurityInformation::DACL)?;
    /// assert_eq!(text.to_string_lossy(), "D:P(A;;GA;;;WD)");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// The length Windows reports for the text is not used. It is documented to count the
    /// terminator, and was measured to be the string's length plus one, plus two, and plus none,
    /// depending on the string; the text is read up to the terminator the documentation promises.
    ///
    /// # Errors
    ///
    /// [`SecurityCaptureFailure::ToSddl`] with Windows' own code, unaltered, when it refuses.
    pub fn to_sddl(
        &self,
        information: SecurityInformation,
    ) -> Result<Wtf16String, SecurityCaptureError> {
        let mut text: *mut u16 = ptr::null_mut();
        // SAFETY: the descriptor validated at capture and is borrowed for the call; the text
        // pointer is writable, and a null length is accepted.
        let converted = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                self.as_ptr().cast_mut(),
                SDDL_REVISION_1,
                information.bits(),
                &raw mut text,
                ptr::null_mut(),
            )
        };
        if converted == FALSE {
            return Err(SecurityCaptureError::last_os(
                SecurityCaptureFailure::ToSddl,
            ));
        }

        let allocation = LocalAllocation(text.cast());
        let mut count = 0_usize;
        // SAFETY: Windows returned a NUL-terminated string, which stays allocated until
        // `allocation` is dropped, after this has been copied; the read stops at the terminator.
        while unsafe { *text.add(count) } != 0 {
            count += 1;
        }
        // SAFETY: the `count` code units before the terminator were just read.
        let units = unsafe { slice::from_raw_parts(text, count) };
        let result = Wtf16String::from_units(units);
        drop(allocation);
        Ok(result)
    }
}
