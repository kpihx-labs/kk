// Copyright (c) Mike Grier.

//! Tests for the typed `FileIdInfo` entry.

use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsHandle, FromRawHandle, OwnedHandle};
use std::process::{Command, Stdio};

use windows_sys::Win32::Foundation::ERROR_INVALID_HANDLE;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ID_EXTD_DIR_INFO, FILE_ID_INFO, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::Threading::CreateEventW;

use super::{FileIdInfo, QueryFileIdInfo};
use crate::handle::tests::{Fixture, handle_allocation};
use crate::query::{FileInformationClass, QueryFileInformation};
use crate::request::Request;
use win_shared_os_owned_handle::SharedHandle;

fn request_for(handle: &impl AsHandle) -> QueryFileIdInfo {
    QueryFileIdInfo::new(SharedHandle::from(
        handle
            .as_handle()
            .try_clone_to_owned()
            .expect("own the handle"),
    ))
}

fn identity_of(handle: &impl AsHandle) -> FileIdInfo {
    request_for(handle).perform().expect("query the identity")
}

const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// The code the untyped entry reports for the same class on the same handle.
fn untyped_code(handle: &impl AsHandle) -> u32 {
    QueryFileInformation::new(
        SharedHandle::from(
            handle
                .as_handle()
                .try_clone_to_owned()
                .expect("own the handle"),
        ),
        FileInformationClass::ID,
    )
    .with_capacity(size_of::<FILE_ID_INFO>())
    .perform()
    .expect_err("the untyped entry is refused too")
    .code()
}

/// The typed entry refuses `handle`, with the code the untyped entry gives for the same class.
///
/// The codes are not on the call's page, so they are compared with the call's own report rather
/// than named: what is under test is that this entry passes the code through unaltered.
fn refused_as_the_call_refuses(handle: &impl AsHandle) {
    let typed = request_for(handle)
        .perform()
        .expect_err("the call refuses this handle")
        .code();
    assert_eq!(typed, untyped_code(handle));
}

#[test]
fn nul_is_refused_with_the_calls_own_code() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    refused_as_the_call_refuses(&File::open("NUL").expect("open NUL"));
}

#[test]
fn an_anonymous_pipe_is_refused_with_the_calls_own_code() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let mut child = Command::new("cmd")
        .args(["/c", "exit", "0"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("start a child with a piped output");
    let output = child.stdout.take().expect("the child's output pipe");
    refused_as_the_call_refuses(&output);
    child.wait().expect("the child exits");
}

#[test]
fn a_volume_is_refused_with_the_calls_own_code() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
    let volume = OpenOptions::new()
        .access_mode(0)
        .share_mode(SHARE_ALL)
        .open(format!(r"\\.\{drive}"))
        .expect("open the system volume with no access");
    refused_as_the_call_refuses(&volume);
}

#[test]
fn a_file_opened_with_no_access_reports_the_identity_of_one_opened_to_read() {
    // Observed; the page names no access right the class needs.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileid-no-access");
    let no_access = OpenOptions::new()
        .access_mode(0)
        .share_mode(SHARE_ALL)
        .open(fixture.file())
        .expect("open the file with no access");

    assert_eq!(identity_of(&no_access), identity_of(&fixture.open_file()));
}

#[test]
fn two_opens_of_one_file_report_one_identity() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileid-two-opens");

    let one = identity_of(&fixture.open_file());
    let two = identity_of(&fixture.open_file());

    assert_eq!(one, two);
}

#[test]
fn two_files_report_different_identifiers_on_one_volume() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileid-two-files");
    let other = fixture.directory().join("g.t");
    std::fs::write(&other, b"other").expect("write a second file");

    let one = identity_of(&fixture.open_file());
    let two = identity_of(&File::open(&other).expect("open the second file"));

    assert_ne!(one.file_id(), two.file_id());
    assert_eq!(one.volume_serial_number(), two.volume_serial_number());
}

#[test]
fn a_copy_of_a_file_is_another_file() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileid-copy-file");
    let copy = fixture.directory().join("copy.t");
    std::fs::copy(fixture.file(), &copy).expect("copy the file");

    let original = identity_of(&fixture.open_file());
    let copied = identity_of(&File::open(&copy).expect("open the copy"));

    assert_ne!(original, copied);
}

#[test]
fn a_hard_link_reports_the_identity_of_the_file_it_names() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileid-hard-link");
    let link = fixture.directory().join("link.t");
    std::fs::hard_link(fixture.file(), &link).expect("make a hard link");

    let by_name = identity_of(&fixture.open_file());
    let by_link = identity_of(&File::open(&link).expect("open the link"));

    assert_eq!(by_name, by_link);
}

