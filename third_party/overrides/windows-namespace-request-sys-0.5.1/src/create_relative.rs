// Copyright (c) 2026 Mike Grier; local K&K extension, MIT.
//! Exclusive directory creation in a retained filesystem directory, not a DOS path.
use std::{
    ffi::OsStr,
    fs::File,
    io, mem,
    os::windows::{
        ffi::OsStrExt,
        fs::MetadataExt,
        io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle},
    },
    ptr,
};
use win_authorization_sys::SecurityAttributes;
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN,
            FILE_OPEN_REPARSE_POINT, FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
        },
    },
    Win32::{
        Foundation::{
            OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, OBJ_INHERIT, RtlNtStatusToDosError,
            STATUS_PENDING, UNICODE_STRING, WAIT_OBJECT_0,
        },
        Storage::FileSystem::{
            FILE_ALL_ACCESS, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        },
        System::{
            IO::IO_STATUS_BLOCK,
            Threading::{INFINITE, WaitForSingleObject},
        },
    },
};

/// Create exactly one new directory relative to `parent`. No existing object is
/// adopted or replaced; reparse traversal is refused. The returned handle grants
/// full access with directory read/write/delete sharing. Windows rename opens its
/// target directory internally; denying that open self-conflicts with publication.
/// Access is enforced by the supplied descriptor; regular-file sharing stays read-only.
/// The security descriptor is applied by NtCreateFile, never by a later chmod.
/// The caller supplies the parent object authority; no path bootstrap is implied.
pub fn create_directory_relative(
    parent: BorrowedHandle<'_>,
    leaf: &OsStr,
    security: &SecurityAttributes,
) -> io::Result<OwnedHandle> {
    open_relative(parent, leaf, Some(security), true, true)
}

/// Open one existing directory by parent capability, without creation or repair.
pub fn open_directory_relative(
    parent: BorrowedHandle<'_>,
    leaf: &OsStr,
) -> io::Result<OwnedHandle> {
    open_relative(parent, leaf, None, true, false)
}

/// A regular file opened synchronously by this module, never an arbitrary async
/// handle. This typestate keeps rename buffers valid through native completion.
pub struct RelativeFile(File);
impl RelativeFile {
    /// Retained synchronous file authority; no pathname is consulted.
    pub fn as_file(&self) -> &File {
        &self.0
    }
}

/// Exclusively create an owner-described regular file; never truncate existing data.
pub fn create_file_relative(
    parent: BorrowedHandle<'_>,
    leaf: &OsStr,
    security: &SecurityAttributes,
) -> io::Result<RelativeFile> {
    open_relative(parent, leaf, Some(security), false, true)
        .map(|handle| RelativeFile(File::from(handle)))
}

/// Open an existing non-directory for bounded reads with no write/delete sharing.
pub fn open_file_relative(parent: BorrowedHandle<'_>, leaf: &OsStr) -> io::Result<RelativeFile> {
    open_relative(parent, leaf, None, false, false).map(|handle| RelativeFile(File::from(handle)))
}

pub(crate) fn component(leaf: &OsStr) -> io::Result<Vec<u16>> {
    let name: Vec<u16> = leaf.encode_wide().collect();
    if name.is_empty()
        || name.len() > 255
        || name == [46]
        || name == [46, 46]
        || name.iter().any(|c| matches!(*c, 0 | 47 | 58 | 92))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected one literal component",
        ));
    }
    Ok(name)
}

/// Check a literal relative component before any staging side effect.
pub fn validate_relative_component(leaf: &OsStr) -> io::Result<()> {
    component(leaf).map(|_| ())
}

