// Copyright (c) 2026 Mike Grier; local K&K extension, MIT.
//! Explicit opt-in Tokio delivery; native handle/attribute ownership stays here.
use std::{
    ffi::OsStr,
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, BorrowedHandle},
    },
    ptr,
};
use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer};
use win_authorization_sys::{
    SecurityAttributes, SecuritySnapshot, Sid, file_security_snapshot, named_pipe_client_sid,
};
use windows_sys::Win32::{
    Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{
        CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
        PIPE_ACCESS_DUPLEX, READ_CONTROL, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
    },
    System::Pipes::{
        CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
    },
};

fn local_name(name: &OsStr) -> io::Result<Vec<u16>> {
    let text: Vec<u16> = name.encode_wide().collect();
    let prefix: Vec<u16> = r"\\.\pipe\".encode_utf16().collect();
    let leaf = text.strip_prefix(prefix.as_slice()).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "only local named pipes are accepted",
        )
    })?;
    if leaf.is_empty()
        || leaf.len() > 200
        || leaf.iter().any(|c| !matches!(*c, 45 | 48..=57 | 97..=122))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid private pipe component",
        ));
    }
    let mut terminated = text;
    terminated.push(0);
    Ok(terminated)
}

/// Create exactly one local duplex instance with explicit noninheritable security.
/// FIRST_PIPE_INSTANCE excludes adoption; the caller must retain this instance
/// across disconnect/reconnect rather than introducing a namespace gap.
pub fn create_server(name: &OsStr, security: &SecurityAttributes) -> io::Result<NamedPipeServer> {
    tokio::runtime::Handle::try_current()
        .map_err(|_| io::Error::other("Tokio IO runtime required"))?;
    if security.inherit_handle() || security.descriptor().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "explicit noninheritable pipe security required",
        ));
    }
    let name = local_name(name)?;
    let attributes = security.to_raw();
    // SAFETY: terminated local name and owned live security descriptor. The API
    // captures attributes synchronously and returns one new OVERLAPPED handle.
    let raw = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            4096,
            0,
            &attributes,
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fresh, unassociated, overlapped server handle; Tokio takes ownership
    // even when registration fails. Caller must enable the runtime IO driver.
    unsafe { NamedPipeServer::from_raw_handle(raw) }
}

/// Connect only to a local pipe. Identification-level SQOS prevents a server
/// acting with client credentials; owner/DACL must be checked before any payload.
pub fn open_client(name: &OsStr) -> io::Result<NamedPipeClient> {
    tokio::runtime::Handle::try_current()
        .map_err(|_| io::Error::other("Tokio IO runtime required"))?;
    let name = local_name(name)?;
    // SAFETY: live terminated name, OPEN_EXISTING cannot create a pipe, no template
    // or security pointer. All returned handles are noninheritable/OVERLAPPED.
    let raw = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE | READ_CONTROL,
            0,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: owned, unassociated, overlapped client handle; transferred to Tokio.
    unsafe { NamedPipeClient::from_raw_handle(raw) }
}

/// Security of this connected server handle, never a pathname security lookup.
pub fn server_security(pipe: &NamedPipeServer) -> io::Result<SecuritySnapshot> {
    // SAFETY: the concrete Tokio owner retains this handle throughout the borrow.
    file_security_snapshot(unsafe { BorrowedHandle::borrow_raw(pipe.as_raw_handle()) })
}
/// Security of this connected client handle, checked before disclosing payload.
pub fn client_security(pipe: &NamedPipeClient) -> io::Result<SecuritySnapshot> {
    // SAFETY: the concrete Tokio owner retains this handle throughout the borrow.
    file_security_snapshot(unsafe { BorrowedHandle::borrow_raw(pipe.as_raw_handle()) })
}
/// Connected client's last-read token SID, with exact synchronous restoration.
pub fn client_sid(pipe: &NamedPipeServer) -> io::Result<Sid> {
    // SAFETY: concrete live server handle, not a borrowed/reused process id.
    named_pipe_client_sid(unsafe { BorrowedHandle::borrow_raw(pipe.as_raw_handle()) })
}
