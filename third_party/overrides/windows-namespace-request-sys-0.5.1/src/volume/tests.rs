// Copyright (c) Mike Grier.

//! Tests for the `GetVolumeInformationByHandleW` entry.

use std::fs::File;
use std::os::windows::io::AsHandle;

use super::{QueryVolumeInformation, VolumeInformation};
use crate::handle::tests::{Fixture, handle_allocation};
use win_shared_os_owned_handle::SharedHandle;
use wtf_string::Wtf16String;

fn request_for(file: &File) -> QueryVolumeInformation {
    QueryVolumeInformation::new(SharedHandle::from(
        file.as_handle()
            .try_clone_to_owned()
            .expect("own the handle"),
    ))
}

#[test]
fn a_real_volume_reports_its_filesystem_and_limits() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("volume-basic");
    let file = fixture.open_file();

    let volume = request_for(&file).perform().expect("query the volume");

    assert!(
        !volume.filesystem_name().is_empty(),
        "a real volume names its filesystem"
    );
    assert!(volume.maximum_component_length() > 0);
    assert_ne!(volume.serial_number(), 0);
}

#[test]
fn the_filesystem_name_is_terminated_rather_than_padded() {
    // The call reports no length for either string buffer, so the terminator is
    // the only signal. A buffer restored to its full capacity would carry
    // trailing NULs into the result.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("volume-terminator");
    let file = fixture.open_file();

    let volume = request_for(&file).perform().expect("query the volume");
    let name = volume.filesystem_name().to_string_lossy();

    assert!(
        !name.contains('\0'),
        "the name must stop at the terminator: {name:?}"
    );
    assert_eq!(
        name.trim().len(),
        name.len(),
        "and must not be padded: {name:?}"
    );
    assert!(name.len() < 32, "a filesystem name is short: {name:?}");
}

#[test]
fn a_directory_handle_reports_the_same_volume_as_a_file_on_it() {
    // The handle names any object on the volume; the volume is what is
    // reported.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("volume-agree");
    let file = fixture.open_file();
    let directory = fixture.open_directory();

    let from_file = request_for(&file).perform().expect("query via the file");
    let from_directory = request_for(&directory)
        .perform()
        .expect("query via the directory");

    assert_eq!(from_file.serial_number(), from_directory.serial_number());
    assert_eq!(
        from_file.filesystem_name().to_string_lossy(),
        from_directory.filesystem_name().to_string_lossy()
    );
}

#[test]
fn the_capability_flags_are_carried_unaltered() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("volume-flags");
    let file = fixture.open_file();

    let volume = request_for(&file).perform().expect("query the volume");

    // Not asserting *which* flags: the point is that the raw mask arrives, so a
    // capability Windows adds later still reaches a consumer.
    assert_ne!(
        volume.flags(),
        0,
        "any real filesystem reports some capabilities"
    );
}

#[test]
fn the_display_form_names_the_filesystem_and_serial() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("volume-display");
    let file = fixture.open_file();

    let volume = request_for(&file).perform().expect("query the volume");
    let rendered = volume.to_string();

    assert!(
        rendered.contains(&volume.filesystem_name().to_string_lossy()),
        "unexpected: {rendered}"
    );
}

#[test]
fn a_clone_shares_the_handle_and_reports_the_same_volume() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("volume-copy");
    let file = fixture.open_file();
    let request = request_for(&file);

    let copy = request.clone();

    assert!(copy.handle().same_handle(request.handle()));
    assert_eq!(
        copy.perform().expect("the copy queries").serial_number(),
        request
            .perform()
            .expect("the original queries")
            .serial_number()
    );
}

#[test]
fn a_request_survives_the_handle_it_was_built_from() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("volume-outlives");

    let request = {
        let file = fixture.open_file();
        request_for(&file)
    };

    request
        .perform()
        .expect("the request's handle outlives the caller's");
}

#[test]
fn a_query_performs_the_same_way_on_another_thread() {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}

    assert_send::<QueryVolumeInformation>();
    assert_sync::<QueryVolumeInformation>();

    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("volume-thread");
    let file = fixture.open_file();
    let expected = request_for(&file)
        .perform()
        .expect("query on this thread")
        .serial_number();
    let request = request_for(&file);

    let observed = std::thread::spawn(move || {
        request
            .perform()
            .expect("query on a worker that never saw the file")
            .serial_number()
    })
    .join()
    .expect("the worker did not panic");

    assert_eq!(observed, expected);
}

#[test]
fn each_accessor_reports_its_own_field() {
    // Four accessors -- `label`, `serial_number`, `maximum_component_length`,
    // and `flags` -- all survived replacement by constants in a mutation run.
    // The tests above query a *real* volume, so they cannot assert exact
    // values: whatever this machine reports is what they get, and a constant is
    // as plausible as the truth.
    //
    // Built directly rather than queried, because what is under test is the
    // wiring between four same-typed fields and their accessors, not the query.
    // Three of them are `u32` and nothing but distinct values can tell a
    // transposition apart -- the failure this catches is an accessor returning
    // its neighbour, which every real query would hide behind plausible numbers.
    let information = VolumeInformation {
        label: Wtf16String::from("LABEL"),
        serial_number: 0x1111_1111,
        maximum_component_length: 0x2222_2222,
        flags: 0x3333_3333,
        filesystem_name: Wtf16String::from("NTFS"),
    };

    assert_eq!(information.label().to_string_lossy(), "LABEL");
    assert_eq!(information.serial_number(), 0x1111_1111);
    assert_eq!(information.maximum_component_length(), 0x2222_2222);
    assert_eq!(information.flags(), 0x3333_3333);
    assert_eq!(information.filesystem_name().to_string_lossy(), "NTFS");

    // The property the three numeric assertions rest on. Stated rather than
    // eyeballed, so a later edit that reused a value would fail here instead of
    // silently weakening the test into one that cannot detect a transposition.
    let numeric = [
        information.serial_number(),
        information.maximum_component_length(),
        information.flags(),
    ];
    for (index, value) in numeric.iter().enumerate() {
        for other in &numeric[index + 1..] {
            assert_ne!(value, other, "two numeric fields share a value");
        }
    }
    assert_ne!(
        information.label(),
        information.filesystem_name(),
        "the two string fields must differ, or one accessor could serve both"
    );
}
