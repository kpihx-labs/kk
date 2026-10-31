// Copyright (c) Mike Grier.

//! Tests for the `CreateFileW` entry.
//!
//! The three audited flag shapes are exercised by name, because the point of
//! the entry is that all three are expressible without the crate choosing
//! between them.

use std::fs::File;
use std::os::windows::io::{AsHandle, AsRawHandle};

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ, FILE_LIST_DIRECTORY,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use wtf_string::Wtf16String;

use super::OpenFile;
use crate::AnchoredPath;
use crate::handle::tests::{FILE_CONTENTS, Fixture, handle_allocation};
use crate::{SecurityAttributes, SecurityDescriptor};
use win_shared_os_owned_handle::SharedHandle;

/// The share mode all three audited consumers use: a watcher or an enumerator
/// is an observer and must not stop anyone else touching the directory.
const AUDITED_SHARE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

fn request_for(path: &std::path::Path) -> OpenFile {
    let text = path.to_str().expect("the fixture path is valid UTF-8");
    OpenFile::new(Wtf16String::from(text)).expect("build the request")
}

/// The two audited consumers that open a directory without an overlapped
/// handle: `windows-file-enumeration-sys` and Globazog.
fn unassociated_directory_request(path: &std::path::Path) -> OpenFile {
    request_for(path)
        .with_desired_access(FILE_LIST_DIRECTORY)
        .with_share_mode(AUDITED_SHARE)
        .with_creation_disposition(OPEN_EXISTING)
        .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS)
}

/// The watcher's shape: the same open plus `FILE_FLAG_OVERLAPPED`, because its
/// handle is destined for a completion port.
fn overlapped_directory_request(path: &std::path::Path) -> OpenFile {
    unassociated_directory_request(path)
        .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED)
}

#[test]
fn the_unassociated_directory_shape_opens() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("open-unassociated");

    let opened = unassociated_directory_request(fixture.directory())
        .perform()
        .expect("open the fixture directory");

    assert!(
        File::from(opened)
            .metadata()
            .expect("read the directory metadata")
            .is_dir()
    );
}

#[test]
fn the_overlapped_directory_shape_opens() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("open-overlapped");

    let opened = overlapped_directory_request(fixture.directory())
        .perform()
        .expect("open the fixture directory for overlapped use");

    assert!(!opened.as_raw_handle().is_null());
}

#[test]
fn the_ordinary_file_shape_opens() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("open-file");

    let opened = request_for(&fixture.file())
        .with_desired_access(FILE_GENERIC_READ)
        .with_share_mode(FILE_SHARE_READ)
        .with_creation_disposition(OPEN_EXISTING)
        .perform()
        .expect("open the fixture file");

    assert_eq!(
        File::from(opened).metadata().expect("read metadata").len(),
        FILE_CONTENTS.len() as u64
    );
}

#[test]
fn the_overlapped_flag_is_carried_rather_than_decided() {
    // The split the audit found: two consumers omit the flag, one supplies it,
    // and the difference is a request field. A crate that added or removed it
    // would be making the delivery-model choice it refuses to make.
    let fixture = Fixture::new("open-flag-split");

    let plain = unassociated_directory_request(fixture.directory());
    let overlapped = overlapped_directory_request(fixture.directory());

    assert_eq!(plain.flags_and_attributes(), FILE_FLAG_BACKUP_SEMANTICS);
    assert_eq!(
        overlapped.flags_and_attributes(),
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED
    );
    assert_eq!(
        plain.flags_and_attributes() | FILE_FLAG_OVERLAPPED,
        overlapped.flags_and_attributes(),
        "the two shapes differ by exactly the one flag and nothing else"
    );
}

#[test]
fn a_new_request_defaults_nothing_on_the_callers_behalf() {
    // Every field starts at "the caller said nothing". A plausible-looking
    // default is exactly what a caller cannot see they were given.
    let fixture = Fixture::new("open-defaults");

    let request = request_for(&fixture.file());

    assert_eq!(request.desired_access(), 0);
    assert_eq!(request.share_mode(), 0);
    assert_eq!(request.creation_disposition(), 0);
    assert_eq!(request.flags_and_attributes(), 0);
    assert!(request.security().is_none());
    assert!(request.template().is_none());
}

