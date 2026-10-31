// Copyright (c) Mike Grier.

//! Tests for `GetSecurityInfo`.
//!
//! The expectations are what this machine's Windows did, measured: Windows decides what a handle may
//! read and with which code it refuses, so those are not restated here from the documentation.

use std::fs::{File, OpenOptions};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr;

use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::Threading::CreateEventW;
use wtf_string::Wtf16String;

use super::{ObjectType, get_security_info, get_security_info_unchecked};
use crate::{
    AclState, SecurityAttributes, SecurityCaptureFailure, SecurityDescriptor, SecurityInformation,
};

/// `READ_CONTROL`, the right to read a descriptor's owner, group and DACL.
const READ_CONTROL: u32 = 0x0002_0000;

/// A file that is removed when this goes out of scope.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        Self(std::env::temp_dir().join(format!("wa-query-{}-{tag}.tmp", std::process::id())))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn descriptor_of(sddl: &str) -> SecurityDescriptor {
    SecurityDescriptor::from_sddl(&Wtf16String::from(sddl))
        .unwrap_or_else(|error| panic!("{sddl:?}: {error}"))
}

/// A new file at `path` created with the DACL `sddl` says, open for reading, writing and reading its
/// security.
fn create_with(path: &Path, sddl: &str) -> File {
    let attributes = SecurityAttributes::new(Some(descriptor_of(sddl)), false);
    let raw = attributes.to_raw();
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: the name is terminated, the attributes point into `attributes`, which outlives the
    // call, and no template is given.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &raw,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    assert_ne!(
        handle, INVALID_HANDLE_VALUE,
        "create the file with the descriptor"
    );
    // SAFETY: a handle `CreateFileW` just returned, owned by nothing else.
    File::from(unsafe { OwnedHandle::from_raw_handle(handle) })
}

fn text_of(descriptor: &SecurityDescriptor, information: SecurityInformation) -> String {
    descriptor
        .to_sddl(information)
        .expect("write the descriptor as text")
        .to_string_lossy()
}

fn read(file: &File, information: SecurityInformation) -> SecurityDescriptor {
    get_security_info(file.as_handle(), ObjectType::FILE, information)
        .expect("read the file's security")
}

#[test]
fn a_dacl_given_at_creation_reads_back_as_the_same_text() {
    // Protected, so nothing is inherited from the directory and added; and file rights by name,
    // because the file system stores a generic right as the file rights it maps to. Each case names
    // the DACL the file is expected to hold, which is the one given except where noted.
    for (index, (sddl, kept)) in [
        ("D:P(A;;FA;;;WD)", "D:P(A;;FA;;;WD)"),
        ("D:P(A;;FR;;;WD)", "D:P(A;;FR;;;WD)"),
        ("D:P(A;;FRFW;;;WD)", "D:P(A;;FRFW;;;WD)"),
        ("D:P(A;;FR;;;WD)(A;;FA;;;BA)", "D:P(A;;FR;;;WD)(A;;FA;;;BA)"),
        ("D:P(A;;FA;;;BA)(A;;FR;;;WD)", "D:P(A;;FA;;;BA)(A;;FR;;;WD)"),
        ("D:P(D;;FW;;;WD)(A;;FR;;;WD)", "D:P(D;;FW;;;WD)(A;;FR;;;WD)"),
        (
            "D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;WD)",
            "D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;WD)",
        ),
        (
            "D:P(A;;FA;;;WD)(A;;FA;;;BA)(A;;FA;;;SY)(A;;FA;;;AU)",
            "D:P(A;;FA;;;WD)(A;;FA;;;BA)(A;;FA;;;SY)(A;;FA;;;AU)",
        ),
        // Measured: the entries are kept and the auto-inherited flag is not.
        ("D:PAI(A;;FR;;;WD)", "D:P(A;;FR;;;WD)"),
        ("D:P(A;;0x120089;;;WD)", "D:P(A;;0x120089;;;WD)"),
    ]
    .into_iter()
    .enumerate()
    {
        let scratch = Scratch::new(&format!("round-{index}"));
        let file = create_with(&scratch.0, sddl);

        let descriptor = read(&file, SecurityInformation::DACL);
        let text = text_of(&descriptor, SecurityInformation::DACL);

        // Windows' rendering is not necessarily the string given, so the expectation is the DACL
        // the file is expected to hold, rendered by the same conversion: derived from what the file
        // was created with, not from what it reported.
        let expected = text_of(&descriptor_of(kept), SecurityInformation::DACL);
        assert_eq!(text, expected, "{sddl:?}");
    }
}