fn open_relative(
    parent: BorrowedHandle<'_>,
    leaf: &OsStr,
    security: Option<&SecurityAttributes>,
    directory: bool,
    create: bool,
) -> io::Result<OwnedHandle> {
    let parent_file = File::from(parent.try_clone_to_owned()?);
    let metadata = parent_file.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "parent must be a non-reparse directory handle",
        ));
    }
    let mut name = component(leaf)?;
    let descriptor = match security {
        Some(attributes) => attributes
            .descriptor()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "explicit creation security required",
                )
            })?
            .as_ptr()
            .cast(),
        None if !create => ptr::null(),
        None => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "explicit creation security required",
            ));
        }
    };
    let text = UNICODE_STRING {
        Length: (name.len() * 2) as u16,
        MaximumLength: (name.len() * 2) as u16,
        Buffer: name.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &text,
        Attributes: OBJ_CASE_INSENSITIVE
            | OBJ_DONT_REPARSE
            | if security.is_some_and(|s| s.inherit_handle()) {
                OBJ_INHERIT
            } else {
                0
            },
        SecurityDescriptor: descriptor,
        SecurityQualityOfService: ptr::null(),
    };
    let mut raw = ptr::null_mut();
    let mut status_block = Box::new(IO_STATUS_BLOCK::default());
    // SAFETY: all input descriptors/UTF-16 storage and the borrowed parent remain
    // live throughout this synchronous call. IOSB is owned heap storage, including
    // the defensive pending path below. No caller-controlled raw pointer enters.
    let status = unsafe {
        NtCreateFile(
            &mut raw,
            if create || directory {
                FILE_ALL_ACCESS
            } else {
                FILE_GENERIC_READ
            },
            &attributes,
            &mut *status_block,
            ptr::null(),
            if directory {
                FILE_ATTRIBUTE_DIRECTORY
            } else {
                FILE_ATTRIBUTE_NORMAL
            },
            if directory {
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
            } else {
                FILE_SHARE_READ
            },
            if create { FILE_CREATE } else { FILE_OPEN },
            (if directory {
                FILE_DIRECTORY_FILE
            } else {
                FILE_NON_DIRECTORY_FILE
            }) | FILE_SYNCHRONOUS_IO_NONALERT
                | FILE_OPEN_REPARSE_POINT,
            ptr::null(),
            0,
        )
    };
    if status < 0 {
        // SAFETY: status conversion reads no caller memory.
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ));
    }
    if raw.is_null() || raw as isize == -1 {
        if status == STATUS_PENDING {
            Box::leak(status_block);
        }
        return Err(io::Error::other("NtCreateFile returned no owned handle"));
    }
    // SAFETY: NtCreateFile has returned a real new handle exclusively owned here.
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    let status_block = complete(status, &handle, status_block)?;
    if status_block.Information != if create { 2 } else { 1 } {
        return Err(io::Error::other(
            "unexpected native create/open disposition",
        ));
    }
    Ok(handle)
}

pub(crate) fn complete(
    status: i32,
    handle: &impl AsRawHandle,
    status_block: Box<IO_STATUS_BLOCK>,
) -> io::Result<Box<IO_STATUS_BLOCK>> {
    complete_retaining(status, handle, status_block, None)
}

pub(crate) fn complete_retaining(
    mut status: i32,
    handle: &impl AsRawHandle,
    status_block: Box<IO_STATUS_BLOCK>,
    mut rename_input: Option<(Box<[usize]>, File)>,
) -> io::Result<Box<IO_STATUS_BLOCK>> {
    if status == STATUS_PENDING {
        // Normally excluded by synchronous IO options; retain IOSB until the
        // kernel completes even if a filesystem nevertheless returns pending.
        // SAFETY: the owned handle has SYNCHRONIZE access (FILE_ALL_ACCESS).
        if unsafe { WaitForSingleObject(handle.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0 {
            let error = io::Error::last_os_error();
            // A failed wait must never free memory a pending kernel IO may use.
            Box::leak(status_block);
            if let Some(input) = rename_input.take() {
                mem::forget(input);
            }
            return Err(error);
        }
        // SAFETY: completion makes the Status member initialized and stable.
        status = unsafe { status_block.Anonymous.Status };
        if status == STATUS_PENDING {
            Box::leak(status_block);
            if let Some(input) = rename_input.take() {
                mem::forget(input);
            }
            return Err(io::Error::other("native IO did not complete"));
        }
    }
    if status < 0 {
        // SAFETY: integer status conversion, no borrowed memory.
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ));
    }
    Ok(status_block)
}