#[test]
fn backup_semantics_is_not_implied_for_a_directory() {
    // Windows refuses to open a directory without the flag, whatever the access
    // mask says. The entry does not add it, so this fails -- which is the
    // caller's mistake surfacing rather than the crate silently correcting it.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("open-no-backup");

    let outcome = request_for(fixture.directory())
        .with_desired_access(FILE_LIST_DIRECTORY)
        .with_share_mode(AUDITED_SHARE)
        .with_creation_disposition(OPEN_EXISTING)
        .perform();

    assert!(
        outcome.is_err(),
        "omitting FILE_FLAG_BACKUP_SEMANTICS must reach Windows unaltered"
    );
}

#[test]
fn a_missing_path_reports_the_raw_code_unaltered() {
    let fixture = Fixture::new("open-missing");
    let absent = fixture.directory().join("no-such-file.t");

    let error = request_for(&absent)
        .with_desired_access(FILE_GENERIC_READ)
        .with_share_mode(FILE_SHARE_READ)
        .with_creation_disposition(OPEN_EXISTING)
        .perform()
        .expect_err("a missing file cannot be opened");

    assert_eq!(
        error.code(),
        ERROR_FILE_NOT_FOUND,
        "the code is passed through, not reclassified"
    );
}

#[test]
fn a_missing_directory_is_distinguishable_from_a_missing_file() {
    // Windows distinguishes these, so the entry must too -- by doing nothing.
    let fixture = Fixture::new("open-missing-dir");
    let absent = fixture.directory().join("no-such-dir").join("file.t");

    let error = request_for(&absent)
        .with_desired_access(FILE_GENERIC_READ)
        .with_share_mode(FILE_SHARE_READ)
        .with_creation_disposition(OPEN_EXISTING)
        .perform()
        .expect_err("a missing directory cannot be traversed");

    assert_eq!(error.code(), ERROR_PATH_NOT_FOUND);
}

/// A descriptor with a DACL of one entry, which is what these requests carry.
fn populated_descriptor() -> SecurityDescriptor {
    SecurityDescriptor::from_sddl(&Wtf16String::from("D:(A;;GA;;;WD)"))
        .expect("a descriptor with one entry")
}

#[test]
fn security_attributes_are_expressible_though_no_consumer_uses_them() {
    // Kept because an entry that cannot express two of its own call's
    // parameters is a narrowed CreateFileW, not because a consumer asked.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("open-security");
    let descriptor = populated_descriptor();
    let request = request_for(&fixture.file())
        .with_desired_access(FILE_GENERIC_READ)
        .with_share_mode(FILE_SHARE_READ)
        .with_creation_disposition(OPEN_EXISTING)
        .with_security(Some(SecurityAttributes::new(Some(descriptor), false)));

    assert!(request.security().is_some());
    // OPEN_EXISTING ignores the descriptor, but the inheritance choice still
    // applies, so the call must succeed with the attributes present.
    request.perform().expect("open with security attributes");
}

#[test]
fn a_template_handle_is_expressible_though_no_consumer_uses_one() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("open-template");
    let template_source = fixture.open_file();
    let template = SharedHandle::from(
        template_source
            .as_handle()
            .try_clone_to_owned()
            .expect("own the template handle"),
    );

    let request = request_for(&fixture.file())
        .with_desired_access(FILE_GENERIC_READ)
        .with_share_mode(FILE_SHARE_READ)
        .with_creation_disposition(OPEN_EXISTING)
        .with_template(Some(template));

    assert!(request.template().is_some());
    // OPEN_EXISTING ignores hTemplateFile, so this asserts the parameter is
    // carried and accepted, not that it changes the result.
    request.perform().expect("open with a template handle");
}