#[test]
fn the_dacl_of_a_file_made_with_one_entry_has_one_entry() {
    let scratch = Scratch::new("one");
    let file = create_with(&scratch.0, "D:P(A;;FA;;;WD)");

    let descriptor = read(&file, SecurityInformation::DACL);

    assert_eq!(descriptor.dacl().expect("the DACL"), AclState::Populated(1));
}

#[test]
fn a_file_made_without_a_descriptor_has_an_owner_a_group_and_a_dacl() {
    let scratch = Scratch::new("default");
    let file = File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .access_mode(GENERIC_READ | GENERIC_WRITE | READ_CONTROL)
        .open(&scratch.0)
        .expect("create the file");

    let everything =
        SecurityInformation::OWNER | SecurityInformation::GROUP | SecurityInformation::DACL;
    let descriptor = read(&file, everything);

    let text = text_of(&descriptor, everything);
    assert!(text.starts_with("O:"), "{text:?}");
    assert!(text.contains("G:"), "{text:?}");
    assert!(text.contains("D:"), "{text:?}");
    assert!(matches!(
        descriptor.dacl().expect("the DACL"),
        AclState::Populated(_)
    ));
}

#[test]
fn only_the_selected_parts_are_in_the_result() {
    let scratch = Scratch::new("selected");
    let file = create_with(&scratch.0, "D:P(A;;FA;;;WD)");

    let owner_only = read(&file, SecurityInformation::OWNER);
    assert_eq!(owner_only.dacl().expect("the DACL"), AclState::Absent);
    assert!(text_of(&owner_only, SecurityInformation::OWNER).starts_with("O:"));
    assert_eq!(text_of(&owner_only, SecurityInformation::DACL), "");

    let dacl_only = read(&file, SecurityInformation::DACL);
    assert_eq!(dacl_only.dacl().expect("the DACL"), AclState::Populated(1));
    assert_eq!(text_of(&dacl_only, SecurityInformation::OWNER), "");
}

#[test]
fn a_directory_handle_is_read_as_a_file() {
    let directory = OpenOptions::new()
        .read(true)
        .access_mode(FILE_READ_ATTRIBUTES | READ_CONTROL)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(std::env::temp_dir())
        .expect("open the temporary directory");

    let descriptor = get_security_info(
        directory.as_handle(),
        ObjectType::FILE,
        SecurityInformation::DACL,
    )
    .expect("read a directory's security");

    assert!(matches!(
        descriptor.dacl().expect("the DACL"),
        AclState::Populated(_)
    ));
}

#[test]
fn the_result_does_not_depend_on_the_handle_staying_open() {
    let scratch = Scratch::new("outlives");
    let file = create_with(&scratch.0, "D:P(A;;FA;;;WD)");

    let descriptor = read(&file, SecurityInformation::DACL);
    drop(file);

    assert_eq!(descriptor.dacl().expect("the DACL"), AclState::Populated(1));
    assert_eq!(
        text_of(&descriptor, SecurityInformation::DACL),
        "D:P(A;;FA;;;WD)"
    );
}

#[test]
fn a_handle_opened_without_the_right_to_read_the_security_is_refused_with_windows_code() {
    let scratch = Scratch::new("no-read-control");
    drop(create_with(&scratch.0, "D:P(A;;FA;;;WD)"));
    let file = OpenOptions::new()
        .read(true)
        .access_mode(FILE_READ_ATTRIBUTES)
        .open(&scratch.0)
        .expect("open for attributes only");

    let error = get_security_info(
        file.as_handle(),
        ObjectType::FILE,
        SecurityInformation::DACL,
    )
    .expect_err("no right to read the DACL");

    assert_eq!(error.failure(), SecurityCaptureFailure::ReadSecurity);
    assert_eq!(
        error.raw_os_error(),
        Some(i32::try_from(ERROR_ACCESS_DENIED).expect("fits"))
    );
    assert!(error.to_string().starts_with("GetSecurityInfo:"), "{error}");
}

