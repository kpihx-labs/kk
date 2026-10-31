// Copyright (c) 2026 Mike Grier; local K&K extension, MIT.
//! Synchronous, scoped named-pipe impersonation; no PID authentication.
use crate::{Sid, token::token_sid};
use std::{
    io,
    os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
    ptr,
};
use windows_sys::Win32::{
    Foundation::ERROR_NO_TOKEN,
    Security::{RevertToSelf, TOKEN_IMPERSONATE, TOKEN_QUERY},
    System::{
        Pipes::ImpersonateNamedPipeClient,
        Threading::{GetCurrentThread, OpenThreadToken, SetThreadToken},
    },
};

fn thread_token(access: u32) -> io::Result<Option<OwnedHandle>> {
    let mut raw = ptr::null_mut();
    // SAFETY: current thread pseudo-handle and live output; no context change.
    if unsafe { OpenThreadToken(GetCurrentThread(), access, 1, &mut raw) } == 0 {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_NO_TOKEN as i32) {
            Ok(None)
        } else {
            Err(error)
        };
    }
    if raw.is_null() {
        return Err(io::Error::other("null thread token"));
    }
    // SAFETY: successful OpenThreadToken transfers the real token handle.
    Ok(Some(unsafe { OwnedHandle::from_raw_handle(raw) }))
}

struct Restore(Option<OwnedHandle>);
impl Drop for Restore {
    fn drop(&mut self) {
        // SAFETY: saved token has TOKEN_IMPERSONATE and remains owned through
        // this call. A null thread pointer selects this same synchronous thread.
        let restored = unsafe {
            match &self.0 {
                Some(token) => SetThreadToken(ptr::null(), token.as_raw_handle()),
                None => RevertToSelf(),
            }
        };
        // Returning while impersonating a client would cross a security boundary.
        if restored == 0 {
            std::process::abort();
        }
    }
}

/// SID of the client whose message was last read from this connected pipe.
/// Caller must read its authentication preamble first and retain the connection.
/// The exact previous thread token is restored before returning or unwinding.
/// Anonymous/no-token contexts are errors, never process-token fallback. This
/// function performs no await/callback while impersonating; restoration failure
/// terminates the process rather than continuing under the wrong identity.
pub fn named_pipe_client_sid(pipe: BorrowedHandle<'_>) -> io::Result<Sid> {
    let previous = thread_token(TOKEN_QUERY | TOKEN_IMPERSONATE)?;
    let _restore = Restore(previous);
    // SAFETY: borrowed live handle; Windows validates that it names a server pipe.
    if unsafe { ImpersonateNamedPipeClient(pipe.as_raw_handle()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = thread_token(TOKEN_QUERY)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "pipe impersonation produced no client token",
        )
    })?;
    token_sid(&token)
}
