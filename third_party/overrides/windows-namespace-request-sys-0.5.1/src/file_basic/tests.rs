// Copyright (c) Mike Grier.

//! Tests for the typed `FileBasicInfo` entry.

use std::fs::{File, FileTimes, OpenOptions};
use std::os::windows::fs::{FileTimesExt, OpenOptionsExt};
use std::os::windows::io::{AsHandle, FromRawHandle, OwnedHandle};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use windows_sys::Win32::Foundation::ERROR_INVALID_HANDLE;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_READONLY, FILE_BASIC_INFO, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::Threading::CreateEventW;

use super::{FileBasicInfo, QueryFileBasicInfo};
use crate::handle::tests::{Fixture, handle_allocation};
use crate::outcome::Outcome;
use crate::query::{FileInformationClass, QueryFileInformation};
use crate::request::Request;
use win_shared_os_owned_handle::SharedHandle;

const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// `FILETIME` units between 1601-01-01 and 1970-01-01, the two epochs.
const EPOCH_DIFFERENCE: i64 = 116_444_736_000_000_000;
/// `FILETIME` units in a second: one unit is 100 ns.
const UNITS_PER_SECOND: i64 = 10_000_000;

fn shared(handle: &impl AsHandle) -> SharedHandle {
    SharedHandle::from(
        handle
            .as_handle()
            .try_clone_to_owned()
            .expect("own the handle"),
    )
}

fn request_for(handle: &impl AsHandle) -> QueryFileBasicInfo {
    QueryFileBasicInfo::new(shared(handle))
}

fn basic_of(handle: &impl AsHandle) -> FileBasicInfo {
    request_for(handle)
        .perform()
        .expect("query the basic information")
}

/// What the untyped entry reports for the same class on the same handle, read field by field from
/// its bytes at the structure's own offsets.
fn untyped(handle: &impl AsHandle) -> Outcome<FileBasicInfo> {
    let bytes = QueryFileInformation::new(shared(handle), FileInformationClass::BASIC)
        .with_capacity(size_of::<FILE_BASIC_INFO>())
        .perform()?;
    let bytes = bytes.as_slice();
    let time = |offset: usize| {
        i64::from_le_bytes(
            bytes[offset..offset + 8]
                .try_into()
                .expect("eight bytes of a time"),
        )
    };
    let attributes_at = std::mem::offset_of!(FILE_BASIC_INFO, FileAttributes);
    Ok(FileBasicInfo::new(
        time(std::mem::offset_of!(FILE_BASIC_INFO, CreationTime)),
        time(std::mem::offset_of!(FILE_BASIC_INFO, LastAccessTime)),
        time(std::mem::offset_of!(FILE_BASIC_INFO, LastWriteTime)),
        time(std::mem::offset_of!(FILE_BASIC_INFO, ChangeTime)),
        u32::from_le_bytes(
            bytes[attributes_at..attributes_at + 4]
                .try_into()
                .expect("four bytes of attributes"),
        ),
    ))
}

/// The typed entry and the untyped one give the same outcome for `handle`: the same five values, or
/// the same code.
///
/// Which codes a handle of another kind gets is not on the call's page, so a refusal is compared with
/// the call's own report rather than named: what is under test is that this entry passes it through.
fn agrees_with_the_untyped_entry(handle: &impl AsHandle) {
    let typed = request_for(handle).perform();
    match (typed, untyped(handle)) {
        (Ok(typed), Ok(untyped)) => assert_eq!(typed, untyped),
        (Err(typed), Err(untyped)) => assert_eq!(typed.code(), untyped.code()),
        (typed, untyped) => panic!("the entries disagree: {typed:?} against {untyped:?}"),
    }
}

/// The `FILETIME` value of a time after 1970.
fn filetime(time: SystemTime) -> i64 {
    let since = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("a time after 1970");
    let seconds = i64::try_from(since.as_secs()).expect("the seconds fit an i64");
    EPOCH_DIFFERENCE + seconds * UNITS_PER_SECOND + i64::from(since.subsec_nanos() / 100)
}

fn at(seconds: u64, units: u32) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::new(seconds, units * 100)
}

/// The fixture file with three distinct times of its own setting, so that no two of the four times
/// the call reports are equal and a field read from the wrong place shows.
fn file_with_distinct_times(fixture: &Fixture) -> (File, SystemTime, SystemTime, SystemTime) {
    let created = at(1_000_000_000, 1);
    let accessed = at(1_100_000_000, 2);
    let modified = at(1_200_000_000, 3);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(fixture.file())
        .expect("open the file to set its times");
    file.set_times(
        FileTimes::new()
            .set_created(created)
            .set_accessed(accessed)
            .set_modified(modified),
    )
    .expect("set the file's times");
    (file, created, accessed, modified)
}

#[test]
fn each_time_is_the_one_set_for_it() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("filebasic-times");
    let (file, created, accessed, modified) = file_with_distinct_times(&fixture);

    let basic = basic_of(&file);

    assert_eq!(basic.creation_time(), filetime(created));
    assert_eq!(basic.last_access_time(), filetime(accessed));
    assert_eq!(basic.last_write_time(), filetime(modified));
    // Not set here: the system sets it when the times change, so it is after all three.
    assert!(basic.change_time() > filetime(modified));
}

