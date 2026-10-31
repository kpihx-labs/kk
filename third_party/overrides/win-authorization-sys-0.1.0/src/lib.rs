// Copyright (c) 2026 Mike Grier
#![cfg(windows)]
#![doc = include_str!("../README.md")]

mod pipe;
mod security;
mod snapshot;
mod token;
pub use pipe::named_pipe_client_sid;

pub use snapshot::{Ace, Dacl, SecuritySnapshot, Sid};
pub use token::effective_user_sid;

/// Read owner and DACL once, retaining their common native snapshot authority.
pub fn file_security_snapshot(
    handle: std::os::windows::io::BorrowedHandle<'_>,
) -> std::io::Result<SecuritySnapshot> {
    let descriptor = get_security_info(
        handle,
        ObjectType::FILE,
        SecurityInformation::OWNER | SecurityInformation::DACL,
    )
    .map_err(|error| error.into_source())?;
    SecuritySnapshot::from_bytes(descriptor.as_bytes())
}

impl SecurityDescriptor {
    /// Explicit owner and a protected, noninheriting owner-only full-access DACL.
    pub fn owner_only(owner: &Sid) -> Result<Self, SecurityCaptureError> {
        let sid = owner.to_sddl();
        Self::from_sddl(&wtf_string::Wtf16String::from(format!(
            "O:{sid}D:P(A;;FA;;;{sid})"
        )))
    }
}

pub use security::{
    AclState, ObjectType, SecurityAttributes, SecurityCaptureError, SecurityCaptureFailure,
    SecurityDescriptor, SecurityInformation, get_security_info, get_security_info_unchecked,
};