#[test]
fn a_request_survives_every_input_it_was_built_from() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");

    let (request, expected_len) = {
        let fixture = Fixture::new("open-outlives");
        let template_source = fixture.open_file();
        let descriptor = populated_descriptor();

        let request = request_for(&fixture.file())
            .with_desired_access(FILE_GENERIC_READ)
            .with_share_mode(FILE_SHARE_READ)
            .with_creation_disposition(OPEN_EXISTING)
            .with_security(Some(SecurityAttributes::new(Some(descriptor), false)))
            .with_template(Some(SharedHandle::from(
                template_source
                    .as_handle()
                    .try_clone_to_owned()
                    .expect("own the template handle"),
            )));

        // The fixture is deliberately leaked so the file outlives this scope;
        // the point is that the *request's* inputs are gone, not the file.
        std::mem::forget(fixture);
        (request, FILE_CONTENTS.len() as u64)
    };

    let opened = request
        .perform()
        .expect("open after every input is dropped");
    assert_eq!(
        File::from(opened).metadata().expect("read metadata").len(),
        expected_len
    );
}

#[test]
fn a_request_performs_the_same_way_on_another_thread() {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}

    assert_send::<OpenFile>();
    assert_sync::<OpenFile>();

    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("open-thread");
    let request = unassociated_directory_request(fixture.directory());

    let opened = std::thread::spawn(move || request.perform())
        .join()
        .expect("the worker did not panic")
        .expect("open on a worker thread");

    assert!(
        File::from(opened)
            .metadata()
            .expect("read the directory metadata")
            .is_dir()
    );
}

#[test]
fn a_request_can_be_performed_more_than_once() {
    // A request is a parameter set, not a one-shot ticket: performing it twice
    // yields two independent handles.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("open-repeat");
    let request = unassociated_directory_request(fixture.directory());

    let first = request.perform().expect("first open");
    let second = request.perform().expect("second open");

    assert_ne!(
        first.as_raw_handle(),
        second.as_raw_handle(),
        "two opens are two handles"
    );
}

#[test]
fn a_copy_without_security_attributes_is_made() {
    // Not `Clone`: copying security attributes allocates, and the request
    // inherits that rather than hiding it behind a signature that would have to
    // panic. A request without them has nothing that can fail.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("open-copy");
    let request = unassociated_directory_request(fixture.directory());

    let copy = request.try_clone().expect("a request with no template");
    drop(request);

    assert!(
        File::from(copy.perform().expect("the copy opens"))
            .metadata()
            .expect("read the directory metadata")
            .is_dir()
    );
}

#[test]
fn a_copy_owns_its_own_copy_of_the_security_attributes() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("open-copy-security");
    let descriptor = populated_descriptor();
    let request = request_for(&fixture.file())
        .with_security(Some(SecurityAttributes::new(Some(descriptor), true)));

    let copy = request.try_clone().expect("copy the request");

    assert_eq!(copy.security(), request.security());
    let blob = |request: &OpenFile| {
        request
            .security()
            .and_then(SecurityAttributes::descriptor)
            .expect("the request has a descriptor")
            .as_ptr()
    };
    assert_ne!(
        blob(&copy),
        blob(&request),
        "each request owns its own blob"
    );
}

#[test]
fn a_copy_shares_the_template() {
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("open-copy-template");
    let template_source = fixture.open_file();
    let request = request_for(&fixture.file())
        .with_desired_access(FILE_GENERIC_READ)
        .with_share_mode(FILE_SHARE_READ)
        .with_creation_disposition(OPEN_EXISTING)
        .with_template(Some(SharedHandle::from(
            template_source
                .as_handle()
                .try_clone_to_owned()
                .expect("own the template handle"),
        )));

    let copy = request.try_clone().expect("copy the request");

    assert!(
        copy.template()
            .expect("the copy kept a template")
            .same_handle(request.template().expect("the original kept a template")),
        "the copy shares the original's template"
    );
    drop(request);
    copy.perform()
        .expect("the copy's template outlived the original");
}

