// Copyright (c) Mike Grier.

//! Tests for the `GetFileType` entry.

use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsHandle, FromRawHandle, OwnedHandle};
use std::process::{Command, Stdio};

use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INVALID_HANDLE, NO_ERROR, SetLastError,
};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::Threading::CreateEventW;

use super::{FileType, QueryFileType, unknown_with_no_error_is_a_result};
use crate::Win32Error;
use crate::handle::tests::{Fixture, handle_allocation};
use win_shared_os_owned_handle::SharedHandle;

fn request_for(handle: &impl AsHandle) -> QueryFileType {
    QueryFileType::new(SharedHandle::from(
        handle
            .as_handle()
            .try_clone_to_owned()
            .expect("own the handle"),
    ))
}

fn kind_of(handle: &impl AsHandle) -> FileType {
    request_for(handle).perform().expect("query the handle")
}

#[test]
fn a_disk_file_reports_disk() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("filetype-file");
    assert_eq!(kind_of(&fixture.open_file()), FileType::DISK);
}

#[test]
fn a_directory_reports_disk() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("filetype-directory");
    assert_eq!(kind_of(&fixture.open_directory()), FileType::DISK);
}

#[test]
fn a_volume_reports_disk_as_a_file_does() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    // Observed, and the reason this entry does not say what a handle is beyond what the page says:
    // a volume opened with no access reports what a file reports.
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
    let volume = OpenOptions::new()
        .access_mode(0)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(format!(r"\\.\{drive}"))
        .expect("open the system volume with no access");
    assert_eq!(kind_of(&volume), FileType::DISK);
}

#[test]
fn nul_reports_char() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let nul = File::open("NUL").expect("open NUL");
    assert_eq!(kind_of(&nul), FileType::CHAR);
}

#[test]
fn an_anonymous_pipe_reports_pipe() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let mut child = Command::new("cmd")
        .args(["/c", "exit", "0"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("start a child with a piped output");
    let output = child.stdout.take().expect("the child's output pipe");
    assert_eq!(kind_of(&output), FileType::PIPE);
    child.wait().expect("the child exits");
}

#[test]
fn a_handle_that_is_not_a_file_is_the_calls_own_code() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    // An event is a kernel object that is not a file; the call fails with `ERROR_INVALID_HANDLE`,
    // which is the call's failure reading of an unknown type, reported unaltered.
    // SAFETY: no attributes, an unnamed auto-reset event, unsignalled.
    let event = unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) };
    assert!(!event.is_null(), "an event is made");
    // SAFETY: a handle this test owns and nothing else does.
    let event = unsafe { OwnedHandle::from_raw_handle(event) };

    let error = request_for(&event)
        .perform()
        .expect_err("an event has no file type");

    assert_eq!(error.code(), ERROR_INVALID_HANDLE);
}

#[test]
fn a_stale_code_from_an_earlier_call_does_not_change_a_result() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("filetype-stale");
    let file = fixture.open_file();
    let request = request_for(&file);

    // SAFETY: `SetLastError` writes only this thread's error slot.
    unsafe { SetLastError(ERROR_ACCESS_DENIED) };

    assert_eq!(request.perform().expect("a result"), FileType::DISK);
}

#[test]
fn an_unknown_type_is_a_result_with_no_error_and_the_code_otherwise() {
    // The page's rule, as a function of what the call returned and what the thread's last error was.
    // No handle kind observed on this machine returns an unknown type from a call that worked, so
    // this reading is checked here and not through a handle.
    assert_eq!(
        unknown_with_no_error_is_a_result(Err(Win32Error::from_code(NO_ERROR))),
        Ok(FileType::UNKNOWN)
    );
    assert_eq!(
        unknown_with_no_error_is_a_result(Err(Win32Error::from_code(ERROR_INVALID_HANDLE))),
        Err(Win32Error::from_code(ERROR_INVALID_HANDLE))
    );
    for kind in [
        FileType::DISK,
        FileType::CHAR,
        FileType::PIPE,
        FileType::REMOTE,
        FileType::UNKNOWN,
        FileType::from_raw(0x1234),
    ] {
        assert_eq!(
            unknown_with_no_error_is_a_result(Ok(kind.as_raw())),
            Ok(kind)
        );
    }
}

#[test]
fn the_values_are_the_pages_and_are_named_when_printed() {
    assert_eq!(FileType::UNKNOWN.as_raw(), 0x0000);
    assert_eq!(FileType::DISK.as_raw(), 0x0001);
    assert_eq!(FileType::CHAR.as_raw(), 0x0002);
    assert_eq!(FileType::PIPE.as_raw(), 0x0003);
    assert_eq!(FileType::REMOTE.as_raw(), 0x8000);

    assert_eq!(format!("{:?}", FileType::DISK), "FileType::DISK");
    assert_eq!(format!("{:?}", FileType::CHAR), "FileType::CHAR");
    assert_eq!(format!("{:?}", FileType::PIPE), "FileType::PIPE");
    assert_eq!(format!("{:?}", FileType::REMOTE), "FileType::REMOTE");
    assert_eq!(format!("{:?}", FileType::UNKNOWN), "FileType::UNKNOWN");
    assert_eq!(
        format!("{:?}", FileType::from_raw(0x1234)),
        "FileType(0x1234)"
    );
    assert_eq!(FileType::from_raw(0x1234).as_raw(), 0x1234);
}

#[test]
fn a_clone_shares_the_handle() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("filetype-copy");
    let file = fixture.open_file();
    let request = request_for(&file);

    let copy = request.clone();

    assert!(copy.handle().same_handle(request.handle()));
    assert_eq!(copy.perform().expect("the copy"), FileType::DISK);
}

#[test]
fn a_query_performs_the_same_way_on_another_thread() {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}

    assert_send::<QueryFileType>();
    assert_sync::<QueryFileType>();

    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("filetype-thread");
    let file = fixture.open_file();
    let request = request_for(&file);

    let kind = std::thread::spawn(move || request.perform().expect("query on a worker"))
        .join()
        .expect("the worker did not panic");

    assert_eq!(kind, FileType::DISK);
}