#[test]
fn a_sacl_on_a_handle_opened_without_the_right_to_read_it_is_refused() {
    let scratch = Scratch::new("sacl");
    let file = create_with(&scratch.0, "D:P(A;;FA;;;WD)");

    let error = get_security_info(
        file.as_handle(),
        ObjectType::FILE,
        SecurityInformation::SACL,
    )
    .expect_err("a SACL needs a right the handle lacks");

    assert_eq!(error.failure(), SecurityCaptureFailure::ReadSecurity);
    // Measured: a handle opened without `ACCESS_SYSTEM_SECURITY` is refused as access denied, which is
    // answered before the privilege is looked at.
    assert_eq!(
        error.raw_os_error(),
        Some(i32::try_from(ERROR_ACCESS_DENIED).expect("fits"))
    );
}

/// A manual-reset event with a name of its own and the DACL `sddl` says. An unnamed event made with
/// default security has no descriptor to read (measured: the result is only a header), so the event
/// is named and given one.
fn named_event(tag: &str, sddl: &str) -> OwnedHandle {
    let attributes = SecurityAttributes::new(Some(descriptor_of(sddl)), false);
    let raw = attributes.to_raw();
    let name: Vec<u16> = format!("wa-query-{}-{tag}", std::process::id())
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: the name is terminated, and the attributes point into `attributes`, which outlives
    // the call.
    let handle = unsafe { CreateEventW(&raw, 1, 0, name.as_ptr()) };
    assert!(!handle.is_null(), "create an event");
    // SAFETY: a handle `CreateEventW` just returned, owned by nothing else.
    unsafe { OwnedHandle::from_raw_handle(handle) }
}

#[test]
fn a_kernel_object_is_read_and_the_kind_does_not_change_what_is_read() {
    // The rights by value, because the object stores a generic right as the specific ones it maps
    // to (`GA` reads back as `0x1f0003` for an event).
    let event = named_event("kernel", "D:P(A;;0x1f0003;;;WD)");
    let all = SecurityInformation::OWNER | SecurityInformation::GROUP | SecurityInformation::DACL;

    let as_kernel = get_security_info(event.as_handle(), ObjectType::KERNEL_OBJECT, all)
        .expect("an event is a kernel object");
    assert_eq!(as_kernel.dacl().expect("the DACL"), AclState::Populated(1));
    assert_eq!(
        text_of(&as_kernel, SecurityInformation::DACL),
        "D:P(A;;0x1f0003;;;WD)"
    );

    // Measured: Windows reads a handle's security whichever of these kinds names it, so the kind
    // does not make a handle of another kind an error.
    for kind in [ObjectType::FILE, ObjectType::WINDOW_OBJECT] {
        let as_other = get_security_info(event.as_handle(), kind, all).expect("the same read");
        assert_eq!(as_other, as_kernel, "{kind:?}");
    }
}

#[test]
fn a_kind_of_unknown_is_refused_with_windows_code() {
    let event = named_event("unknown", "D:P(A;;0x1f0003;;;WD)");

    let error = get_security_info(
        event.as_handle(),
        ObjectType::UNKNOWN,
        SecurityInformation::DACL,
    )
    .expect_err("an unknown kind");

    assert_eq!(error.failure(), SecurityCaptureFailure::ReadSecurity);
    assert_eq!(
        error.raw_os_error(),
        Some(i32::try_from(ERROR_INVALID_PARAMETER).expect("fits"))
    );
}

