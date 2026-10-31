// Copyright (c) Mike Grier.

//! Tests for the seam.
//!
//! The point of a seam is that something can be substituted through it, so
//! these bind a fake and a real entry to the same generic code and check that
//! both drive it. A seam only ever exercised by its real implementation is not
//! a seam.

use std::os::windows::io::OwnedHandle;

use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING,
};
use wtf_string::Wtf16String;

use super::{ConsumingRequest, Request};
use crate::Win32Error;
use crate::close::CloseRequest;
use crate::handle::tests::{Fixture, handle_allocation};
use crate::open::OpenFile;
use crate::outcome::Outcome;
use crate::watch::{NotifyFilter, WatchDirectory};

/// Stands in for consumer code written against the seam.
fn attempt_twice<R: Request>(request: &R) -> usize {
    [request.perform(), request.perform()]
        .iter()
        .filter(|outcome| outcome.is_ok())
        .count()
}

/// Stands in for consumer cleanup code written against the seam.
fn perform_each<R: ConsumingRequest>(requests: Vec<R>) -> usize {
    requests
        .into_iter()
        .map(ConsumingRequest::perform)
        .filter(Result::is_ok)
        .count()
}

/// A fake that never touches Win32, which is the whole point.
struct CannedOpen {
    outcome: Result<u32, Win32Error>,
}

impl Request for CannedOpen {
    type Error = Win32Error;
    type Output = u32;

    fn perform(&self) -> Outcome<u32> {
        self.outcome
    }
}

struct CannedClose;

impl ConsumingRequest for CannedClose {
    type Error = Win32Error;
    type Output = ();

    fn perform(self) -> Outcome<()> {
        Ok(())
    }
}

#[test]
fn a_fake_drives_consumer_code_with_no_filesystem() {
    let succeeds = CannedOpen { outcome: Ok(1) };
    let fails = CannedOpen {
        outcome: Err(Win32Error::from_code(ERROR_FILE_NOT_FOUND)),
    };

    assert_eq!(attempt_twice(&succeeds), 2);
    assert_eq!(attempt_twice(&fails), 0);
}

#[test]
fn a_real_entry_drives_the_same_consumer_code() {
    // The seam is only worth having if both sides fit it.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("seam-real-open");
    let text = fixture
        .directory()
        .to_str()
        .expect("the fixture path is valid UTF-8");
    let request = OpenFile::new(Wtf16String::from(text))
        .expect("build the request")
        .with_desired_access(FILE_LIST_DIRECTORY)
        .with_share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .with_creation_disposition(OPEN_EXISTING)
        .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS);

    assert_eq!(
        attempt_twice(&request),
        2,
        "a real open is a parameter set that performs repeatedly"
    );
}

#[test]
fn the_watch_entry_fits_the_same_trait() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("seam-real-watch");
    let text = fixture
        .directory()
        .to_str()
        .expect("the fixture path is valid UTF-8");
    let request = WatchDirectory::new(Wtf16String::from(text))
        .expect("build the request")
        .with_filter(NotifyFilter::FILE_NAME);

    assert_eq!(attempt_twice(&request), 2);
}

#[test]
fn a_fake_close_drives_consumer_cleanup_code() {
    assert_eq!(perform_each(vec![CannedClose, CannedClose]), 2);
}

#[test]
fn a_real_close_drives_the_same_cleanup_code() {
    let _allocating = handle_allocation()
        .write()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("seam-real-close");

    let requests: Vec<CloseRequest> = (0..3)
        .map(|_| {
            let file = fixture.open_file();
            CloseRequest::for_handle(OwnedHandle::from(file))
        })
        .collect();

    assert_eq!(perform_each(requests), 3);
}

#[test]
fn the_trait_method_and_the_inherent_method_agree() {
    // The entries keep their inherent `perform`, which is what an ordinary
    // caller uses; the trait must not be a second, divergent path.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("seam-agreement");
    let text = fixture
        .directory()
        .to_str()
        .expect("the fixture path is valid UTF-8");
    let request = OpenFile::new(Wtf16String::from(text))
        .expect("build the request")
        .with_desired_access(FILE_LIST_DIRECTORY)
        .with_share_mode(FILE_SHARE_READ)
        .with_creation_disposition(OPEN_EXISTING)
        .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS);

    let inherent = OpenFile::perform(&request);
    let through_trait = Request::perform(&request);

    assert_eq!(inherent.is_ok(), through_trait.is_ok());
}

#[test]
fn a_request_can_be_held_behind_a_trait_object() {
    // A consumer that stores heterogeneous requests needs this to work, and it
    // only does if the trait stays object-safe.
    let requests: Vec<Box<dyn Request<Output = u32, Error = Win32Error>>> = vec![
        Box::new(CannedOpen { outcome: Ok(1) }),
        Box::new(CannedOpen {
            outcome: Err(Win32Error::from_code(ERROR_FILE_NOT_FOUND)),
        }),
    ];

    let successes = requests
        .iter()
        .filter(|request| request.perform().is_ok())
        .count();

    assert_eq!(successes, 1);
}