#[test]
fn every_configured_parameter_reads_back_through_its_own_accessor() {
    // A mutation run replaced `desired_access`, `share_mode`, and
    // `creation_disposition` with constants and nothing failed. The tests above
    // build requests with `with_*` and then *open* them, so they exercise the
    // fields through Win32 -- which is exactly what cannot distinguish an
    // accessor reporting the truth from one reporting a constant, because the
    // open path reads the struct's fields directly rather than through them.
    //
    // Every value here is deliberately non-zero and pairwise distinct.
    // `OpenFile::new` starts every parameter at zero, so a test that configured
    // a zero -- or reused one value twice -- would be satisfied by
    // `-> Default::default()` and by an accessor reading a neighbour's field.
    let fixture = Fixture::new("open-accessors");
    let request = request_for(fixture.directory())
        .with_desired_access(FILE_GENERIC_READ)
        .with_share_mode(FILE_SHARE_READ)
        .with_creation_disposition(OPEN_EXISTING)
        .with_flags_and_attributes(FILE_FLAG_BACKUP_SEMANTICS);

    assert_eq!(request.desired_access(), FILE_GENERIC_READ);
    assert_eq!(request.share_mode(), FILE_SHARE_READ);
    assert_eq!(request.creation_disposition(), OPEN_EXISTING);
    assert_eq!(request.flags_and_attributes(), FILE_FLAG_BACKUP_SEMANTICS);

    // The property the four assertions above rest on, stated rather than left
    // to the reader's eye: these are Win32 constants and their values are not
    // obvious, so a collision between two of them would silently weaken the
    // test into one that cannot tell those accessors apart.
    let configured = [
        FILE_GENERIC_READ,
        FILE_SHARE_READ,
        OPEN_EXISTING,
        FILE_FLAG_BACKUP_SEMANTICS,
    ];
    for (index, value) in configured.iter().enumerate() {
        assert_ne!(
            *value, 0,
            "a zero is indistinguishable from the unset default"
        );
        for other in &configured[index + 1..] {
            assert_ne!(
                value, other,
                "two parameters share a value, so this test cannot tell their \
                 accessors apart"
            );
        }
    }
}

#[test]
fn an_unset_parameter_reads_back_as_nothing_rather_than_as_a_plausible_open() {
    // The contract `OpenFile::new` states: every parameter starts at "the
    // caller said nothing" rather than at a plausible-looking open, because a
    // plausible default is exactly what a caller cannot see they got. An
    // accessor that invented one would hide that from them.
    let fixture = Fixture::new("open-unset");
    let request = request_for(fixture.directory());

    assert_eq!(request.desired_access(), 0);
    assert_eq!(request.share_mode(), 0);
    assert_eq!(request.creation_disposition(), 0);
    assert_eq!(request.flags_and_attributes(), 0);
    assert!(request.security().is_none());
}

// --- The path: as given, and as handed to Win32 ------------------------------------------------

#[test]
fn an_absolute_path_is_handed_to_win32_as_given() {
    let fixture = Fixture::new("open-absolute");
    let request = request_for(fixture.directory());

    assert_eq!(request.path(), request.win32_path());
    assert_eq!(
        request.win32_path().to_string_lossy(),
        fixture.directory().to_str().expect("a UTF-8 fixture path")
    );
}

#[test]
fn a_relative_path_is_fixed_to_the_current_directory_when_the_request_is_built() {
    // Whether anything of this name exists is not at issue here, so no file is assumed to; the
    // child-process test below performs one.
    let directory = std::env::current_dir().expect("a current directory");
    let directory = directory.to_str().expect("a UTF-8 current directory");
    let request = OpenFile::new(Wtf16String::from("Cargo.toml")).expect("build the request");

    assert_eq!(request.path().to_string_lossy(), "Cargo.toml");
    assert_eq!(
        request.win32_path().to_string_lossy(),
        format!(r"{}\Cargo.toml", directory.trim_end_matches('\\'))
    );
}

#[test]
fn paths_that_are_not_plain_relative_ones_are_left_as_given() {
    for path in [
        r"\\?\C:\a",
        r"\\.\PhysicalDrive0",
        r"\\server\share\a",
        r"\a\b",
        "C:a",
        "name:stream",
        "NUL",
        "con",
        "",
    ] {
        let request = OpenFile::new(Wtf16String::from(path)).expect("build the request");
        assert_eq!(request.win32_path().to_string_lossy(), path);
    }
}

#[test]
fn a_path_with_an_interior_nul_is_refused_when_the_request_is_built() {
    // Win32 reads a path up to its first NUL, so the open would be of a shorter path than the one
    // named, and a success would be for the wrong file.
    let error = OpenFile::new(Wtf16String::from_units(&[0x0061, 0x0000, 0x0062]))
        .expect_err("a NUL truncates the path");

    assert!(matches!(error, crate::PathCaptureError::InteriorNul));
}

