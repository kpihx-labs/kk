// Copyright (c) Mike Grier.

//! The entries composed, which is what no single-entry test reaches.
//!
//! The shape that matters is a chain: a handle opened by one request is carried
//! into a later one, across a thread boundary, with every input the requests
//! were built from already gone.

use std::fs::File;
use std::os::windows::io::AsHandle;
use std::time::Duration;

use win_shared_os_owned_handle::SharedHandle;
use windows_namespace_request_sys::close::CloseRequest;
use windows_namespace_request_sys::open_by_id::{FileIdentifier, OpenFileByIdentifier};
use windows_namespace_request_sys::watch::{ChangeNotification, NotifyFilter};
use windows_sys::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_LIST_DIRECTORY};
use windows_sys::Win32::System::Threading::WaitForSingleObject;

use crate::support::{
    AUDITED_SHARE, Tree, file_id_of, open_directory, unassociated_directory, watch_directory,
};

/// Long enough that a working watch signals well inside it, short enough that a
/// broken one fails rather than hangs.
const SIGNAL_TIMEOUT: Duration = Duration::from_secs(5);

fn signalled_within(notification: &ChangeNotification, timeout: Duration) -> bool {
    let millis = u32::try_from(timeout.as_millis()).expect("the timeout fits in a u32");
    // SAFETY: the handle is live for the notification's lifetime.
    match unsafe { WaitForSingleObject(notification.as_raw(), millis) } {
        WAIT_OBJECT_0 => true,
        WAIT_TIMEOUT => false,
        other => panic!("the wait failed unexpectedly: {other}"),
    }
}

#[test]
fn a_handle_opened_by_one_entry_is_carried_into_a_later_one() {
    // This is the combination the audit called out: an open produces a handle
    // that becomes the *input* to a reopen-by-id. Nothing but a composed test
    // exercises it.
    let tree = Tree::new("compose-carry");

    let opened = unassociated_directory(tree.root())
        .perform()
        .expect("open the tree root");
    let opened = File::from(opened);
    let id = file_id_of(&opened);

    let reopened = OpenFileByIdentifier::new(
        SharedHandle::from(
            opened
                .as_handle()
                .try_clone_to_owned()
                .expect("own the opened handle as a hint"),
        ),
        FileIdentifier::FileId(id),
    )
    .with_desired_access(FILE_LIST_DIRECTORY)
    .with_share_mode(AUDITED_SHARE)
    .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS)
    .perform()
    .expect("reopen the same directory by its identifier");

    assert!(
        File::from(reopened)
            .metadata()
            .expect("read the reopened directory's metadata")
            .is_dir()
    );
}

#[test]
fn a_reopen_survives_its_source_handle_being_closed_first() {
    // The volume hint is never the object being reopened, so the handle the
    // identifier came from can be closed -- through the close entry -- before
    // the reopen runs.
    let tree = Tree::new("compose-source-closed");

    let hint = open_directory(tree.root());
    let target = File::open(tree.file(0)).expect("open a file in the tree");
    let id = file_id_of(&target);

    let request = OpenFileByIdentifier::new(
        SharedHandle::from(
            hint.as_handle()
                .try_clone_to_owned()
                .expect("own the volume hint"),
        ),
        FileIdentifier::FileId(id),
    )
    .with_desired_access(FILE_LIST_DIRECTORY)
    .with_share_mode(AUDITED_SHARE);

    // Close the source through the catalogue rather than by dropping it, so the
    // close entry is part of the chain being proven.
    CloseRequest::for_handle(target.into())
        .perform()
        .expect("close the source handle first");
    drop(hint);

    let reopened = request
        .perform()
        .expect("the reopen outlives both the source handle and the hint");

    CloseRequest::for_handle(reopened)
        .perform()
        .expect("close the reopened handle");
}

#[test]
fn the_whole_chain_runs_on_a_worker_that_saw_none_of_its_inputs() {
    // Open, reopen by id, watch, and close -- every entry in this milestone --
    // performed on a thread that never saw the tree, the paths, or the handles
    // they were built from.
    let tree = Tree::new("compose-worker");

    let hint = open_directory(tree.root());
    let id = file_id_of(&hint);

    let open_request = unassociated_directory(tree.root());
    let reopen_request = OpenFileByIdentifier::new(
        SharedHandle::from(
            hint.as_handle()
                .try_clone_to_owned()
                .expect("own the volume hint"),
        ),
        FileIdentifier::FileId(id),
    )
    .with_desired_access(FILE_LIST_DIRECTORY)
    .with_share_mode(AUDITED_SHARE)
    .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS);
    let watch_request = watch_directory(tree.root()).with_filter(NotifyFilter::FILE_NAME);

    drop(hint);

    let worker = std::thread::spawn(move || {
        let opened = open_request.perform().expect("open on the worker");
        let reopened = reopen_request.perform().expect("reopen on the worker");
        let notification = watch_request.perform().expect("watch on the worker");

        CloseRequest::for_handle(opened)
            .perform()
            .expect("close the opened handle on the worker");
        CloseRequest::for_handle(reopened)
            .perform()
            .expect("close the reopened handle on the worker");

        notification
    });

    let notification = worker.join().expect("the worker did not panic");

    // The watch started on the worker still signals for a change made here.
    std::fs::write(tree.root().join("from-the-test.t"), b"x").expect("create a file");
    assert!(
        signalled_within(&notification, SIGNAL_TIMEOUT),
        "a watch started on a worker observes changes made anywhere"
    );

    CloseRequest::for_change_notification(notification)
        .perform()
        .expect("close the notification with its own routine");
}

#[test]
fn many_requests_from_one_capture_run_across_concurrent_workers() {
    // Globazog's shape: one capture taken once, shared by many workers. The
    // requests are built up front and each worker performs its own.
    const WORKERS: usize = 16;

    let tree = Tree::new("compose-concurrent");

    let requests: Vec<_> = (0..WORKERS)
        .map(|_| unassociated_directory(tree.root()))
        .collect();

    let workers: Vec<_> = requests
        .into_iter()
        .map(|request| {
            std::thread::spawn(move || {
                let opened = request.perform().expect("open on a worker");
                CloseRequest::for_handle(opened)
                    .perform()
                    .expect("close on the same worker");
            })
        })
        .collect();

    for worker in workers {
        worker.join().expect("every worker succeeded");
    }
}

#[test]
fn a_watch_and_an_open_observe_the_same_directory_at_once() {
    let tree = Tree::new("compose-watch-open");

    let notification = watch_directory(tree.root())
        .with_filter(NotifyFilter::FILE_NAME)
        .with_subtree(true)
        .perform()
        .expect("watch the tree root");
    let opened = unassociated_directory(tree.root())
        .perform()
        .expect("open the tree root while it is watched");

    // A change below the root, which the subtree watch must see even though the
    // directory is simultaneously held open by another entry.
    std::fs::write(tree.child().join("deep.t"), b"x").expect("create a file in the child");

    assert!(signalled_within(&notification, SIGNAL_TIMEOUT));

    CloseRequest::for_handle(opened)
        .perform()
        .expect("close the open handle");
    CloseRequest::for_change_notification(notification)
        .perform()
        .expect("close the notification");
}
