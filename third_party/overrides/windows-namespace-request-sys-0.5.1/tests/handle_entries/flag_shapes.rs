// Copyright (c) Mike Grier.

//! The three audited flag shapes, and the close routine each opened handle
//! needs, against a real directory.

use std::fs::File;

use windows_namespace_request_sys::close::CloseRequest;
use windows_namespace_request_sys::watch::NotifyFilter;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ, FILE_SHARE_READ,
    OPEN_EXISTING,
};

use crate::support::{AUDITED_SHARE, Tree, open_file, unassociated_directory, watch_directory};

#[test]
fn the_enumeration_shape_opens_a_real_directory() {
    let tree = Tree::new("shape-enumeration");

    let handle = unassociated_directory(tree.root())
        .perform()
        .expect("open the tree root for listing");

    assert!(
        File::from(handle)
            .metadata()
            .expect("read the directory metadata")
            .is_dir()
    );
}

#[test]
fn the_watcher_shape_opens_the_same_directory_with_the_overlapped_flag() {
    let tree = Tree::new("shape-watcher");

    let handle = unassociated_directory(tree.root())
        .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED)
        .perform()
        .expect("open the tree root for overlapped use");

    // The handle comes back plain and unassociated: nothing here has bound it
    // to a completion port, which is what keeps the IoRing fork open.
    CloseRequest::for_handle(handle)
        .perform()
        .expect("close the overlapped handle");
}

#[test]
fn the_file_shape_opens_an_ordinary_file() {
    let tree = Tree::new("shape-file");

    let handle = open_file(&tree.file(0))
        .with_desired_access(FILE_GENERIC_READ)
        .with_share_mode(FILE_SHARE_READ)
        .with_creation_disposition(OPEN_EXISTING)
        .perform()
        .expect("open a file in the tree");

    assert_eq!(
        File::from(handle)
            .metadata()
            .expect("read the file metadata")
            .len(),
        b"contents".len() as u64
    );
}

#[test]
fn all_three_shapes_open_the_same_tree_concurrently() {
    // The share mode the audit found is what makes this possible: three
    // consumers can hold the same directory open at once.
    let tree = Tree::new("shape-concurrent");

    let enumeration = unassociated_directory(tree.root())
        .perform()
        .expect("the enumeration shape opens");
    let watcher = unassociated_directory(tree.root())
        .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED)
        .perform()
        .expect("the watcher shape opens alongside it");
    let file = open_file(&tree.file(1))
        .with_desired_access(FILE_GENERIC_READ)
        .with_share_mode(AUDITED_SHARE)
        .with_creation_disposition(OPEN_EXISTING)
        .perform()
        .expect("a file opens alongside both");

    for handle in [enumeration, watcher, file] {
        CloseRequest::for_handle(handle)
            .perform()
            .expect("each handle closes");
    }
}

#[test]
fn a_notification_handle_needs_its_own_close_routine() {
    // The pairing a close entry cannot assume. Both handles below are closed
    // through the same entry, and the entry uses a different routine for each
    // because the handle carries it.
    let tree = Tree::new("shape-close-routines");

    let directory = unassociated_directory(tree.root())
        .perform()
        .expect("open the tree root");
    let notification = watch_directory(tree.root())
        .with_filter(NotifyFilter::FILE_NAME)
        .perform()
        .expect("watch the tree root");

    let ordinary = CloseRequest::for_handle(directory);
    let custom = CloseRequest::for_change_notification(notification);

    assert!(format!("{ordinary:?}").contains("CloseHandle"));
    assert!(format!("{custom:?}").contains("FindCloseChangeNotification"));

    ordinary.perform().expect("close the directory handle");
    custom.perform().expect("close the notification handle");
}

#[test]
fn a_missing_directory_fails_in_every_shape_with_the_raw_code() {
    let tree = Tree::new("shape-missing");
    let absent = tree.root().join("no-such-directory");

    let plain = unassociated_directory(&absent).perform();
    let overlapped = unassociated_directory(&absent)
        .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED)
        .perform();
    let watch = watch_directory(&absent)
        .with_filter(NotifyFilter::FILE_NAME)
        .perform();

    assert!(plain.is_err());
    assert!(overlapped.is_err());
    assert!(watch.is_err());
}
