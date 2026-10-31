// Copyright (c) Mike Grier.

//! What a file created through [`OpenFile`] with a security descriptor is given: the descriptor's own
//! parsing and rendering are `win-authorization-sys`'s and are tested there; what is checked here is
//! that the request hands the descriptor to `CreateFileW` as it was given.

use std::os::windows::io::{AsHandle, OwnedHandle};

use win_authorization_sys::{
    ObjectType, SecurityAttributes, SecurityDescriptor, SecurityInformation, get_security_info,
};
use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, FILE_GENERIC_READ, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use wtf_string::Wtf16String;

use crate::OpenFile;
use crate::handle::tests::{Fixture, handle_allocation};

fn text(sddl: &str) -> Wtf16String {
    Wtf16String::from(sddl)
}

fn parse(sddl: &str) -> SecurityDescriptor {
    SecurityDescriptor::from_sddl(&text(sddl)).unwrap_or_else(|error| panic!("{sddl:?}: {error}"))
}

const EVERYONE_ALL: &str = "D:P(A;;GA;;;WD)";

/// The DACL of the file `handle` is open on, as the text Windows renders it as, read through
/// `win-authorization-sys`'s safe query.
fn dacl_of(handle: &OwnedHandle) -> String {
    get_security_info(
        handle.as_handle(),
        ObjectType::FILE,
        SecurityInformation::DACL,
    )
    .expect("read the file's security")
    .to_sddl(SecurityInformation::DACL)
    .expect("write the descriptor as text")
    .to_string_lossy()
}

fn create_with(fixture: &Fixture, name: &str, sddl: &str) -> OwnedHandle {
    let path = fixture.directory().join(name);
    let path = path.to_str().expect("the fixture path is valid UTF-8");
    OpenFile::new(text(path))
        .expect("build the request")
        .with_desired_access(FILE_GENERIC_READ | GENERIC_READ | GENERIC_WRITE)
        .with_share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .with_creation_disposition(CREATE_NEW)
        .with_security(Some(SecurityAttributes::new(Some(parse(sddl)), false)))
        .perform()
        .expect("create the file")
}

#[test]
fn a_file_created_with_a_descriptor_from_text_is_given_that_dacl() {
    let _allocating = handle_allocation().read().expect("not poisoned");
    let fixture = Fixture::new("sddl-create");

    // A protected DACL, so nothing is inherited from the directory and added to it, and file
    // rights by name, because the file system stores a generic right as the file rights it maps to
    // (`GA` reads back as `FA`).
    let everyone_all = "D:P(A;;FA;;;WD)";
    let everyone = create_with(&fixture, "everyone.bin", everyone_all);
    let read_only = create_with(&fixture, "read-only.bin", "D:P(A;;FR;;;WD)(A;;FA;;;BA)");

    assert_eq!(dacl_of(&everyone), everyone_all);
    assert_eq!(
        dacl_of(&create_with(&fixture, "generic.bin", EVERYONE_ALL)),
        everyone_all
    );
    assert_eq!(dacl_of(&read_only), "D:P(A;;FR;;;WD)(A;;FA;;;BA)");
}
