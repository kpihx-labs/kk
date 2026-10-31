// Copyright (c) Mike Grier.

//! Tests for the `GetFinalPathNameByHandleW` entry.
//!
//! The grow-the-buffer retry is the part worth proving, and it is proven by
//! forcing it: a deeply nested path exceeds the first attempt's `MAX_PATH`
//! buffer, so the retry runs rather than being assumed correct from a path that
//! always fitted.

use std::fs::File;
use std::os::windows::io::AsHandle;

use super::{FinalPathFlags, QueryFinalPath};
use crate::handle::tests::{Fixture, handle_allocation};
use win_shared_os_owned_handle::SharedHandle;

/// Long enough that the resolved path exceeds the 260-character first attempt,
/// so the retry path is exercised rather than merely present.
const DEEP_NESTING: usize = 12;
const SEGMENT: &str = "a-directory-with-a-deliberately-long-name";

fn request_for(file: &File) -> QueryFinalPath {
    QueryFinalPath::new(SharedHandle::from(
        file.as_handle()
            .try_clone_to_owned()
            .expect("own the handle"),
    ))
}

#[test]
fn a_file_resolves_to_a_verbatim_path() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("finalpath-file");
    let file = fixture.open_file();

    let resolved = request_for(&file)
        .perform()
        .expect("resolve the fixture file")
        .to_string_lossy();

    assert!(resolved.starts_with(r"\\?\"), "unexpected: {resolved}");
    assert!(resolved.ends_with("f.t"), "unexpected: {resolved}");
}

#[test]
fn a_directory_resolves_to_its_own_path() {
    // Deliberately NOT compared against `fixture.directory()` as text. That
    // path comes from `std::env::temp_dir()`, which can hand back the 8.3 short
    // form (`C:\Users\RUNNER~1\...`) while this call, with
    // FILE_NAME_NORMALIZED, returns the long form (`C:\Users\runneradmin\...`).
    // The two are the same directory and different strings.
    //
    // Measured the hard way: the textual version passed on a host whose user
    // name is exactly eight characters -- so no mangling occurred -- and failed
    // on CI, where it is longer. That is an environment difference in the
    // *test*, not a platform finding, and comparing normalised forms is what
    // makes it go away for the right reason.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("finalpath-directory");
    let directory = fixture.open_directory();

    let resolved = request_for(&directory)
        .perform()
        .expect("resolve the fixture directory")
        .to_string_lossy();

    // The unique final component survives any short/long-name difference, so it
    // is the part worth asserting textually.
    let name = fixture
        .directory()
        .file_name()
        .and_then(|name| name.to_str())
        .expect("the fixture directory has a name");
    assert!(
        resolved.ends_with(name),
        "resolved {resolved} should end with {name}"
    );

    // And the whole path is cross-checked against `std::fs::canonicalize`,
    // which normalises the same way. This is the substitution the audit found
    // Globazog making -- it reaches this call through canonicalize -- so the
    // two agreeing is exactly the property that matters for that consumer.
    //
    // Worth stating what it does not prove: canonicalize is itself
    // GetFinalPathNameByHandleW underneath, so this checks that our flags and
    // grow-retry reach the same answer, not that the answer is independently
    // correct. The component check above is the independent half.
    let canonical =
        std::fs::canonicalize(fixture.directory()).expect("canonicalize the fixture directory");
    assert_eq!(
        resolved,
        canonical.to_str().expect("a canonical path is valid UTF-8"),
        "the entry must agree with std::fs::canonicalize"
    );
}
#[test]
fn the_buffer_grows_for_a_path_longer_than_the_first_attempt() {
    // The retry, forced rather than assumed. The first attempt uses a MAX_PATH
    // buffer, so a deeper path must take the second.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("finalpath-deep");

    let mut deep = fixture.directory().to_path_buf();
    for index in 0..DEEP_NESTING {
        deep = deep.join(format!("{SEGMENT}-{index:02}"));
    }
    std::fs::create_dir_all(&deep).expect("create a deeply nested directory");
    let target = deep.join("f.t");
    std::fs::write(&target, b"x").expect("write the deep file");

    let file = File::open(&target).expect("open the deep file");
    let resolved = request_for(&file)
        .perform()
        .expect("resolve a path longer than the first attempt")
        .to_string_lossy();

    assert!(
        resolved.encode_utf16().count() > 260,
        "the fixture must actually exceed the first attempt: {} UTF-16 units",
        resolved.encode_utf16().count()
    );
    assert!(resolved.ends_with("f.t"), "unexpected: {resolved}");
}

#[test]
fn the_guid_volume_form_is_reachable() {
    // Not what the audited consumers use, but an entry that could express one
    // of its call's volume forms would be narrowed.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("finalpath-guid");
    let file = fixture.open_file();

    let resolved = request_for(&file)
        .with_flags(FinalPathFlags::VOLUME_NAME_GUID | FinalPathFlags::NAME_NORMALIZED)
        .perform()
        .expect("resolve with a volume GUID")
        .to_string_lossy();

    assert!(
        resolved.contains("Volume{"),
        "a GUID volume path names a volume rather than a drive letter: {resolved}"
    );
}

#[test]
fn the_default_flags_are_what_the_watcher_relies_on() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("finalpath-defaults");
    let file = fixture.open_file();

    let request = request_for(&file);

    assert_eq!(request.flags(), FinalPathFlags::DEFAULT);
    assert_eq!(
        FinalPathFlags::DEFAULT,
        FinalPathFlags::VOLUME_NAME_DOS | FinalPathFlags::NAME_NORMALIZED
    );
}

#[test]
fn two_handles_to_one_file_resolve_to_the_same_path() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("finalpath-agree");
    let first = fixture.open_file();
    let second = fixture.open_file();

    let one = request_for(&first).perform().expect("resolve the first");
    let two = request_for(&second).perform().expect("resolve the second");

    assert_eq!(one.to_string_lossy(), two.to_string_lossy());
}

#[test]
fn a_clone_shares_the_handle_and_resolves_the_same_way() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("finalpath-copy");
    let file = fixture.open_file();
    let request = request_for(&file);

    let copy = request.clone();

    assert!(copy.handle().same_handle(request.handle()));
    assert_eq!(
        copy.perform().expect("the copy resolves").to_string_lossy(),
        request
            .perform()
            .expect("the original resolves")
            .to_string_lossy()
    );
}

#[test]
fn a_request_survives_the_handle_it_was_built_from() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("finalpath-outlives");

    let request = {
        let file = fixture.open_file();
        request_for(&file)
    };

    let resolved = request
        .perform()
        .expect("the request's handle outlives the caller's")
        .to_string_lossy();
    assert!(resolved.ends_with("f.t"), "unexpected: {resolved}");
}

#[test]
fn a_resolution_performs_the_same_way_on_another_thread() {
    // The offload the audit called out: Globazog does this on its submitting
    // thread today, once per root, with unbounded latency on a network path.
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}

    assert_send::<QueryFinalPath>();
    assert_sync::<QueryFinalPath>();

    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("finalpath-thread");
    let file = fixture.open_file();
    let request = request_for(&file);

    let resolved = std::thread::spawn(move || {
        request
            .perform()
            .expect("resolve on a worker that never saw the file")
            .to_string_lossy()
    })
    .join()
    .expect("the worker did not panic");

    assert!(resolved.ends_with("f.t"), "unexpected: {resolved}");
}
