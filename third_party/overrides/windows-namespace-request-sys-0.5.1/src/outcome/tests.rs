// Copyright (c) Mike Grier.

//! Tests for the faithful-execution contract.
//!
//! The load-bearing case is [`the_code_survives_cleanup_that_would_overwrite_it`]:
//! it demonstrates the trap by reproducing it, then shows that binding to
//! [`perform`] closes it. A test that only ever checked a code round-tripped
//! through an otherwise-quiet thread would pass without exercising the
//! guarantee at all.

use std::ptr;

use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_INVALID_HANDLE, ERROR_NOT_ENOUGH_MEMORY,
    ERROR_VIRTDISK_NOT_VIRTUAL_DISK, FALSE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    SetLastError, TRUE,
};

use super::{
    Win32Error, out_of_memory, perform, perform_bool, perform_handle, perform_nonnull_handle,
    perform_nonzero,
};

/// A stand-in for a Win32 call that fails with `code`, setting the thread's
/// last error exactly as Windows would.
fn fails_with<T>(code: u32, result: T) -> impl FnOnce() -> T {
    move || {
        // SAFETY: SetLastError only writes the calling thread's own error slot.
        unsafe { SetLastError(code) };
        result
    }
}

/// A stand-in for cleanup that runs after a failed call and clobbers the
/// thread's last error -- a `CloseHandle` in a `Drop`, a buffer release, a
/// restoration guard unwinding.
fn clobber_last_error() {
    // SAFETY: as above.
    unsafe { SetLastError(ERROR_ACCESS_DENIED) };
}

#[test]
fn a_succeeding_bool_call_yields_no_error() {
    let outcome = perform_bool(|| TRUE);

    assert!(outcome.is_ok());
}

#[test]
fn a_failing_bool_call_reports_the_raw_code() {
    let outcome = perform_bool(fails_with(ERROR_FILE_NOT_FOUND, FALSE));

    assert_eq!(
        outcome.expect_err("FALSE is a failure").code(),
        ERROR_FILE_NOT_FOUND
    );
}

#[test]
fn success_is_not_second_guessed_by_a_stale_last_error() {
    // Many Win32 calls leave a non-zero last error behind on success. An entry
    // that consulted GetLastError on the success path would invent failures.
    clobber_last_error();

    let outcome = perform_bool(|| TRUE);

    assert!(
        outcome.is_ok(),
        "the return value decides success, not the thread's error slot"
    );
}

#[test]
fn the_code_survives_cleanup_that_would_overwrite_it() {
    // First, the trap, reproduced: read the code after cleanup and it is gone.
    let naive = {
        // SAFETY: SetLastError writes only this thread's error slot.
        unsafe { SetLastError(ERROR_FILE_NOT_FOUND) };
        let _failed = FALSE;
        clobber_last_error();
        // SAFETY: GetLastError reads only this thread's error slot.
        unsafe { GetLastError() }
    };
    assert_eq!(
        naive, ERROR_ACCESS_DENIED,
        "the trap must be real, or the guarantee below proves nothing"
    );

    // Now the same sequence through `perform`, which snapshots immediately.
    let outcome = perform_bool(fails_with(ERROR_FILE_NOT_FOUND, FALSE));
    clobber_last_error();

    assert_eq!(
        outcome.expect_err("FALSE is a failure").code(),
        ERROR_FILE_NOT_FOUND,
        "the code is snapshotted before anything else can run"
    );
}

#[test]
fn error_file_not_found_is_passed_through_rather_than_interpreted() {
    // The same code means a missing directory, an empty directory, or a real
    // failure depending on which call produced it and when. Nothing here may
    // decide which.
    let from_open = perform_handle(fails_with(ERROR_FILE_NOT_FOUND, INVALID_HANDLE_VALUE));
    let from_query = perform_bool(fails_with(ERROR_FILE_NOT_FOUND, FALSE));

    assert_eq!(
        from_open
            .expect_err("an invalid handle is a failure")
            .code(),
        ERROR_FILE_NOT_FOUND
    );
    assert_eq!(
        from_query.expect_err("FALSE is a failure").code(),
        ERROR_FILE_NOT_FOUND,
        "two calls, one code, and the crate distinguishes neither"
    );
}

#[test]
fn a_handle_call_uses_the_invalid_handle_value_convention() {
    let sentinel = 0x1234_usize as HANDLE;

    let succeeded = perform_handle(|| sentinel).expect("a real handle is a success");
    let failed = perform_handle(fails_with(ERROR_ACCESS_DENIED, INVALID_HANDLE_VALUE));

    assert_eq!(succeeded, sentinel);
    assert_eq!(
        failed
            .expect_err("INVALID_HANDLE_VALUE is a failure")
            .code(),
        ERROR_ACCESS_DENIED
    );
}

