// Copyright (c) Mike Grier.

//! Tests for the `ReOpenFile` entry.
//!
//! The expectations are what this machine's Windows did, measured: which combinations conflict with
//! an open still open, and the codes it answers with, are the kernel's and are not restated here
//! from the documentation.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsHandle, AsRawHandle};

use io_buffers::AlignedBuf;
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, ERROR_SHARING_VIOLATION, GENERIC_READ,
    GENERIC_WRITE,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_FLAG_NO_BUFFERING, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use super::ReOpenFile;
use crate::Win32Error;
use crate::handle::SharedRaw;
use crate::handle::tests::{FILE_CONTENTS, Fixture, handle_allocation};
use crate::request::Request;
use win_shared_os_owned_handle::SharedHandle;

const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

fn request_for(file: &File) -> ReOpenFile {
    ReOpenFile::new(SharedHandle::from(
        file.as_handle()
            .try_clone_to_owned()
            .expect("own the handle"),
    ))
}

/// The file at the fixture's path, opened as `read` and `write` say, sharing as `share` says.
fn open(fixture: &Fixture, read: bool, write: bool, share: u32) -> File {
    OpenOptions::new()
        .read(read)
        .write(write)
        .share_mode(share)
        .open(fixture.file())
        .expect("open the fixture file")
}

fn reopen(file: &File, access: u32, share: u32, flags: u32) -> Result<File, Win32Error> {
    request_for(file)
        .with_desired_access(access)
        .with_share_mode(share)
        .with_flags_and_attributes(flags)
        .perform()
        .map(File::from)
}

fn code(outcome: Result<File, Win32Error>) -> u32 {
    outcome.expect_err("the call refuses this").code()
}

fn read_all(file: &mut File) -> Vec<u8> {
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(0)).expect("rewind");
    file.read_to_end(&mut bytes).expect("read");
    bytes
}

// --- The reopen -------------------------------------------------------------------------------

#[test]
fn a_reopened_handle_is_the_same_file_under_a_new_handle() {
    let _allocating = handle_allocation().read().expect("not poisoned");
    let fixture = Fixture::new("reopen-same");
    let original = open(&fixture, true, true, SHARE_ALL);

    let mut reopened = reopen(&original, GENERIC_READ, SHARE_ALL, 0).expect("reopen");

    assert_ne!(reopened.as_raw_handle(), original.as_raw_handle());
    assert_eq!(read_all(&mut reopened), FILE_CONTENTS);
}

#[test]
fn the_new_access_is_not_limited_to_the_originals() {
    // A read-only original, reopened for writing: the access is asked for anew, and what the file's
    // security and the sharing allow is what decides it.
    let _allocating = handle_allocation().read().expect("not poisoned");
    let fixture = Fixture::new("reopen-widen");
    let mut original = open(&fixture, true, false, SHARE_ALL);

    let mut writer = reopen(&original, GENERIC_WRITE, SHARE_ALL, 0).expect("reopen for writing");
    writer
        .write_all(b"X")
        .expect("write through the reopened handle");
    drop(writer);

    assert_eq!(read_all(&mut original)[0], b'X');
}

#[test]
fn the_same_request_performed_twice_gives_two_handles() {
    let _allocating = handle_allocation().read().expect("not poisoned");
    let fixture = Fixture::new("reopen-twice");
    let original = open(&fixture, true, false, SHARE_ALL);
    let request = request_for(&original)
        .with_desired_access(GENERIC_READ)
        .with_share_mode(SHARE_ALL);

    let mut first = File::from(request.perform().expect("first"));
    let mut second = File::from(request.perform().expect("second"));

    assert_ne!(first.as_raw_handle(), second.as_raw_handle());
    assert_eq!(read_all(&mut first), FILE_CONTENTS);
    assert_eq!(read_all(&mut second), FILE_CONTENTS);
}