#[test]
fn a_request_that_is_copied_keeps_the_path_as_given_and_as_handed_over() {
    let directory = std::env::current_dir().expect("a current directory");
    let request = OpenFile::new(Wtf16String::from("Cargo.toml")).expect("build the request");

    let copy = request.try_clone().expect("copy the request");

    assert_eq!(copy.path(), request.path());
    assert_eq!(copy.win32_path(), request.win32_path());
    assert_ne!(
        copy.win32_path(),
        copy.path(),
        "{directory:?} was grafted on"
    );
}

const CHILD_MARKER: &str = "WNRS_OPEN_CHILD";
const CHILD_TEST: &str =
    "open::tests::a_relative_path_names_the_directory_it_was_built_in_after_the_directory_changes";

// The process current directory is shared by every test in the process, so a test that changes it
// runs in a child process of its own, as `anchored::tests` does.
#[test]
fn a_relative_path_names_the_directory_it_was_built_in_after_the_directory_changes() {
    const NAME: &str = "wnrs-open-relative.t";

    if std::env::var_os(CHILD_MARKER).is_some() {
        let request = OpenFile::new(Wtf16String::from(NAME))
            .expect("build the request")
            .with_desired_access(FILE_GENERIC_READ)
            .with_share_mode(AUDITED_SHARE)
            .with_creation_disposition(OPEN_EXISTING);
        let elsewhere = std::env::var_os("SystemRoot").expect("Windows names its directory");
        std::env::set_current_dir(elsewhere).expect("change the current directory");

        // Performed after the move, it still opens the file the directory it was built in held.
        let opened = request
            .perform()
            .expect("open by the path fixed when it was built");
        assert!(File::from(opened).metadata().expect("metadata").is_file());

        // The same name given now is a different path, which is what the graft prevents.
        let later = OpenFile::new(Wtf16String::from(NAME))
            .expect("build the request")
            .with_desired_access(FILE_GENERIC_READ)
            .with_share_mode(AUDITED_SHARE)
            .with_creation_disposition(OPEN_EXISTING);
        let refused = later
            .perform()
            .expect_err("the file is not in this directory");
        assert_eq!(refused.code(), ERROR_FILE_NOT_FOUND);
        return;
    }

    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("open-relative-child");
    std::fs::write(fixture.directory().join(NAME), b"contents").expect("create the file");

    let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .arg("--exact")
        .arg(CHILD_TEST)
        .arg("--nocapture")
        .env(CHILD_MARKER, "1")
        .current_dir(fixture.directory())
        .output()
        .expect("run this test in a child process");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "the child did not pass: {}\n{stdout}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

const VERBATIM_CHILD_MARKER: &str = "WNRS_OPEN_VERBATIM_CHILD";
const VERBATIM_CHILD_TEST: &str =
    "open::tests::a_relative_path_is_not_grafted_onto_a_verbatim_current_directory";

// A verbatim current directory is one Win32 does not normalise, so a child joined onto it is read
// literally: `.\kernel32.dll` opens relative to the directory and `<directory>\.\kernel32.dll` does
// not. The process current directory is shared, so this runs in a child process.
#[test]
fn a_relative_path_is_not_grafted_onto_a_verbatim_current_directory() {
    if std::env::var_os(VERBATIM_CHILD_MARKER).is_some() {
        let system = std::env::var("SystemRoot").expect("Windows names its directory");
        let verbatim = format!(r"\\?\{system}\System32");
        std::env::set_current_dir(&verbatim).expect("change to the verbatim directory");

        let request = OpenFile::new(Wtf16String::from(r".\kernel32.dll"))
            .expect("build the request")
            .with_desired_access(FILE_GENERIC_READ)
            .with_share_mode(AUDITED_SHARE)
            .with_creation_disposition(OPEN_EXISTING);
        assert_eq!(
            request.win32_path(),
            request.path(),
            "the path is left for Win32 to resolve, not joined onto a verbatim directory"
        );
        let opened = request
            .perform()
            .expect("open relative to the verbatim current directory");
        assert!(File::from(opened).metadata().expect("metadata").is_file());
        return;
    }

    // Spawning allocates the pipe handles, which must not reuse a value a test holding the write
    // lock is reasoning about.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .arg("--exact")
        .arg(VERBATIM_CHILD_TEST)
        .arg("--nocapture")
        .env(VERBATIM_CHILD_MARKER, "1")
        .output()
        .expect("run this test in a child process");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "the child did not pass: {}\n{stdout}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_request_from_an_anchored_path_hands_win32_the_anchored_path() {
    // Anchored onto a root that is not the current directory, so a constructor that anchored the
    // path again would hand Win32 a different one.
    let anchored = AnchoredPath::anchored_at(
        Wtf16String::from("a-child"),
        &Wtf16String::from(r"C:\anchored-elsewhere"),
    );

    let request = OpenFile::from_anchored(anchored.clone());

    assert_eq!(request.path(), anchored.given());
    assert_eq!(request.win32_path(), anchored.win32());
    assert_eq!(
        request.win32_path().to_string_lossy(),
        r"C:\anchored-elsewhere\a-child"
    );
}

#[test]
fn a_request_from_an_anchored_path_starts_as_one_built_from_text() {
    let text = Wtf16String::from(r"C:\Windows");
    let built = OpenFile::new(text.clone()).expect("build the request");
    let anchored = OpenFile::from_anchored(AnchoredPath::new(text).expect("anchor the path"));

    assert_eq!(anchored.path(), built.path());
    assert_eq!(anchored.win32_path(), built.win32_path());
    assert_eq!(anchored.desired_access(), 0);
    assert_eq!(anchored.share_mode(), 0);
    assert!(anchored.security().is_none());
    assert_eq!(anchored.creation_disposition(), 0);
    assert_eq!(anchored.flags_and_attributes(), 0);
    assert!(anchored.template().is_none());
}

const ANCHORED_CHILD_MARKER: &str = "WNRS_OPEN_ANCHORED_CHILD";
const ANCHORED_CHILD_TEST: &str =
    "open::tests::a_path_anchored_before_the_directory_changes_opens_what_it_named_then";

// The request is built after the directory changes, from a path anchored before it, which is the
// enumeration crate's case: it anchors when its own request is built and opens later. The process
// current directory is shared, so this runs in a child process.
#[test]
fn a_path_anchored_before_the_directory_changes_opens_what_it_named_then() {
    const NAME: &str = "wnrs-open-anchored.t";

    if std::env::var_os(ANCHORED_CHILD_MARKER).is_some() {
        let anchored = AnchoredPath::new(Wtf16String::from(NAME)).expect("anchor the path");
        let elsewhere = std::env::var_os("SystemRoot").expect("Windows names its directory");
        std::env::set_current_dir(elsewhere).expect("change the current directory");

        let opened = OpenFile::from_anchored(anchored)
            .with_desired_access(FILE_GENERIC_READ)
            .with_share_mode(AUDITED_SHARE)
            .with_creation_disposition(OPEN_EXISTING)
            .perform()
            .expect("open by the path anchored before the move");
        assert!(File::from(opened).metadata().expect("metadata").is_file());

        // Anchored now, the same name is a path in this directory, where there is no such file.
        let refused = OpenFile::new(Wtf16String::from(NAME))
            .expect("build the request")
            .with_desired_access(FILE_GENERIC_READ)
            .with_share_mode(AUDITED_SHARE)
            .with_creation_disposition(OPEN_EXISTING)
            .perform()
            .expect_err("the file is not in this directory");
        assert_eq!(refused.code(), ERROR_FILE_NOT_FOUND);
        return;
    }

    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let fixture = Fixture::new("open-anchored-child");
    std::fs::write(fixture.directory().join(NAME), b"contents").expect("create the file");

    let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .arg("--exact")
        .arg(ANCHORED_CHILD_TEST)
        .arg("--nocapture")
        .env(ANCHORED_CHILD_MARKER, "1")
        .current_dir(fixture.directory())
        .output()
        .expect("run this test in a child process");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "the child did not pass: {}\n{stdout}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}
