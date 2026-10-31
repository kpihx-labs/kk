// Copyright (c) Mike Grier.

//! Operation coverage: every audited call site, re-expressed.
//!
//! One test per audited call site, named for the consumer and the call, so a
//! gap shows up as a missing test rather than as a paragraph nobody re-reads.
//! Each asserts the *parameter shape* that consumer actually uses is reachable,
//! not merely that the entry exists.
//!
//! The three consumers are this repository's `windows-file-watcher` and
//! `windows-file-enumeration-sys`, and `MikeGrier/Globazog-rs` at `55a0b1ae`.

use std::fs::File;
use std::os::windows::io::AsHandle;

use win_shared_os_owned_handle::SharedHandle;
use windows_namespace_request_sys::close::CloseRequest;
use windows_namespace_request_sys::file_info::QueryFileInformationByHandle;
use windows_namespace_request_sys::final_path::{FinalPathFlags, QueryFinalPath};
use windows_namespace_request_sys::full_path::ResolveFullPath;
use windows_namespace_request_sys::open_by_id::{FileIdentifier, OpenFileByIdentifier};
use windows_namespace_request_sys::query::{FileInformationClass, QueryFileInformation};
use windows_namespace_request_sys::volume::QueryVolumeInformation;
use windows_namespace_request_sys::watch::NotifyFilter;
use windows_sys::Win32::Foundation::ERROR_NO_MORE_FILES;
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ, FILE_LIST_DIRECTORY,
    FILE_SHARE_READ, FileStandardInfo, OPEN_EXISTING,
};
use wtf_string::Wtf16String;

use crate::support::{
    AUDITED_SHARE, Tree, audited_directory_open, open_directory, open_file, watch_directory,
};

fn shared(file: &File) -> SharedHandle {
    SharedHandle::from(
        file.as_handle()
            .try_clone_to_owned()
            .expect("own the handle"),
    )
}

// -- Entry 1: CreateFileW -------------------------------------------------

#[test]
fn watcher_createfilew_shape_is_reachable() {
    // directory.rs:383 -- FILE_LIST_DIRECTORY, share R|W|D, OPEN_EXISTING,
    // BACKUP_SEMANTICS | OVERLAPPED, null security, null template.
    let tree = Tree::new("op-watcher-open");

    let handle = audited_directory_open(tree.root())
        .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED)
        .with_security(None)
        .with_template(None)
        .perform()
        .expect("the watcher's open shape");

    CloseRequest::for_handle(handle).perform().expect("close");
}

#[test]
fn enumeration_createfilew_shape_is_reachable() {
    // native.rs:83 -- the same, without OVERLAPPED.
    let tree = Tree::new("op-enum-open");

    let handle = audited_directory_open(tree.root())
        .perform()
        .expect("the enumeration crate's open shape");

    CloseRequest::for_handle(handle).perform().expect("close");
}

#[test]
fn globazog_canonicalize_shape_is_reachable() {
    // Globazog reaches CreateFileW + GetFinalPathNameByHandleW + CloseHandle
    // through std::fs::canonicalize, on its submitting thread, per root.
    let tree = Tree::new("op-globazog-canonicalize");

    let handle = open_file(tree.root())
        .with_desired_access(0)
        .with_share_mode(AUDITED_SHARE)
        .with_creation_disposition(OPEN_EXISTING)
        .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS)
        .perform()
        .expect("open for canonicalisation");

    let file = File::from(handle);
    let resolved = QueryFinalPath::new(shared(&file))
        .perform()
        .expect("resolve the final path")
        .to_string_lossy();
    assert!(resolved.starts_with(r"\\?\"), "unexpected: {resolved}");

    CloseRequest::for_handle(file.into())
        .perform()
        .expect("close");
}

// -- Entry 2: OpenFileById ------------------------------------------------

#[test]
fn watcher_openfilebyid_shape_is_reachable() {
    // directory.rs:451 -- volume hint, FILE_ID_DESCRIPTOR by file id,
    // FILE_LIST_DIRECTORY, share R|W|D, BACKUP_SEMANTICS | OVERLAPPED.
    let tree = Tree::new("op-watcher-byid");
    let hint = open_directory(tree.root());
    let id = QueryFileInformationByHandle::new(shared(&hint))
        .perform()
        .expect("read the directory identity");
    let file_id = (u64::from(id.nFileIndexHigh) << 32) | u64::from(id.nFileIndexLow);

    let reopened = OpenFileByIdentifier::new(shared(&hint), FileIdentifier::FileId(file_id))
        .with_desired_access(FILE_LIST_DIRECTORY)
        .with_share_mode(AUDITED_SHARE)
        .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED)
        .perform()
        .expect("the watcher's reopen-by-id shape");

    CloseRequest::for_handle(reopened).perform().expect("close");
}

