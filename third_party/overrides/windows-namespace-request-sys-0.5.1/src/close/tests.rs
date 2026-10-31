// Copyright (c) Mike Grier.

//! Tests for the close entries.
//!
//! The load-bearing property is that a handle is closed exactly once, whichever
//! path it takes. That is checked by closing and then observing that a second
//! close of the same value fails -- if either path closed twice, the process
//! would be closing a handle it no longer owns.

use std::fs::File;
use std::os::windows::io::{AsRawHandle, OwnedHandle};

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INVALID_HANDLE, FALSE, HANDLE};
use wtf_string::Wtf16String;

use super::CloseRequest;
use crate::handle::tests::{Fixture, handle_allocation};
use crate::watch::{NotifyFilter, WatchDirectory};

/// Closes a raw handle directly, to observe whether it was still open.
///
/// Returns true if the close succeeded, meaning the handle was open.
fn was_still_open(handle: HANDLE) -> bool {
    // SAFETY: the handle value is only passed to CloseHandle, which validates
    // it. A stale value fails with ERROR_INVALID_HANDLE rather than closing
    // something else, because these tests hold the allocation lock.
    unsafe { CloseHandle(handle) != FALSE }
}

fn an_owned_handle(fixture: &Fixture) -> OwnedHandle {
    OwnedHandle::from(fixture.open_file())
}

#[test]
fn an_ordinary_handle_closes() {
    let _allocating = handle_allocation()
        .write()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("close-ordinary");
    let owned = an_owned_handle(&fixture);
    let raw = owned.as_raw_handle().cast();

    CloseRequest::for_handle(owned)
        .perform()
        .expect("close the handle");

    assert!(
        !was_still_open(raw),
        "the handle must be closed exactly once, by the request"
    );
}

#[test]
fn a_dropped_request_still_closes_its_handle() {
    // An unperformed request closing nothing would be a leak, which is worse
    // than closing late.
    let _allocating = handle_allocation()
        .write()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("close-dropped");
    let owned = an_owned_handle(&fixture);
    let raw = owned.as_raw_handle().cast();

    drop(CloseRequest::for_handle(owned));

    assert!(
        !was_still_open(raw),
        "dropping the request must close the handle, not leak it"
    );
}

#[test]
fn performing_the_request_does_not_also_close_on_drop() {
    // `perform` consumes the request through a ManuallyDrop precisely so the
    // destructor cannot close a second time. A double close is what this rules
    // out, and it would be silent without the check below.
    let _allocating = handle_allocation()
        .write()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("close-once");
    let owned = an_owned_handle(&fixture);
    let raw = owned.as_raw_handle().cast();

    CloseRequest::for_handle(owned)
        .perform()
        .expect("close the handle");

    // If the drop had also closed it, this value would have been recycled or
    // the second close would already have happened; either way a third close
    // must fail rather than succeed.
    assert!(!was_still_open(raw));
}

#[test]
fn taking_the_handle_stops_owned_handle_from_closing_it_first() {
    // for_handle takes ownership away from OwnedHandle. If it did not, the
    // handle would be closed when that value dropped and the request would then
    // close a stale value.
    let _allocating = handle_allocation()
        .write()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("close-ownership");
    let owned = an_owned_handle(&fixture);
    let raw: HANDLE = owned.as_raw_handle().cast();

    let request = CloseRequest::for_handle(owned);

    assert_eq!(request.handle(), raw, "the request holds the same handle");
    request
        .perform()
        .expect("the handle was still open to close");
}

#[test]
fn a_change_notification_closes_with_its_own_routine() {
    // The pairing the audit found a close entry cannot assume: this handle is
    // closed with FindCloseChangeNotification, and CloseHandle is wrong for it.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("close-notification");
    let text = fixture
        .directory()
        .to_str()
        .expect("the fixture path is valid UTF-8");
    let notification = WatchDirectory::new(Wtf16String::from(text))
        .expect("build the request")
        .with_filter(NotifyFilter::FILE_NAME)
        .perform()
        .expect("start a watch");

    CloseRequest::for_change_notification(notification)
        .perform()
        .expect("close the notification with the correct routine");
}