#[test]
fn a_directory_has_an_identity_of_its_own_on_the_volume_of_its_files() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileid-directory");

    let directory = identity_of(&fixture.open_directory());
    let file = identity_of(&fixture.open_file());

    assert_ne!(directory.file_id(), file.file_id());
    assert_eq!(
        directory.volume_serial_number(),
        file.volume_serial_number()
    );
}

#[test]
fn the_identifier_is_the_one_a_directory_listing_reports_for_the_same_file() {
    // The listing is `FileIdExtdDirectoryInfo` through the untyped entry, the record the enumeration
    // crate reads its identities from.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileid-listing");
    let directory = fixture.open_directory();

    let batch = QueryFileInformation::new(
        SharedHandle::from(
            directory
                .as_handle()
                .try_clone_to_owned()
                .expect("own the directory"),
        ),
        FileInformationClass::ID_EXTD_DIRECTORY_RESTART,
    )
    .perform()
    .expect("list the directory");

    let mut listed = None;
    let mut offset = 0_usize;
    loop {
        let base = batch.as_ptr().wrapping_add(offset);
        // SAFETY: the batch is 8-byte aligned and Windows wrote whole records into it; each record's
        // NextEntryOffset bounds the walk.
        let record = unsafe { &*base.cast::<FILE_ID_EXTD_DIR_INFO>() };
        let name_offset = std::mem::offset_of!(FILE_ID_EXTD_DIR_INFO, FileName);
        // SAFETY: the name follows the fixed part, in bounds by the same guarantee.
        let name = unsafe {
            std::slice::from_raw_parts(
                base.wrapping_add(name_offset).cast::<u16>(),
                record.FileNameLength as usize / 2,
            )
        };
        if String::from_utf16_lossy(name) == "f.t" {
            listed = Some(record.FileId.Identifier);
        }
        if record.NextEntryOffset == 0 {
            break;
        }
        offset += record.NextEntryOffset as usize;
    }

    let listed = listed.expect("the listing names the fixture file");
    assert_eq!(identity_of(&fixture.open_file()).file_id(), listed);
}

#[test]
fn the_identifier_opens_the_file_it_names_by_its_extended_form() {
    use crate::open_by_id::{FileIdentifier, OpenFileByIdentifier};

    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileid-open-by-id");
    let file = fixture.open_file();
    let identity = identity_of(&file);

    let reopened = OpenFileByIdentifier::new(
        SharedHandle::from(
            fixture
                .open_directory()
                .as_handle()
                .try_clone_to_owned()
                .expect("own the hint"),
        ),
        FileIdentifier::ExtendedFileId(identity.file_id()),
    )
    .with_share_mode(SHARE_ALL)
    .perform()
    .expect("open by the extended identifier");

    assert_eq!(identity_of(&reopened), identity);
}

#[test]
fn a_handle_that_is_not_a_file_is_the_calls_own_code() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    // SAFETY: no attributes, an unnamed auto-reset event, unsignalled.
    let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
    assert!(!event.is_null(), "an event is made");
    // SAFETY: a handle this test owns and nothing else does.
    let event = unsafe { OwnedHandle::from_raw_handle(event) };

    let error = request_for(&event)
        .perform()
        .expect_err("an event has no file identity");

    assert_eq!(error.code(), ERROR_INVALID_HANDLE);
}

#[test]
fn a_value_is_its_two_fields_and_equality_needs_both() {
    let id = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let value = FileIdInfo::new(0x0123_4567_89ab_cdef, id);

    assert_eq!(value.volume_serial_number(), 0x0123_4567_89ab_cdef);
    assert_eq!(value.file_id(), id);
    assert_eq!(value, FileIdInfo::new(0x0123_4567_89ab_cdef, id));
    assert_ne!(value, FileIdInfo::new(0x0123_4567_89ab_cdee, id));
    let mut other = id;
    other[15] = 17;
    assert_ne!(value, FileIdInfo::new(0x0123_4567_89ab_cdef, other));
}

#[test]
fn a_clone_shares_the_handle() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileid-copy");
    let file = fixture.open_file();
    let request = request_for(&file);

    let copy = request.clone();

    assert!(copy.handle().same_handle(request.handle()));
    assert_eq!(
        copy.perform().expect("the copy"),
        request.perform().expect("the original")
    );
}

#[test]
fn the_seam_performs_the_same_query() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileid-seam");
    let request = request_for(&fixture.open_file());

    assert_eq!(
        Request::perform(&request).expect("through the seam"),
        request.perform().expect("directly")
    );
}

#[test]
fn a_query_performs_the_same_way_on_another_thread() {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}

    assert_send::<QueryFileIdInfo>();
    assert_sync::<QueryFileIdInfo>();

    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("fileid-thread");
    let file = fixture.open_file();
    let here = identity_of(&file);
    let request = request_for(&file);

    let there = std::thread::spawn(move || request.perform().expect("query on a worker"))
        .join()
        .expect("the worker did not panic");

    assert_eq!(there, here);
}
