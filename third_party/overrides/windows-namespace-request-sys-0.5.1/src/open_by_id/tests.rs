// Copyright (c) Mike Grier.

//! Tests for the `OpenFileById` entry.
//!
//! These need a real file identifier, which means calling
//! `GetFileInformationByHandle` directly here. That call becomes entry 6 in
//! M26.2; until it does, reaching for it in a test is honest, whereas building
//! half of it early would prejudge that entry's shape.

use std::fs::File;
use std::os::windows::io::{AsHandle, AsRawHandle};

use windows_sys::Win32::Foundation::FALSE;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, ExtendedFileIdType, FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_READ,
    FILE_LIST_DIRECTORY, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdType,
    GetFileInformationByHandle, ObjectIdType,
};

use super::{FileIdentifier, OpenFileByIdentifier};
use crate::SecurityAttributes;
use crate::handle::tests::{FILE_CONTENTS, Fixture, handle_allocation};
use win_shared_os_owned_handle::SharedHandle;

const AUDITED_SHARE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// Reads a file's 64-bit reference number.
fn file_id_of(file: &File) -> u64 {
    // SAFETY: the handle is live for the call and the out-parameter is
    // writable.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let read = unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &raw mut info) };
    assert_ne!(read, FALSE, "read the fixture file's identity");

    (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow)
}

fn open_directory_for_hint(fixture: &Fixture) -> File {
    fixture.open_directory()
}

#[test]
fn an_object_is_reopened_by_its_identifier() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("byid-reopen");
    let file = fixture.open_file();
    let id = file_id_of(&file);
    let hint = open_directory_for_hint(&fixture);

    let reopened = OpenFileByIdentifier::new(
        SharedHandle::from(
            hint.as_handle()
                .try_clone_to_owned()
                .expect("own the volume hint"),
        ),
        FileIdentifier::FileId(id),
    )
    .with_desired_access(FILE_GENERIC_READ)
    .with_share_mode(AUDITED_SHARE)
    .perform()
    .expect("reopen the fixture file by id");

    assert_eq!(
        File::from(reopened)
            .metadata()
            .expect("read metadata")
            .len(),
        FILE_CONTENTS.len() as u64
    );
}

#[test]
fn the_reopen_survives_the_source_handle_being_closed_first() {
    // The property that makes reopen-by-id worth having: the request holds a
    // share of the *hint*, which is never the object being reopened, so the
    // handle the id came from can be gone entirely.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("byid-outlives");
    let hint = open_directory_for_hint(&fixture);

    let request = {
        let file = fixture.open_file();
        let id = file_id_of(&file);
        let request = OpenFileByIdentifier::new(
            SharedHandle::from(
                hint.as_handle()
                    .try_clone_to_owned()
                    .expect("own the volume hint"),
            ),
            FileIdentifier::FileId(id),
        )
        .with_desired_access(FILE_GENERIC_READ)
        .with_share_mode(AUDITED_SHARE);

        drop(file);
        request
    };
    drop(hint);

    let reopened = request
        .perform()
        .expect("the request outlives both the source handle and the hint it was built from");

    assert_eq!(
        File::from(reopened)
            .metadata()
            .expect("read metadata")
            .len(),
        FILE_CONTENTS.len() as u64
    );
}

#[test]
fn a_directory_is_reopened_in_the_audited_shape() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("byid-directory");
    let directory = fixture.open_directory();
    let id = file_id_of(&directory);

    let reopened = OpenFileByIdentifier::new(
        SharedHandle::from(
            directory
                .as_handle()
                .try_clone_to_owned()
                .expect("own the volume hint"),
        ),
        FileIdentifier::FileId(id),
    )
    .with_desired_access(FILE_LIST_DIRECTORY)
    .with_share_mode(AUDITED_SHARE)
    .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS)
    .perform()
    .expect("reopen the fixture directory by id");

    assert!(
        File::from(reopened)
            .metadata()
            .expect("read the directory metadata")
            .is_dir()
    );
}