#[test]
fn a_new_request_says_nothing_and_is_the_calls_to_answer() {
    let _allocating = handle_allocation().read().expect("not poisoned");
    let fixture = Fixture::new("reopen-nothing");
    let original = open(&fixture, true, false, SHARE_ALL);
    let request = request_for(&original);

    assert_eq!(request.desired_access(), 0);
    assert_eq!(request.share_mode(), 0);
    assert_eq!(request.flags_and_attributes(), 0);
    // No access, no sharing and no flags is a combination the call accepts when nothing open
    // conflicts with it.
    assert!(request.perform().is_ok());
}

// --- What conflicts with an open still open, and the kernel's own code --------------------------

#[test]
fn an_access_the_originals_sharing_does_not_allow_is_a_sharing_violation() {
    let _allocating = handle_allocation().read().expect("not poisoned");
    let fixture = Fixture::new("reopen-share-access");
    // The original shares reading only, so nothing else may write while it is open.
    let original = open(&fixture, true, false, FILE_SHARE_READ);

    assert_eq!(
        code(reopen(&original, GENERIC_WRITE, SHARE_ALL, 0)),
        ERROR_SHARING_VIOLATION
    );
    assert!(reopen(&original, GENERIC_READ, FILE_SHARE_READ, 0).is_ok());
}

#[test]
fn a_sharing_that_does_not_allow_the_originals_access_is_a_sharing_violation() {
    let _allocating = handle_allocation().read().expect("not poisoned");
    let fixture = Fixture::new("reopen-share-mode");
    // The original writes, so a handle that does not share writing conflicts with it.
    let original = open(&fixture, true, true, FILE_SHARE_READ | FILE_SHARE_WRITE);

    assert_eq!(
        code(reopen(&original, GENERIC_READ, FILE_SHARE_READ, 0)),
        ERROR_SHARING_VIOLATION
    );
    assert!(
        reopen(
            &original,
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            0
        )
        .is_ok()
    );
}

#[test]
fn sharing_nothing_conflicts_with_an_original_that_is_open_for_reading() {
    let _allocating = handle_allocation().read().expect("not poisoned");
    let fixture = Fixture::new("reopen-share-none");
    let original = open(&fixture, true, false, SHARE_ALL);

    assert_eq!(
        code(reopen(&original, GENERIC_READ, 0, 0)),
        ERROR_SHARING_VIOLATION
    );
    // Delete access is a different access, and the original shares deletion.
    assert!(reopen(&original, DELETE, SHARE_ALL, 0).is_ok());
}

// --- The flags are carried verbatim ------------------------------------------------------------

#[test]
fn a_reopen_can_add_no_buffering_to_a_handle_opened_without_it() {
    let _allocating = handle_allocation().read().expect("not poisoned");
    let fixture = Fixture::new("reopen-no-buffering");
    let mut buffered = open(&fixture, true, false, SHARE_ALL);
    let mut unaligned = [0_u8; 7];
    assert_eq!(
        buffered
            .read(&mut unaligned)
            .expect("a buffered read of any size"),
        7
    );

    let mut unbuffered = reopen(
        &buffered,
        FILE_GENERIC_READ,
        SHARE_ALL,
        FILE_FLAG_NO_BUFFERING,
    )
    .expect("reopen");

    // An unbuffered handle takes only transfers that are a multiple of the sector size, to an
    // aligned buffer: the unaligned one is refused, and an aligned one is not.
    let refused = unbuffered
        .read(&mut unaligned)
        .expect_err("an unaligned transfer is refused");
    assert_eq!(
        refused.raw_os_error(),
        Some(ERROR_INVALID_PARAMETER.cast_signed())
    );
    let mut aligned = AlignedBuf::zeroed(4096, 4096).expect("an aligned buffer");
    assert_eq!(
        unbuffered
            .read(aligned.as_mut_slice())
            .expect("an aligned transfer"),
        FILE_CONTENTS.len()
    );
    assert_eq!(&aligned.as_slice()[..FILE_CONTENTS.len()], FILE_CONTENTS);
}

#[test]
fn an_overlapped_reopen_is_accepted() {
    let _allocating = handle_allocation().read().expect("not poisoned");
    let fixture = Fixture::new("reopen-overlapped");
    let original = open(&fixture, true, false, SHARE_ALL);

    assert!(
        reopen(
            &original,
            FILE_GENERIC_READ,
            SHARE_ALL,
            FILE_FLAG_OVERLAPPED
        )
        .is_ok()
    );
}