// -- Entry 3: FindFirstChangeNotificationW --------------------------------

#[test]
fn watcher_findfirstchangenotification_shape_is_reachable() {
    // coarse.rs:54 -- path, subtree flag, FILE_NOTIFY_CHANGE_* mask.
    let tree = Tree::new("op-watcher-notify");

    let notification = watch_directory(tree.root())
        .with_subtree(true)
        .with_filter(
            NotifyFilter::FILE_NAME
                | NotifyFilter::DIR_NAME
                | NotifyFilter::LAST_WRITE
                | NotifyFilter::SIZE,
        )
        .perform()
        .expect("the watcher's notification shape");

    // coarse.rs:95 -- and it closes with FindCloseChangeNotification.
    CloseRequest::for_change_notification(notification)
        .perform()
        .expect("close with the right routine");
}

// -- Entry 4: CloseHandle and variants ------------------------------------

#[test]
fn both_close_routines_are_reachable() {
    // Every consumer closes ordinary handles; only the watcher closes a
    // notification, and not with CloseHandle.
    let tree = Tree::new("op-closes");

    let ordinary = audited_directory_open(tree.root())
        .perform()
        .expect("open a directory");
    let notification = watch_directory(tree.root())
        .with_filter(NotifyFilter::FILE_NAME)
        .perform()
        .expect("start a watch");

    CloseRequest::for_handle(ordinary)
        .perform()
        .expect("CloseHandle");
    CloseRequest::for_change_notification(notification)
        .perform()
        .expect("FindCloseChangeNotification");
}

// -- Entry 5: GetFileInformationByHandleEx --------------------------------

#[test]
fn all_five_audited_information_classes_are_reachable() {
    // watcher directory.rs:348 (FileCaseSensitiveInfo); enumeration
    // native.rs:136/169/267 (FileBasicInfo, FileIdInfo, and the two directory
    // classes).
    let tree = Tree::new("op-classes");
    let directory = open_directory(tree.root());

    for class in [
        FileInformationClass::BASIC,
        FileInformationClass::ID,
        FileInformationClass::CASE_SENSITIVE,
        FileInformationClass::ID_EXTD_DIRECTORY_RESTART,
        FileInformationClass::ID_EXTD_DIRECTORY,
    ] {
        let request = QueryFileInformation::new(shared(&directory), class).with_capacity(4096);
        assert_eq!(request.class(), class);

        // Reachability is what this asserts, and `ERROR_NO_MORE_FILES` is a
        // successful reach: the restart above drains this small directory, so
        // the continuation legitimately has nothing left. Treating it as a
        // failure would make the test assert something it does not mean.
        match request.perform() {
            Ok(_) => {}
            Err(error) if error.code() == ERROR_NO_MORE_FILES => {
                assert!(
                    class.moves_enumeration_cursor(),
                    "only an enumeration class can exhaust: {class:?}"
                );
            }
            Err(error) => panic!("class {class:?} was not reachable: {error}"),
        }
    }
}

#[test]
fn an_unaudited_information_class_is_also_reachable() {
    // The entry is not narrowed to the audited five.
    let tree = Tree::new("op-class-unaudited");
    let file = File::open(tree.file(0)).expect("open a file");

    QueryFileInformation::new(
        shared(&file),
        FileInformationClass::from_raw(FileStandardInfo),
    )
    .with_capacity(4096)
    .perform()
    .expect("a class with no named constant here");
}

