// Copyright (c) 2026 Mike Grier; local K&K extension, MIT.
//! The effective caller SID, copied out before the owned token/buffer are dropped.
use crate::Sid;
use std::{
    io, mem,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr, slice,
};
use windows_sys::Win32::{
    Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_NO_TOKEN},
    Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser},
    System::Threading::{GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken},
};

/// Read the thread token, or the process token only when no thread token exists.
/// An access-denied thread-token query is never silently downgraded.
pub fn effective_user_sid() -> io::Result<Sid> {
    let mut raw = ptr::null_mut();
    // SAFETY: valid pseudo-thread handle, live out pointer, no impersonation change.
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut raw) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_NO_TOKEN as i32) {
            return Err(error);
        }
        // SAFETY: valid pseudo-process handle and live out pointer.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
            return Err(io::Error::last_os_error());
        }
    }
    if raw.is_null() {
        return Err(io::Error::other("token query returned a null handle"));
    }
    // SAFETY: successful Open*Token transferred this real token handle to us.
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    token_sid(&token)
}

pub(crate) fn token_sid(token: &OwnedHandle) -> io::Result<Sid> {
    let mut required = 0;
    // SAFETY: null/zero buffer is the documented size query; required is writable.
    let first = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut required,
        )
    };
    if first != 0 {
        return Err(io::Error::other("invalid TokenUser size query"));
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32) {
        return Err(error);
    }
    if required as usize > 65536 || (required as usize) < mem::size_of::<TOKEN_USER>() {
        return Err(io::Error::other("TokenUser size refused"));
    }
    let mut buffer = vec![0usize; (required as usize).div_ceil(mem::size_of::<usize>())];
    let capacity = required;
    // SAFETY: usize storage is pointer-aligned and has at least capacity bytes.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            capacity,
            &mut required,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if required > capacity || (required as usize) < mem::size_of::<TOKEN_USER>() {
        return Err(io::Error::other("TokenUser result size refused"));
    }
    // SAFETY: aligned storage, at least one complete TOKEN_USER was returned.
    let user = unsafe { ptr::read(buffer.as_ptr().cast::<TOKEN_USER>()) };
    let start = buffer.as_ptr() as usize;
    let end = start
        .checked_add(required as usize)
        .ok_or_else(|| io::Error::other("token range overflow"))?;
    let sid = user.User.Sid as usize;
    if sid < start || sid.checked_add(8).is_none_or(|n| n > end) {
        return Err(io::Error::other("TokenUser SID lies outside its buffer"));
    }
    // SAFETY: the header lies inside the live owned allocation, checked above.
    let header = unsafe { slice::from_raw_parts(sid as *const u8, 8) };
    let length = 8 + 4 * header[1] as usize;
    if sid.checked_add(length).is_none_or(|n| n > end) {
        return Err(io::Error::other("TokenUser SID is truncated"));
    }
    // SAFETY: the entire slice lies in the allocation; Sid copies and validates it.
    Sid::from_bytes(unsafe { slice::from_raw_parts(sid as *const u8, length) })
}