#[test]
fn a_directory_handle_is_the_kernels_own_answer() {
    // Measured: every combination tried on a directory handle was access denied, with the backup
    // semantics flag and without, and with no access requested. Why is not established.
    let _allocating = handle_allocation().read().expect("not poisoned");
    let fixture = Fixture::new("reopen-directory");
    let directory = fixture.open_directory();

    assert_eq!(
        code(reopen(&directory, FILE_GENERIC_READ, SHARE_ALL, 0)),
        ERROR_ACCESS_DENIED
    );
}

// --- The request --------------------------------------------------------------------------------

#[test]
fn what_is_configured_reads_back() {
    let _allocating = handle_allocation().read().expect("not poisoned");
    let fixture = Fixture::new("reopen-configured");
    let original = open(&fixture, true, false, SHARE_ALL);

    let request = request_for(&original)
        .with_desired_access(0x0102_0304)
        .with_share_mode(0x0A)
        .with_flags_and_attributes(0x0500_0600);

    assert_eq!(request.desired_access(), 0x0102_0304);
    assert_eq!(request.share_mode(), 0x0A);
    assert_eq!(request.flags_and_attributes(), 0x0500_0600);
    assert_ne!(request.original().raw(), original.as_raw_handle());
}

#[test]
fn a_request_reopens_after_the_handle_it_was_built_from_is_gone() {
    let _allocating = handle_allocation().read().expect("not poisoned");
    let fixture = Fixture::new("reopen-outlives");
    let original = open(&fixture, true, false, SHARE_ALL);
    let request = request_for(&original)
        .with_desired_access(GENERIC_READ)
        .with_share_mode(SHARE_ALL);
    drop(original);

    let mut reopened = File::from(
        request
            .perform()
            .expect("reopen after the original is closed"),
    );

    assert_eq!(read_all(&mut reopened), FILE_CONTENTS);
}

#[test]
fn a_clone_keeps_every_parameter_and_shares_the_handle() {
    let _allocating = handle_allocation().read().expect("not poisoned");
    let fixture = Fixture::new("reopen-copy");
    let original = open(&fixture, true, false, SHARE_ALL);
    let request = request_for(&original)
        .with_desired_access(FILE_GENERIC_READ)
        .with_share_mode(SHARE_ALL)
        .with_flags_and_attributes(FILE_FLAG_NO_BUFFERING);

    let copy = request.clone();

    assert_eq!(copy.desired_access(), FILE_GENERIC_READ);
    assert_eq!(copy.share_mode(), SHARE_ALL);
    assert_eq!(copy.flags_and_attributes(), FILE_FLAG_NO_BUFFERING);
    assert!(copy.original().same_handle(request.original()));
    drop(request);
    drop(original);
    assert!(copy.perform().is_ok(), "the copy still reopens");
}

#[test]
fn a_request_performs_the_same_way_on_another_thread_and_through_the_trait() {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}
    assert_send::<ReOpenFile>();
    assert_sync::<ReOpenFile>();

    fn through_the_seam<
        R: Request<Output = std::os::windows::io::OwnedHandle, Error = Win32Error>,
    >(
        request: &R,
    ) -> Result<std::os::windows::io::OwnedHandle, Win32Error> {
        request.perform()
    }

    let _allocating = handle_allocation().read().expect("not poisoned");
    let fixture = Fixture::new("reopen-thread");
    let original = open(&fixture, true, false, SHARE_ALL);
    let request = request_for(&original)
        .with_desired_access(GENERIC_READ)
        .with_share_mode(SHARE_ALL);

    assert!(through_the_seam(&request).is_ok());
    let on_another = std::thread::spawn(move || request.perform().map(File::from))
        .join()
        .expect("the worker did not panic");
    assert_eq!(
        read_all(&mut on_another.expect("reopen on a worker")),
        FILE_CONTENTS
    );
}
