// Copyright (c) Mike Grier.

//! Scenario coverage: does the catalogue serve each consumer's actual *shape*?
//!
//! A different question from "is every call reachable", and the one the
//! original audit left unanswered. Globazog makes it concrete and demanding:
//! one capture taken at submission, shared by up to 64 concurrent workers,
//! applied repeatedly over a traversal that may run for minutes.
//!
//! # This is where the two sibling crates are paired
//!
//! `windows-thread-ambient-sys` is a **dev**-dependency of this crate and
//! nothing more. That is the whole point: a request carries no ambient context
//! and a context is useful to work that never opens a file, so whoever owns
//! both pairs them at the submission site. These tests *are* that submission
//! site, which is the only honest way to demonstrate a relationship the design
//! deliberately refuses to make into a dependency.

use std::fs::File;
use std::os::windows::io::AsHandle;
use std::sync::Arc;

use win_shared_os_owned_handle::SharedHandle;
use windows_namespace_request_sys::Request;
use windows_namespace_request_sys::close::CloseRequest;
use windows_namespace_request_sys::file_info::QueryFileInformationByHandle;
use windows_namespace_request_sys::final_path::QueryFinalPath;
use windows_namespace_request_sys::open_by_id::{FileIdentifier, OpenFileByIdentifier};
use windows_namespace_request_sys::query::{FileInformationClass, QueryFileInformation};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY};
use windows_thread_ambient_sys::{AmbientState, CaptureSet};

use crate::support::{AUDITED_SHARE, Tree, audited_directory_open, open_directory};

/// Globazog's stated worker count.
const CONCURRENT_WORKERS: usize = 16;

fn shared(file: &File) -> SharedHandle {
    SharedHandle::from(
        file.as_handle()
            .try_clone_to_owned()
            .expect("own the handle"),
    )
}

#[test]
fn a_request_built_on_one_thread_executes_on_another_under_a_captured_context() {
    // The pairing, demonstrated: the context is captured here, where a failure
    // is still the submitter's to see, and the request is built here too. Both
    // travel; neither knows about the other.
    let tree = Tree::new("scenario-paired");

    let state = AmbientState::capture(CaptureSet::DEFAULT).expect("capture the ambient state");
    let request = audited_directory_open(tree.root());

    let applied = std::thread::spawn(move || {
        state.with_applied(|| request.perform().map(File::from).is_ok())
    })
    .join()
    .expect("the worker did not panic")
    .expect("the context applied on the worker");

    assert!(
        *applied.value(),
        "the request ran under the captured context"
    );
    assert!(applied.restore().is_clean());
}

#[test]
fn one_capture_serves_many_requests_across_concurrent_workers() {
    // Globazog's shape: one capture at submit(), shared by many workers, each
    // performing its own request. This needs AmbientState to be Sync and
    // shareable through an Arc, not merely Send.
    let tree = Tree::new("scenario-many-workers");

    let state = Arc::new(AmbientState::capture(CaptureSet::DEFAULT).expect("capture once"));

    let workers: Vec<_> = (0..CONCURRENT_WORKERS)
        .map(|_| {
            let state = Arc::clone(&state);
            let request = audited_directory_open(tree.root());

            std::thread::spawn(move || {
                let handle = state
                    .with_applied(|| request.perform())
                    .expect("the context applied")
                    // A contaminated thread is a failure rather than something
                    // to check separately and forget to act on.
                    .into_clean_value()
                    .expect("the worker's thread was restored cleanly")
                    .expect("the open succeeded on the worker");
                CloseRequest::for_handle(handle)
                    .perform()
                    .expect("and closed on the same worker");
                true
            })
        })
        .collect();

    for worker in workers {
        assert!(
            worker.join().expect("every worker succeeded"),
            "every worker restored its thread cleanly"
        );
    }
}