#[test]
fn an_unknown_identifier_reports_the_raw_code() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("byid-unknown");
    let hint = open_directory_for_hint(&fixture);

    let outcome = OpenFileByIdentifier::new(
        SharedHandle::from(
            hint.as_handle()
                .try_clone_to_owned()
                .expect("own the volume hint"),
        ),
        // An id that names nothing on this volume.
        FileIdentifier::FileId(u64::MAX - 1),
    )
    .with_desired_access(FILE_GENERIC_READ)
    .with_share_mode(AUDITED_SHARE)
    .perform();

    assert!(
        outcome.is_err(),
        "an identifier naming nothing must fail, unaltered"
    );
}

#[test]
fn every_identifier_kind_carries_its_own_win32_tag() {
    // The tag is implied by the variant, so a caller cannot pair a GUID with
    // the file-id tag the way the raw union permits.
    assert_eq!(FileIdentifier::FileId(7).id_type(), FileIdType);
    assert_eq!(FileIdentifier::ObjectId(0).id_type(), ObjectIdType);
    assert_eq!(
        FileIdentifier::ExtendedFileId([0; 16]).id_type(),
        ExtendedFileIdType
    );
}

#[test]
fn all_three_identifier_kinds_are_expressible() {
    // Only FileId appears in the audited consumers. The other two are kept
    // because an entry that can express one of its call's three identifier
    // kinds is a narrowed OpenFileById.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("byid-kinds");
    let hint = open_directory_for_hint(&fixture);

    for identifier in [
        FileIdentifier::FileId(1),
        FileIdentifier::ObjectId(0x1234),
        FileIdentifier::ExtendedFileId([9; 16]),
    ] {
        let request = OpenFileByIdentifier::new(
            SharedHandle::from(
                hint.as_handle()
                    .try_clone_to_owned()
                    .expect("own the volume hint"),
            ),
            identifier,
        )
        .with_desired_access(FILE_GENERIC_READ)
        .with_share_mode(AUDITED_SHARE);

        assert_eq!(request.identifier(), identifier);
        // Each reaches Windows and is judged there; none is refused by this
        // crate for being an unaudited kind.
        let _ = request.perform();
    }
}

#[test]
fn a_new_request_defaults_nothing_and_has_no_creation_disposition() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("byid-defaults");
    let hint = open_directory_for_hint(&fixture);

    let request = OpenFileByIdentifier::new(
        SharedHandle::from(
            hint.as_handle()
                .try_clone_to_owned()
                .expect("own the volume hint"),
        ),
        FileIdentifier::FileId(1),
    );

    assert_eq!(request.desired_access(), 0);
    assert_eq!(request.share_mode(), 0);
    assert_eq!(request.flags_and_attributes(), 0);
    assert!(request.security().is_none());
    // There is deliberately no creation disposition to read back: OpenFileById
    // has none, which is one reason this is its own entry.
}

#[test]
fn a_copy_shares_the_volume_hint() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("byid-copy");
    let hint = open_directory_for_hint(&fixture);
    let request = OpenFileByIdentifier::new(
        SharedHandle::from(
            hint.as_handle()
                .try_clone_to_owned()
                .expect("own the volume hint"),
        ),
        FileIdentifier::FileId(1),
    );

    let copy = request.try_clone().expect("copy the request");

    assert!(
        copy.volume_hint().same_handle(request.volume_hint()),
        "the copy shares the original's hint"
    );
    assert_eq!(copy.identifier(), request.identifier());
}

#[test]
fn a_copy_keeps_the_security_attributes() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("byid-copy-security");
    let hint = open_directory_for_hint(&fixture);
    let request = OpenFileByIdentifier::new(
        SharedHandle::from(
            hint.as_handle()
                .try_clone_to_owned()
                .expect("own the volume hint"),
        ),
        FileIdentifier::FileId(1),
    )
    .with_security(Some(SecurityAttributes::new(None, true)));

    let copy = request.try_clone().expect("copy the request");

    assert!(copy.security().is_some(), "the copy kept its attributes");
    assert_eq!(copy.security(), request.security());
}