#[test]
fn the_fields_agree_with_the_untyped_entrys_bytes() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("filebasic-bytes");
    let (file, ..) = file_with_distinct_times(&fixture);

    let typed = basic_of(&file);
    let untyped = untyped(&file).expect("the untyped entry reads the class");

    assert_eq!(typed, untyped);
}

#[test]
fn the_fields_agree_with_the_untyped_entrys_bytes_for_a_directory() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("filebasic-bytes-directory");

    agrees_with_the_untyped_entry(&fixture.open_directory());
}

#[test]
fn a_directory_reports_the_directory_attribute_and_a_file_does_not() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("filebasic-directory");

    let directory = basic_of(&fixture.open_directory());
    let file = basic_of(&fixture.open_file());

    assert_ne!(directory.file_attributes() & FILE_ATTRIBUTE_DIRECTORY, 0);
    assert_eq!(file.file_attributes() & FILE_ATTRIBUTE_DIRECTORY, 0);
}

#[test]
fn a_read_only_file_reports_the_read_only_attribute() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("filebasic-read-only");
    let writable = basic_of(&fixture.open_file());
    let original = std::fs::metadata(fixture.file())
        .expect("read the permissions")
        .permissions();
    let mut read_only_permissions = original.clone();
    read_only_permissions.set_readonly(true);
    std::fs::set_permissions(fixture.file(), read_only_permissions).expect("make it read-only");

    let read_only = basic_of(&fixture.open_file());

    std::fs::set_permissions(fixture.file(), original).expect("restore the permissions");
    assert_eq!(writable.file_attributes() & FILE_ATTRIBUTE_READONLY, 0);
    assert_ne!(read_only.file_attributes() & FILE_ATTRIBUTE_READONLY, 0);
}

#[test]
fn writing_to_a_file_moves_its_last_write_time_and_not_its_creation_time() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("filebasic-write");
    let (file, ..) = file_with_distinct_times(&fixture);
    let before = basic_of(&file);
    drop(file);

    // Through a handle of its own: a time set explicitly through a handle was observed not to move
    // when that same handle is later written through.
    let writer = OpenOptions::new()
        .write(true)
        .open(fixture.file())
        .expect("open the file to write");
    std::io::Write::write_all(&mut &writer, b"more").expect("write to the file");
    writer.sync_all().expect("flush the write");
    let after = basic_of(&writer);

    assert_eq!(after.creation_time(), before.creation_time());
    assert!(after.last_write_time() > before.last_write_time());
}

#[test]
fn a_file_opened_with_no_access_agrees_with_the_untyped_entry() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("filebasic-no-access");
    let no_access = OpenOptions::new()
        .access_mode(0)
        .share_mode(SHARE_ALL)
        .open(fixture.file())
        .expect("open the file with no access");

    agrees_with_the_untyped_entry(&no_access);
}

#[test]
fn nul_agrees_with_the_untyped_entry() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    agrees_with_the_untyped_entry(&File::open("NUL").expect("open NUL"));
}

#[test]
fn an_anonymous_pipe_agrees_with_the_untyped_entry() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let mut child = Command::new("cmd")
        .args(["/c", "exit", "0"])
        .stdout(Stdio::piped())
        .spawn()
        .expect("start a child with a piped output");
    let output = child.stdout.take().expect("the child's output pipe");
    agrees_with_the_untyped_entry(&output);
    child.wait().expect("the child exits");
}

#[test]
fn a_volume_opened_with_no_access_agrees_with_the_untyped_entry() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_owned());
    let volume = OpenOptions::new()
        .access_mode(0)
        .share_mode(SHARE_ALL)
        .open(format!(r"\\.\{drive}"))
        .expect("open the system volume with no access");
    agrees_with_the_untyped_entry(&volume);
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
        .expect_err("an event has no basic file information");

    assert_eq!(error.code(), ERROR_INVALID_HANDLE);
    agrees_with_the_untyped_entry(&event);
}

#[test]
fn a_value_is_its_five_fields_and_equality_needs_each() {
    let value = FileBasicInfo::new(1, 2, 3, 4, 5);

    assert_eq!(value.creation_time(), 1);
    assert_eq!(value.last_access_time(), 2);
    assert_eq!(value.last_write_time(), 3);
    assert_eq!(value.change_time(), 4);
    assert_eq!(value.file_attributes(), 5);
    assert_eq!(value, FileBasicInfo::new(1, 2, 3, 4, 5));
    for other in [
        FileBasicInfo::new(0, 2, 3, 4, 5),
        FileBasicInfo::new(1, 0, 3, 4, 5),
        FileBasicInfo::new(1, 2, 0, 4, 5),
        FileBasicInfo::new(1, 2, 3, 0, 5),
        FileBasicInfo::new(1, 2, 3, 4, 0),
    ] {
        assert_ne!(value, other);
    }
}

#[test]
fn a_clone_shares_the_handle() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("filebasic-clone");
    let request = request_for(&fixture.open_file());

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
    let fixture = Fixture::new("filebasic-seam");
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

    assert_send::<QueryFileBasicInfo>();
    assert_sync::<QueryFileBasicInfo>();

    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("filebasic-thread");
    let (file, ..) = file_with_distinct_times(&fixture);
    let here = basic_of(&file);
    let request = request_for(&file);

    let there = std::thread::spawn(move || request.perform().expect("the query on a worker"))
        .join()
        .expect("the worker did not panic");

    assert_eq!(there, here);
}
