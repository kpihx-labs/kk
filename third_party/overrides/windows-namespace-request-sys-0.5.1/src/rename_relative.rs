// Copyright (c) 2026 Mike Grier; local K&K extension, MIT.
//! Source-handle, target-directory-handle exclusive rename; no pathname reopen.
use crate::create_relative::{RelativeFile, complete_retaining, component};
use std::{
    ffi::OsStr,
    fs::File,
    io, mem,
    os::windows::{
        fs::MetadataExt,
        io::{AsRawHandle, BorrowedHandle},
    },
    ptr,
};
use windows_sys::{
    Wdk::Storage::FileSystem::{
        FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
    },
    Win32::System::IO::IO_STATUS_BLOCK,
};

impl RelativeFile {
    /// Atomically move this synchronous source file into a retained directory.
    /// The kernel must fail if the target exists; no replacement or path fallback.
    /// The source must have DELETE access, supplied by create_file_relative.
    pub fn rename_no_replace(
        &mut self,
        directory: BorrowedHandle<'_>,
        leaf: &OsStr,
    ) -> io::Result<()> {
        let parent = File::from(directory.try_clone_to_owned()?);
        let metadata = parent.metadata()?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "rename target must be a non-reparse directory handle",
            ));
        }
        let name = component(leaf)?;
        let offset = mem::offset_of!(FILE_RENAME_INFORMATION, FileName);
        let length = (offset + name.len() * 2).max(mem::size_of::<FILE_RENAME_INFORMATION>());
        let mut buffer = vec![0usize; length.div_ceil(mem::size_of::<usize>())].into_boxed_slice();
        let information = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        // SAFETY: usize storage supplies pointer alignment, is zero-initialized,
        // and covers both the full fixed structure and every UTF-16 name unit.
        // The zero ReplaceIfExists/Flags union forbids replacement. No reference
        // is formed to the trailing one-element placeholder as a longer slice.
        unsafe {
            ptr::addr_of_mut!((*information).RootDirectory).write(parent.as_raw_handle());
            ptr::addr_of_mut!((*information).FileNameLength).write((name.len() * 2) as u32);
            ptr::copy_nonoverlapping(
                name.as_ptr(),
                ptr::addr_of_mut!((*information).FileName).cast::<u16>(),
                name.len(),
            );
        }
        let mut status_block = Box::new(IO_STATUS_BLOCK::default());
        // SAFETY: RelativeFile can only be constructed with synchronous native
        // IO mode. The source remains borrowed exclusively; owned input storage,
        // target handle and IOSB remain live until completion, including pending.
        let status = unsafe {
            NtSetInformationFile(
                self.as_file().as_raw_handle(),
                &mut *status_block,
                buffer.as_ptr().cast(),
                length as u32,
                FileRenameInformation,
            )
        };
        complete_retaining(status, self.as_file(), status_block, Some((buffer, parent)))?;
        Ok(())
    }
}