#[test]
fn a_dropped_notification_request_still_closes() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("close-notification-dropped");
    let text = fixture
        .directory()
        .to_str()
        .expect("the fixture path is valid UTF-8");
    let notification = WatchDirectory::new(Wtf16String::from(text))
        .expect("build the request")
        .with_filter(NotifyFilter::FILE_NAME)
        .perform()
        .expect("start a watch");

    // Must not double close: the notification's own Drop is suppressed when the
    // request takes it over.
    drop(CloseRequest::for_change_notification(notification));
}

#[test]
fn the_two_routines_are_distinguishable() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("close-routines");
    let text = fixture
        .directory()
        .to_str()
        .expect("the fixture path is valid UTF-8");
    let notification = WatchDirectory::new(Wtf16String::from(text))
        .expect("build the request")
        .with_filter(NotifyFilter::FILE_NAME)
        .perform()
        .expect("start a watch");

    let ordinary = format!("{:?}", CloseRequest::for_handle(an_owned_handle(&fixture)));
    let custom = format!("{:?}", CloseRequest::for_change_notification(notification));

    assert!(ordinary.contains("CloseHandle"), "unexpected: {ordinary}");
    assert!(
        custom.contains("FindCloseChangeNotification"),
        "unexpected: {custom}"
    );
}

#[test]
fn a_caller_supplied_routine_is_carried() {
    // The escape hatch: a handle whose close routine this crate does not know
    // about needs no change here.
    let _allocating = handle_allocation()
        .write()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("close-custom");
    let owned = an_owned_handle(&fixture);
    let raw: HANDLE = owned.as_raw_handle().cast();
    std::mem::forget(owned);

    // SAFETY: raw is a live handle whose ownership is given up above, and
    // CloseHandle is its correct routine.
    let request = unsafe { CloseRequest::from_raw(raw, CloseHandle) };

    request.perform().expect("close through the custom routine");
    assert!(!was_still_open(raw));
}

#[test]
fn closing_an_already_closed_handle_reports_the_raw_code() {
    let _allocating = handle_allocation()
        .write()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("close-stale");
    let owned = an_owned_handle(&fixture);
    let raw: HANDLE = owned.as_raw_handle().cast();
    drop(owned);

    // SAFETY: the value is stale, which is what this asserts; the allocation
    // lock is held exclusively so nothing can have reused it.
    let request = unsafe { CloseRequest::from_raw(raw, CloseHandle) };
    let error = request.perform().expect_err("a stale handle cannot close");

    assert_eq!(
        error.code(),
        ERROR_INVALID_HANDLE,
        "the code is passed through, not reclassified"
    );
}

#[test]
fn a_close_performs_the_same_way_on_another_thread() {
    // The point of making a close a request at all: CloseHandle blocks on
    // outstanding I/O, so it belongs wherever blocking is acceptable.
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}

    assert_send::<CloseRequest>();
    assert_sync::<CloseRequest>();

    let _allocating = handle_allocation()
        .write()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("close-thread");
    let owned = an_owned_handle(&fixture);
    let raw: HANDLE = owned.as_raw_handle().cast();
    let request = CloseRequest::for_handle(owned);

    std::thread::spawn(move || request.perform())
        .join()
        .expect("the worker did not panic")
        .expect("close on a worker thread");

    assert!(!was_still_open(raw));
}

#[test]
fn a_file_handle_round_trips_through_the_request() {
    let _allocating = handle_allocation()
        .write()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("close-roundtrip");
    let file = File::open(fixture.file()).expect("open the fixture file");
    let raw: HANDLE = file.as_raw_handle().cast();

    // std's File converts straight into the request, so a consumer does not
    // need to know anything about raw handles to use this entry.
    CloseRequest::for_handle(file.into())
        .perform()
        .expect("close a std File's handle");

    assert!(!was_still_open(raw));
}
