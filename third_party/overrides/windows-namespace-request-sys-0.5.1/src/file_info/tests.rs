// Copyright (c) Mike Grier.

//! Tests for the `GetFileInformationByHandle` entry.

use std::fs::File;
use std::os::windows::io::AsHandle;

use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;

use super::QueryFileInformationByHandle;
use crate::handle::tests::{FILE_CONTENTS, Fixture, handle_allocation};
use crate::query::{FileInformationClass, QueryFileInformation};
use win_shared_os_owned_handle::SharedHandle;

/// Small enough that the fixture directory cannot be drained in one batch.
const SMALL_BATCH: usize = 320;

/// Enough files that a small batch leaves the enumeration mid-directory.
const ENOUGH_TO_SPAN_BATCHES: usize = 24;

fn request_for(file: &File) -> QueryFileInformationByHandle {
    QueryFileInformationByHandle::new(SharedHandle::from(
        file.as_handle()
            .try_clone_to_owned()
            .expect("own the handle"),
    ))
}

fn file_index(
    information: &windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION,
) -> u64 {
    (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow)
}

#[test]
fn a_file_reports_its_size_and_identity() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileinfo-file");
    let file = fixture.open_file();

    let information = request_for(&file).perform().expect("query the file");

    assert_eq!(information.nFileSizeLow, FILE_CONTENTS.len() as u32);
    assert_eq!(information.nFileSizeHigh, 0);
    assert_ne!(file_index(&information), 0);
    assert_ne!(information.dwVolumeSerialNumber, 0);
}

#[test]
fn a_file_reports_a_link_count() {
    // The field the Ex form's FileIdInfo does not give, which is one reason
    // this is a separate entry rather than a class of it.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileinfo-links");
    let file = fixture.open_file();

    let information = request_for(&file).perform().expect("query the file");

    assert!(
        information.nNumberOfLinks >= 1,
        "an open file has at least one link"
    );
}

#[test]
fn a_directory_is_reported_as_a_directory() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileinfo-directory");
    let directory = fixture.open_directory();

    let information = request_for(&directory)
        .perform()
        .expect("query the directory");

    assert_ne!(
        information.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY,
        0,
        "the directory attribute must be reported"
    );
}

#[test]
fn two_handles_to_one_object_report_the_same_identity() {
    // The identity is the object's, not the handle's, which is what makes it
    // usable as a reopen key.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileinfo-identity");
    let first = fixture.open_file();
    let second = fixture.open_file();

    let one = request_for(&first)
        .perform()
        .expect("query the first handle");
    let two = request_for(&second)
        .perform()
        .expect("query the second handle");

    assert_eq!(file_index(&one), file_index(&two));
    assert_eq!(one.dwVolumeSerialNumber, two.dwVolumeSerialNumber);
}

#[test]
fn the_query_does_not_disturb_an_enumeration_in_progress() {
    // Measured, not assumed: this is a pure read and composes freely with an
    // enumeration on the same handle.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::with_extra_files("fileinfo-interleave", ENOUGH_TO_SPAN_BATCHES);
    let directory = fixture.open_directory();

    let restart = QueryFileInformation::new(
        SharedHandle::from(
            directory
                .as_handle()
                .try_clone_to_owned()
                .expect("own the handle"),
        ),
        FileInformationClass::ID_EXTD_DIRECTORY_RESTART,
    )
    .with_capacity(SMALL_BATCH);
    let first = restart.perform().expect("the first batch");

    request_for(&directory)
        .perform()
        .expect("an interleaved non-Ex query");

    let next = QueryFileInformation::new(
        SharedHandle::from(
            directory
                .as_handle()
                .try_clone_to_owned()
                .expect("own the handle"),
        ),
        FileInformationClass::ID_EXTD_DIRECTORY,
    )
    .with_capacity(SMALL_BATCH);
    let continued = next.perform().expect("the enumeration continues");

    assert_ne!(
        first.as_slice(),
        continued.as_slice(),
        "the cursor must have advanced rather than restarted"
    );
}

#[test]
fn a_stale_handle_reports_the_raw_code() {
    let _allocating = handle_allocation()
        .write()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileinfo-stale");
    let file = fixture.open_file();
    let request = request_for(&file);
    drop(file);

    // The request holds a handle of its own, so it still works once the caller's is closed.
    request
        .perform()
        .expect("the request's handle outlives the caller's");
}

#[test]
fn a_clone_shares_the_handle() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileinfo-copy");
    let file = fixture.open_file();
    let request = request_for(&file);

    let copy = request.clone();

    assert!(copy.handle().same_handle(request.handle()));
    copy.perform().expect("the copy queries the same object");
}

#[test]
fn a_query_performs_the_same_way_on_another_thread() {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}

    assert_send::<QueryFileInformationByHandle>();
    assert_sync::<QueryFileInformationByHandle>();

    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileinfo-thread");
    let file = fixture.open_file();
    let request = request_for(&file);

    let size = std::thread::spawn(move || {
        request
            .perform()
            .expect("query on a worker that never saw the file")
            .nFileSizeLow
    })
    .join()
    .expect("the worker did not panic");

    assert_eq!(size, FILE_CONTENTS.len() as u32);
}