#[test]
fn an_object_with_no_descriptor_reads_as_a_descriptor_with_no_parts() {
    // An unnamed event made with default security (measured): Windows answers with a descriptor that
    // carries nothing, which is not an error and is not a NULL DACL.
    // SAFETY: no attributes and no name are given.
    let handle = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
    assert!(!handle.is_null(), "create an event");
    // SAFETY: a handle `CreateEventW` just returned, owned by nothing else.
    let event = unsafe { OwnedHandle::from_raw_handle(handle) };
    let all = SecurityInformation::OWNER | SecurityInformation::GROUP | SecurityInformation::DACL;

    let descriptor =
        get_security_info(event.as_handle(), ObjectType::KERNEL_OBJECT, all).expect("not an error");

    assert_eq!(descriptor.dacl().expect("the DACL"), AclState::Absent);
    assert_eq!(text_of(&descriptor, all), "");
}

#[test]
fn a_selection_of_nothing_reads_as_a_descriptor_with_no_parts() {
    let scratch = Scratch::new("nothing");
    let file = create_with(&scratch.0, "D:P(A;;FA;;;WD)");

    let descriptor = get_security_info(
        file.as_handle(),
        ObjectType::FILE,
        SecurityInformation::NONE,
    )
    .expect("not an error");

    assert_eq!(descriptor.dacl().expect("the DACL"), AclState::Absent);
    assert_eq!(text_of(&descriptor, SecurityInformation::DACL), "");
}

#[test]
fn the_kinds_name_the_win32_values_and_round_trip() {
    assert_eq!(ObjectType::UNKNOWN.as_raw(), 0);
    assert_eq!(ObjectType::FILE.as_raw(), 1);
    assert_eq!(ObjectType::SERVICE.as_raw(), 2);
    assert_eq!(ObjectType::PRINTER.as_raw(), 3);
    assert_eq!(ObjectType::REGISTRY_KEY.as_raw(), 4);
    assert_eq!(ObjectType::LMSHARE.as_raw(), 5);
    assert_eq!(ObjectType::KERNEL_OBJECT.as_raw(), 6);
    assert_eq!(ObjectType::WINDOW_OBJECT.as_raw(), 7);
    assert_eq!(ObjectType::DS_OBJECT.as_raw(), 8);
    assert_eq!(ObjectType::DS_OBJECT_ALL.as_raw(), 9);
    assert_eq!(ObjectType::PROVIDER_DEFINED.as_raw(), 10);
    assert_eq!(ObjectType::WMIGUID.as_raw(), 11);
    assert_eq!(ObjectType::REGISTRY_WOW64_32KEY.as_raw(), 12);
    assert_eq!(ObjectType::REGISTRY_WOW64_64KEY.as_raw(), 13);
    assert_eq!(ObjectType::from_raw(42).as_raw(), 42);
    assert_eq!(ObjectType::from_raw(1), ObjectType::FILE);
}

/// Every named kind, with whether the safe entry passes it with a handle of any kind.
const KINDS: [(ObjectType, bool); 14] = [
    (ObjectType::UNKNOWN, true),
    (ObjectType::FILE, true),
    (ObjectType::SERVICE, false),
    (ObjectType::PRINTER, false),
    (ObjectType::REGISTRY_KEY, false),
    (ObjectType::LMSHARE, false),
    (ObjectType::KERNEL_OBJECT, true),
    (ObjectType::WINDOW_OBJECT, true),
    (ObjectType::DS_OBJECT, false),
    (ObjectType::DS_OBJECT_ALL, false),
    (ObjectType::PROVIDER_DEFINED, false),
    (ObjectType::WMIGUID, false),
    (ObjectType::REGISTRY_WOW64_32KEY, false),
    (ObjectType::REGISTRY_WOW64_64KEY, false),
];

#[test]
fn the_safe_entry_passes_exactly_the_kinds_measured_to_read_any_handle() {
    for (kind, passed) in KINDS {
        assert_eq!(kind.reads_any_handle(), passed, "{kind:?}");
    }
    assert!(!ObjectType::from_raw(42).reads_any_handle());
}

