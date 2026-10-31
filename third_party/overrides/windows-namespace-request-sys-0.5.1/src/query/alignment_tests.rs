// Copyright (c) Mike Grier.

//! Tests for the alignment `perform_into` requires of a buffer, where the page of the structure a
//! class writes requires one.
//!
//! The check is a pure function of the class and the address, so most of it is tested in memory,
//! in both directions. The end-to-end tests use zero-length buffers, which lie at their stated
//! alignment, to reach a misaligned address deterministically.

use std::fs::File;
use std::os::windows::io::AsHandle;

use windows_sys::Win32::Storage::FileSystem::{
    FILE_FULL_DIR_INFO, FILE_INFO_BY_HANDLE_CLASS, FileBasicInfo, FileFullDirectoryInfo,
    FileFullDirectoryRestartInfo, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo,
    FileIdExtdDirectoryInfo, FileIdExtdDirectoryRestartInfo, FileIdInfo, FileNameInfo,
    FileStandardInfo, FileStreamInfo,
};

use super::{
    BATCH_ALIGNMENT, FileInformationClass, PerformIntoError, QueryFileInformation, check_alignment,
    required_alignment,
};
use crate::handle::tests::{Fixture, handle_allocation};
use io_buffers::AlignedBuf;
use win_shared_os_owned_handle::SharedHandle;

/// The five classes whose structure's page requires 8-byte alignment, with the structure.
const REQUIRING: [(FILE_INFO_BY_HANDLE_CLASS, &str); 5] = [
    (FileStreamInfo, "FILE_STREAM_INFO"),
    (FileIdBothDirectoryInfo, "FILE_ID_BOTH_DIR_INFO"),
    (FileIdBothDirectoryRestartInfo, "FILE_ID_BOTH_DIR_INFO"),
    (FileFullDirectoryInfo, "FILE_FULL_DIR_INFO"),
    (FileFullDirectoryRestartInfo, "FILE_FULL_DIR_INFO"),
];

fn class(raw: FILE_INFO_BY_HANDLE_CLASS) -> FileInformationClass {
    FileInformationClass::from_raw(raw)
}

fn query(file: &File, class: FileInformationClass) -> QueryFileInformation {
    QueryFileInformation::new(
        SharedHandle::from(
            file.as_handle()
                .try_clone_to_owned()
                .expect("own the handle"),
        ),
        class,
    )
}

// --- The table, in memory ---------------------------------------------------------------------

#[test]
fn the_five_classes_require_eight_bytes_for_the_structure_their_page_names() {
    for (raw, structure) in REQUIRING {
        assert_eq!(
            required_alignment(class(raw)),
            Some((structure, 8)),
            "class {raw}"
        );
    }
}

#[test]
fn no_other_class_requires_an_alignment() {
    // Every class value up to well past the last the page lists, and the extremes.
    let requiring: Vec<_> = REQUIRING.iter().map(|(raw, _)| *raw).collect();
    for raw in (0..64).chain([i32::MAX, i32::MIN, -1]) {
        if !requiring.contains(&raw) {
            assert_eq!(required_alignment(class(raw)), None, "class {raw}");
        }
    }
}

#[test]
fn the_classes_this_crate_names_require_none() {
    for named in [
        FileInformationClass::BASIC,
        FileInformationClass::ID,
        FileInformationClass::CASE_SENSITIVE,
        FileInformationClass::ID_EXTD_DIRECTORY,
        FileInformationClass::ID_EXTD_DIRECTORY_RESTART,
    ] {
        assert_eq!(required_alignment(named), None, "{named:?}");
    }
}

#[test]
fn every_required_alignment_divides_the_allocating_forms() {
    for (raw, _) in REQUIRING {
        let (_, required) = required_alignment(class(raw)).expect("required");
        assert_eq!(BATCH_ALIGNMENT % required, 0, "class {raw}");
    }
}

// --- The check, in memory: refused and accepted -----------------------------------------------

#[test]
fn an_address_off_the_boundary_is_refused_for_each_requiring_class() {
    for (raw, structure) in REQUIRING {
        for address in [1usize, 2, 3, 4, 5, 6, 7, 9, 12, 0x1004, usize::MAX] {
            match check_alignment(class(raw), address) {
                Err(PerformIntoError::Misaligned {
                    class: refused,
                    structure: named,
                    required,
                    address: at,
                }) => {
                    assert_eq!(refused, class(raw));
                    assert_eq!(named, structure);
                    assert_eq!(required, 8);
                    assert_eq!(at, address);
                }
                other => panic!("class {raw} at {address:#x}: {other:?}"),
            }
        }
    }
}