#[test]
fn a_request_performs_the_same_way_on_another_thread() {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}

    assert_send::<OpenFileByIdentifier>();
    assert_sync::<OpenFileByIdentifier>();

    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("byid-thread");
    let file = fixture.open_file();
    let id = file_id_of(&file);
    let hint = open_directory_for_hint(&fixture);

    let request = OpenFileByIdentifier::new(
        SharedHandle::from(
            hint.as_handle()
                .try_clone_to_owned()
                .expect("own the volume hint"),
        ),
        FileIdentifier::FileId(id),
    )
    .with_desired_access(FILE_GENERIC_READ)
    .with_share_mode(AUDITED_SHARE);

    let length = std::thread::spawn(move || {
        let reopened = request.perform().expect("reopen on a worker thread");
        File::from(reopened)
            .metadata()
            .expect("read metadata")
            .len()
    })
    .join()
    .expect("the worker did not panic");

    assert_eq!(length, FILE_CONTENTS.len() as u64);
}

#[test]
fn every_configured_parameter_reads_back_through_its_own_accessor() {
    // Four accessors -- `desired_access`, `share_mode`, `security`, and
    // `flags_and_attributes` -- all survived replacement by constants in a
    // mutation run. Every test above builds a request and then *performs* it,
    // and the perform path reads the struct's fields directly, so nothing
    // distinguished an accessor that reports the truth from one that does not.
    //
    // `OpenFileByIdentifier::new` starts every parameter at zero or `None`, so
    // the values here are deliberately non-zero and pairwise distinct: a zero
    // would be indistinguishable from the default, and a repeated value would
    // let one accessor read a neighbour's field undetected.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("byid-accessors");
    let file = fixture.open_file();
    let id = file_id_of(&file);
    let hint = open_directory_for_hint(&fixture);

    let request = OpenFileByIdentifier::new(
        SharedHandle::from(
            hint.as_handle()
                .try_clone_to_owned()
                .expect("own the volume hint"),
        ),
        FileIdentifier::FileId(id),
    )
    .with_desired_access(FILE_GENERIC_READ)
    .with_share_mode(FILE_SHARE_READ)
    .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS);

    assert_eq!(request.desired_access(), FILE_GENERIC_READ);
    assert_eq!(request.share_mode(), FILE_SHARE_READ);
    assert_eq!(request.flags_and_attributes(), FILE_FLAG_BACKUP_SEMANTICS);
    assert!(
        request.security().is_none(),
        "nothing was supplied, so nothing must be reported"
    );

    let configured = [
        FILE_GENERIC_READ,
        FILE_SHARE_READ,
        FILE_FLAG_BACKUP_SEMANTICS,
    ];
    for (index, value) in configured.iter().enumerate() {
        assert_ne!(*value, 0, "a zero is indistinguishable from the default");
        for other in &configured[index + 1..] {
            assert_ne!(
                value, other,
                "two parameters share a value, so this test cannot tell their \
                 accessors apart"
            );
        }
    }
}

#[test]
fn supplied_security_attributes_read_back_rather_than_reporting_none() {
    // `security -> None` is the one accessor whose default is already `None`,
    // so it needs the opposite case: attributes that were supplied must be
    // visible to a caller inspecting the request, not only to the open that
    // consumes it.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("byid-security");
    let file = fixture.open_file();
    let id = file_id_of(&file);
    let hint = open_directory_for_hint(&fixture);

    let request = OpenFileByIdentifier::new(
        SharedHandle::from(
            hint.as_handle()
                .try_clone_to_owned()
                .expect("own the volume hint"),
        ),
        FileIdentifier::FileId(id),
    )
    .with_security(Some(SecurityAttributes::new(None, false)));

    assert!(request.security().is_some());
}