#[test]
fn a_kind_the_safe_entry_does_not_pass_is_refused_before_any_call() {
    // Refused before the call, so even the kind measured to crash with a file handle is safe here.
    let scratch = Scratch::new("unsupported");
    let file = create_with(&scratch.0, "D:P(A;;FA;;;WD)");

    let refused = KINDS
        .iter()
        .filter(|(_, passed)| !passed)
        .map(|(kind, _)| *kind)
        .chain([ObjectType::from_raw(42)]);
    for kind in refused {
        let error = get_security_info(file.as_handle(), kind, SecurityInformation::DACL)
            .expect_err("refused before the call");
        assert_eq!(
            error.failure(),
            SecurityCaptureFailure::UnsupportedObjectType,
            "{kind:?}"
        );
        assert_eq!(
            error.raw_os_error(),
            Some(i32::try_from(ERROR_INVALID_PARAMETER).expect("fits")),
            "{kind:?}"
        );
    }
}

#[test]
fn a_file_handle_reads_the_same_under_every_kind_the_safe_entry_passes() {
    let scratch = Scratch::new("any-kind");
    let file = create_with(&scratch.0, "D:P(A;;FA;;;WD)(A;;FR;;;BA)");
    let as_file = text_of(
        &read(&file, SecurityInformation::DACL),
        SecurityInformation::DACL,
    );

    for kind in [ObjectType::KERNEL_OBJECT, ObjectType::WINDOW_OBJECT] {
        let descriptor = get_security_info(file.as_handle(), kind, SecurityInformation::DACL)
            .unwrap_or_else(|error| panic!("{kind:?}: {error}"));
        assert_eq!(
            text_of(&descriptor, SecurityInformation::DACL),
            as_file,
            "{kind:?}"
        );
    }
}

#[test]
fn the_unchecked_entry_reads_what_the_safe_one_reads_for_a_matching_kind() {
    let scratch = Scratch::new("unchecked");
    let file = create_with(&scratch.0, "D:P(A;;FA;;;WD)");

    // SAFETY: the handle names a file, and the kind is the file kind.
    let unchecked = unsafe {
        get_security_info_unchecked(
            file.as_handle(),
            ObjectType::FILE,
            SecurityInformation::DACL,
        )
    }
    .expect("read the file's security");

    assert_eq!(
        text_of(&unchecked, SecurityInformation::DACL),
        text_of(
            &read(&file, SecurityInformation::DACL),
            SecurityInformation::DACL
        )
    );
}

const CRASH_CHILD_MARKER: &str = "WA_QUERY_LMSHARE_CHILD";
const CRASH_CHILD_TEST: &str =
    "security::query::tests::a_share_kind_with_a_file_handle_is_not_refused_cleanly_by_windows";
/// `STATUS_ACCESS_VIOLATION`, the exit code of a process that faulted.
const ACCESS_VIOLATION: u32 = 0xC000_0005;

// Evidence for the narrowing, not a contract: it records what Windows did with a kind that does not
// match the handle, which is why the safe entry refuses such kinds. The call faults, so it is made in
// a child process of its own, with the error-reporting dialog suppressed.
#[test]
fn a_share_kind_with_a_file_handle_is_not_refused_cleanly_by_windows() {
    use windows_sys::Win32::System::Diagnostics::Debug::{
        SEM_FAILCRITICALERRORS, SEM_NOGPFAULTERRORBOX, SetErrorMode,
    };

    if std::env::var_os(CRASH_CHILD_MARKER).is_some() {
        // SAFETY: changes only this process's error mode.
        unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX) };
        let scratch = Scratch::new("lmshare-child");
        let file = create_with(&scratch.0, "D:P(A;;FA;;;WD)");
        // SAFETY: deliberately not: the kind does not match the handle. This is the measurement,
        // made in a process that exists to be lost to it.
        let outcome = unsafe {
            get_security_info_unchecked(
                file.as_handle(),
                ObjectType::LMSHARE,
                SecurityInformation::DACL,
            )
        };
        println!("returned: {:?}", outcome.map(|_| ()));
        return;
    }

    let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .args(["--exact", CRASH_CHILD_TEST, "--nocapture"])
        .env(CRASH_CHILD_MARKER, "1")
        .output()
        .expect("run this test in a child process");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code().map(i32::cast_unsigned),
        Some(ACCESS_VIOLATION),
        "the child was expected to fault: {}\n{stdout}",
        output.status
    );
}