#[test]
fn an_address_on_the_boundary_is_accepted_for_each_requiring_class() {
    for (raw, _) in REQUIRING {
        for address in [0usize, 8, 16, 64, 0x1000, 0x1008, usize::MAX - 7] {
            assert!(
                check_alignment(class(raw), address).is_ok(),
                "class {raw} at {address:#x}"
            );
        }
    }
}

#[test]
fn any_address_is_accepted_for_a_class_that_requires_none() {
    for raw in [
        FileBasicInfo,
        FileStandardInfo,
        FileNameInfo,
        FileIdInfo,
        FileIdExtdDirectoryInfo,
        FileIdExtdDirectoryRestartInfo,
        0x7fff,
    ] {
        for address in [1usize, 2, 3, 4, 7, 8, 0x1001] {
            assert!(
                check_alignment(class(raw), address).is_ok(),
                "class {raw} at {address:#x}"
            );
        }
    }
}

// --- The error ---------------------------------------------------------------------------------

#[test]
fn a_refusal_carries_no_code_and_names_the_structure() {
    let error = check_alignment(class(FileFullDirectoryInfo), 4).expect_err("refused");
    assert_eq!(error.code(), None);
    assert!(std::error::Error::source(&error).is_none());
    let text = error.to_string();
    assert!(text.contains("FILE_FULL_DIR_INFO"), "{text}");
    assert!(text.contains("8-byte"), "{text}");
    assert!(text.contains("0x4"), "{text}");
}

#[test]
fn a_calls_error_carries_its_code_unaltered() {
    let error = PerformIntoError::from(crate::Win32Error::from_code(0xC03A_0015));
    assert_eq!(error.code(), Some(0xC03A_0015));
    assert!(std::error::Error::source(&error).is_some());
}

// --- End to end --------------------------------------------------------------------------------

#[test]
fn a_misaligned_buffer_is_refused_before_the_call_for_each_requiring_class() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::with_extra_files("query-misaligned", 3);
    let directory = fixture.open_directory();
    for (raw, structure) in REQUIRING {
        for align in [1usize, 2, 4] {
            let mut buffer = AlignedBuf::zeroed(0, align).expect("an empty buffer");
            assert_eq!(
                buffer.as_ptr().addr(),
                align,
                "an empty buffer lies at its alignment"
            );
            let error = query(&directory, class(raw))
                .perform_into(&mut buffer)
                .expect_err("misaligned");
            assert!(
                matches!(error, PerformIntoError::Misaligned { structure: s, .. } if s == structure),
                "class {raw}, align {align}: {error:?}"
            );
            assert_eq!(error.code(), None, "the call was not made");
        }
    }
}

#[test]
fn an_aligned_empty_buffer_reaches_the_call_for_each_requiring_class() {
    // The other direction: at 8 the check passes and Windows answers, with its own code.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::with_extra_files("query-aligned-empty", 3);
    let directory = fixture.open_directory();
    for (raw, _) in REQUIRING {
        let mut buffer = AlignedBuf::zeroed(0, 8).expect("an empty buffer");
        let error = query(&directory, class(raw))
            .perform_into(&mut buffer)
            .expect_err("no room");
        assert!(
            matches!(error, PerformIntoError::Win32(_)),
            "class {raw}: {error:?}"
        );
    }
}

#[test]
fn a_full_directory_class_enumerates_into_an_aligned_buffer() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::with_extra_files("query-full-dir", 3);
    let directory = fixture.open_directory();
    let mut buffer = AlignedBuf::zeroed(64 * 1024, 8).expect("a buffer");
    query(&directory, class(FileFullDirectoryRestartInfo))
        .perform_into(&mut buffer)
        .expect("the first batch");
    // The first record is a real entry, so its name length is non-zero.
    let at = std::mem::offset_of!(FILE_FULL_DIR_INFO, FileNameLength);
    let name_length = u32::from_ne_bytes(buffer[at..at + 4].try_into().expect("four bytes"));
    assert_ne!(name_length, 0);
}

#[test]
fn a_misaligned_buffer_reaches_the_call_for_a_class_that_requires_none() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::with_extra_files("query-misaligned-extd", 3);
    let directory = fixture.open_directory();
    let mut buffer = AlignedBuf::zeroed(0, 1).expect("an empty buffer");
    let error = query(&directory, FileInformationClass::ID_EXTD_DIRECTORY_RESTART)
        .perform_into(&mut buffer)
        .expect_err("no room");
    assert!(matches!(error, PerformIntoError::Win32(_)), "{error:?}");
}