#[test]
fn a_null_returning_call_uses_the_other_handle_convention() {
    let failed = perform_nonnull_handle(fails_with(ERROR_INVALID_HANDLE, ptr::null_mut()));

    assert_eq!(
        failed.expect_err("a null handle is a failure").code(),
        ERROR_INVALID_HANDLE
    );
}

#[test]
fn the_two_handle_conventions_disagree_about_the_same_values() {
    // Getting these the wrong way round turns a failure into a
    // plausible-looking handle, which is why both exist by name.
    assert!(
        perform_handle(ptr::null_mut).is_ok(),
        "null is a success under the INVALID_HANDLE_VALUE convention"
    );
    assert!(
        perform_nonnull_handle(|| INVALID_HANDLE_VALUE).is_ok(),
        "INVALID_HANDLE_VALUE is a success under the null convention"
    );
}

#[test]
fn a_nonzero_call_reports_its_length_or_the_raw_code() {
    let succeeded = perform_nonzero(|| 42).expect("a non-zero length is a success");
    let failed = perform_nonzero(fails_with(ERROR_ACCESS_DENIED, 0));

    assert_eq!(succeeded, 42);
    assert_eq!(
        failed.expect_err("zero is a failure").code(),
        ERROR_ACCESS_DENIED
    );
}

#[test]
fn the_general_form_accepts_a_convention_of_its_own() {
    // A call that signals failure some other way still gets the same snapshot
    // guarantee, which is why the general form is public.
    let outcome = perform(fails_with(ERROR_FILE_NOT_FOUND, -1_i64), |result| {
        *result < 0
    });

    assert_eq!(
        outcome.expect_err("a negative result is a failure").code(),
        ERROR_FILE_NOT_FOUND
    );
}

#[test]
fn an_unknown_code_is_carried_rather_than_rejected() {
    // The type is not an enum precisely so a code this crate has never heard of
    // still reaches the consumer intact.
    let unknown = 0x0BAD_F00D_u32;

    let outcome = perform_bool(fails_with(unknown, FALSE));

    assert_eq!(outcome.expect_err("FALSE is a failure").code(), unknown);
}

#[test]
fn a_code_of_two_to_the_thirty_first_or_more_survives_the_io_error_form() {
    // `ERROR_VIRTDISK_NOT_VIRTUAL_DISK`, which std holds as a negative number and which this crate's
    // own earlier type turned into a message with no code. Taken through `perform`, as an entry
    // takes it, and cleanup that would overwrite it runs before it is read.
    let outcome = perform_bool(fails_with(ERROR_VIRTDISK_NOT_VIRTUAL_DISK, FALSE));
    clobber_last_error();
    let error = outcome.expect_err("FALSE is a failure");

    assert_eq!(error.code(), ERROR_VIRTDISK_NOT_VIRTUAL_DISK);
    let io_error = error.to_io_error();
    assert_eq!(
        io_error.raw_os_error(),
        Some(ERROR_VIRTDISK_NOT_VIRTUAL_DISK.cast_signed()),
        "the code is in the io::Error, not replaced by a message"
    );
    assert_eq!(Win32Error::from_io(&io_error), Some(error));
}

#[test]
fn this_crates_own_out_of_memory_report_is_the_code_for_it() {
    assert_eq!(out_of_memory().code(), ERROR_NOT_ENOUGH_MEMORY);
}

#[test]
fn an_error_renders_and_converts_without_losing_its_code() {
    let error = Win32Error::from_code(ERROR_FILE_NOT_FOUND);

    assert_eq!(
        error.to_io_error().raw_os_error(),
        Some(ERROR_FILE_NOT_FOUND as i32),
        "the io::Error form is a re-presentation, not a reclassification"
    );
    assert!(
        error
            .to_string()
            .contains(&ERROR_FILE_NOT_FOUND.to_string()),
        "unexpected message: {error}"
    );
}

#[test]
fn errors_are_comparable_and_cheap_to_carry() {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}
    const fn assert_copy<T: Copy>() {}

    assert_send::<Win32Error>();
    assert_sync::<Win32Error>();
    assert_copy::<Win32Error>();

    assert_eq!(
        Win32Error::from_code(ERROR_FILE_NOT_FOUND),
        Win32Error::from_code(ERROR_FILE_NOT_FOUND)
    );
    assert_ne!(
        Win32Error::from_code(ERROR_FILE_NOT_FOUND),
        Win32Error::from_code(ERROR_ACCESS_DENIED)
    );
}