// -- Entry 6: GetFileInformationByHandle (non-Ex) -------------------------

#[test]
fn watcher_getfileinformationbyhandle_shape_is_reachable() {
    // directory.rs:314 -- the non-Ex call, for the volume serial and file index
    // the watcher uses as an identity.
    let tree = Tree::new("op-watcher-fileinfo");
    let directory = open_directory(tree.root());

    let information = QueryFileInformationByHandle::new(shared(&directory))
        .perform()
        .expect("the watcher's identity query");

    assert_ne!(information.dwVolumeSerialNumber, 0);
    assert_ne!(
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
        0
    );
}

// -- Entry 7: GetFinalPathNameByHandleW -----------------------------------

#[test]
fn watcher_getfinalpathnamebyhandle_shape_is_reachable() {
    // directory.rs:696 -- VOLUME_NAME_DOS | FILE_NAME_NORMALIZED.
    let tree = Tree::new("op-watcher-finalpath");
    let directory = open_directory(tree.root());

    let resolved = QueryFinalPath::new(shared(&directory))
        .with_flags(FinalPathFlags::VOLUME_NAME_DOS | FinalPathFlags::NAME_NORMALIZED)
        .perform()
        .expect("the watcher's final-path shape")
        .to_string_lossy();

    assert!(resolved.starts_with(r"\\?\"), "unexpected: {resolved}");
}

// -- Entry 8: GetVolumeInformationByHandleW -------------------------------

#[test]
fn watcher_getvolumeinformationbyhandle_shape_is_reachable() {
    // directory.rs:655 -- label, serial, and filesystem name. The watcher
    // passes NULL for the component-length and flags out-params; this entry
    // fills them anyway, so it is *wider* than the call site rather than
    // narrower, which is the correct direction.
    let tree = Tree::new("op-watcher-volume");
    let directory = open_directory(tree.root());

    let volume = QueryVolumeInformation::new(shared(&directory))
        .perform()
        .expect("the watcher's volume query");

    assert_ne!(volume.serial_number(), 0);
    assert!(!volume.filesystem_name().is_empty());
    assert!(volume.maximum_component_length() > 0);
}

// -- Entry 9: GetFullPathNameW --------------------------------------------

#[test]
fn enumeration_getfullpathname_shape_is_reachable() {
    // enumeration path.rs:149 -- resolution with a null file-part out-param.
    // Qualified by crate because this is the one consumer filename that also
    // exists in THIS crate, and a reviewer resolved it against the local
    // `src/path.rs` -- where line 149 is unrelated -- and reported the citation
    // as stale. It is not. The neighbouring "watcher directory.rs" /
    // "enumeration native.rs" citations already use this form for the same
    // reason.
    let resolved = ResolveFullPath::new(Wtf16String::from(r"C:\Windows\System32\..\.\Temp"))
        .expect("the path has no NUL")
        .perform()
        .expect("the enumeration crate's resolution shape")
        .to_string_lossy();

    assert_eq!(resolved, r"C:\Windows\Temp");
}

// -- The parameter shapes no consumer uses, kept anyway -------------------

#[test]
fn the_unused_createfilew_parameters_are_still_expressible() {
    // No audited consumer passes a security descriptor or a template file. An
    // entry that could not express two of its own call's parameters would be a
    // narrowed CreateFileW, so both stay reachable.
    let tree = Tree::new("op-unused-params");
    let template_source = File::open(tree.file(0)).expect("open a file");

    let request = open_file(&tree.file(1))
        .with_desired_access(FILE_GENERIC_READ)
        .with_share_mode(FILE_SHARE_READ)
        .with_creation_disposition(OPEN_EXISTING)
        .with_template(Some(shared(&template_source)));

    assert!(request.template().is_some());
    let handle = request.perform().expect("open with a template");
    CloseRequest::for_handle(handle).perform().expect("close");
}