#[test]
fn a_handle_opened_by_one_request_is_carried_into_a_later_one() {
    // The combination no single-entry test exercises, and the one the audit
    // named: a caller's owned duplicate meeting M26.1's shared enumeration cursor.
    let tree = Tree::new("scenario-carry");

    // First request: open the directory.
    let opened = File::from(
        audited_directory_open(tree.root())
            .perform()
            .expect("open the tree root"),
    );

    // Second request: enumerate through a *duplicate* of that handle.
    let restart = QueryFileInformation::new(
        shared(&opened),
        FileInformationClass::ID_EXTD_DIRECTORY_RESTART,
    )
    .with_capacity(320);
    let first = restart.perform().expect("the first batch");

    // Third request: continue, through another duplicate. The cursor is shared,
    // so this is a continuation rather than a restart.
    let next = QueryFileInformation::new(shared(&opened), FileInformationClass::ID_EXTD_DIRECTORY)
        .with_capacity(320);
    let second = next.perform().expect("the second batch");

    assert_ne!(
        first.as_slice(),
        second.as_slice(),
        "a duplicate continues the source's enumeration rather than restarting it"
    );

    // Fourth request: the same handle becomes a volume hint for a reopen.
    let identity = QueryFileInformationByHandle::new(shared(&opened))
        .perform()
        .expect("read the identity");
    let file_id = (u64::from(identity.nFileIndexHigh) << 32) | u64::from(identity.nFileIndexLow);

    let reopened = OpenFileByIdentifier::new(shared(&opened), FileIdentifier::FileId(file_id))
        .with_desired_access(FILE_LIST_DIRECTORY)
        .with_share_mode(AUDITED_SHARE)
        .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS)
        .perform()
        .expect("reopen through the carried handle");

    CloseRequest::for_handle(reopened).perform().expect("close");
}

#[test]
fn a_traversal_runs_many_requests_from_one_capture_over_time() {
    // Globazog applies its capture repeatedly over a traversal that may run for
    // minutes, rather than once around a single call. The granularity is the
    // consumer's choice and this proves the per-operation end of it works.
    let tree = Tree::new("scenario-traversal");
    let state = AmbientState::capture(CaptureSet::DEFAULT).expect("capture once");

    let mut resolved = 0_usize;
    for index in 0..8 {
        let file = File::open(tree.file(index)).expect("open a file in the tree");
        let request = QueryFinalPath::new(shared(&file));

        state
            .with_applied(|| request.perform())
            .expect("the context applied")
            .into_clean_value()
            .expect("the thread was restored cleanly")
            .expect("the resolution succeeded");
        resolved += 1;
    }

    assert_eq!(resolved, 8);
}

#[test]
fn requests_are_reusable_across_a_traversal() {
    // A parameter set, not a one-shot ticket: a traversal that revisits a root
    // does not have to rebuild the request.
    let tree = Tree::new("scenario-reuse");
    let request = audited_directory_open(tree.root());

    for _ in 0..8 {
        let handle = request.perform().expect("reperform the same request");
        CloseRequest::for_handle(handle).perform().expect("close");
    }
}

#[test]
fn a_consumer_can_drive_the_catalogue_through_the_seam() {
    // The shape a consumer's own tests need: code written against the trait,
    // driven here by a real entry. The consumer substitutes a fake in its own
    // suite, which is what the seam is for.
    fn open_and_close<R: Request<Output = std::os::windows::io::OwnedHandle>>(
        request: &R,
        times: usize,
    ) -> usize {
        (0..times)
            .filter_map(|_| request.perform().ok())
            .filter(|handle| {
                CloseRequest::for_handle(handle.try_clone().expect("duplicate for the close"))
                    .perform()
                    .is_ok()
            })
            .count()
    }

    let tree = Tree::new("scenario-seam");
    let request = audited_directory_open(tree.root());

    assert_eq!(open_and_close(&request, 4), 4);
}

#[test]
fn the_catalogue_needs_no_ambient_context_of_its_own() {
    // The negative that makes the pairing meaningful: every entry works with no
    // captured context at all, because access was checked at the open. A
    // catalogue that silently required a context would not be a sibling of the
    // ambient crate -- it would be a layer above it.
    let tree = Tree::new("scenario-no-context");
    let directory = open_directory(tree.root());

    QueryFileInformation::new(shared(&directory), FileInformationClass::BASIC)
        .with_capacity(4096)
        .perform()
        .expect("a query with no ambient context whatsoever");
    QueryFinalPath::new(shared(&directory))
        .perform()
        .expect("a resolution with no ambient context whatsoever");
}
