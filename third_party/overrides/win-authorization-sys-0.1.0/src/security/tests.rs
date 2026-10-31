// Copyright (c) Mike Grier.
// Split from crates/windows-namespace-request-sys/src/security/tests.rs at 11020848.

//! Tests for security-attribute capture.
//!
//! The descriptors here are built by hand rather than parsed from SDDL,
//! because SDDL only ever produces the self-relative form and the **absolute**
//! form -- a struct of raw pointers into the builder's own storage -- is the
//! case capture exists for.

use std::ffi::c_void;
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_NOT_ENOUGH_MEMORY, FALSE, GENERIC_ALL, TRUE};
use windows_sys::Win32::Security::{
    ACL, ACL_REVISION, AddAccessAllowedAce, CreateWellKnownSid, InitializeAcl,
    InitializeSecurityDescriptor, MakeSelfRelativeSD, SECURITY_ATTRIBUTES, SECURITY_DESCRIPTOR,
    SECURITY_MAX_SID_SIZE, SetSecurityDescriptorDacl, SetSecurityDescriptorGroup,
    SetSecurityDescriptorOwner, WinWorldSid,
};

use super::{
    AclState, SELF_RELATIVE_ALIGNMENT, SecurityAttributes, SecurityCaptureFailure,
    SecurityDescriptor, allocate,
};
use io_buffers::AlignedBuf;

/// A zeroed buffer at `align`, for the descriptors and lists these tests build by hand.
fn aligned_zeroed(len: usize, align: usize) -> AlignedBuf {
    AlignedBuf::zeroed(len, align).expect("an aligned allocation")
}

/// The revision every descriptor built here declares.
///
/// windows-sys does not export `SECURITY_DESCRIPTOR_REVISION`, so it is named
/// here rather than written as a bare literal at each use.
const SECURITY_DESCRIPTOR_REVISION: u32 = 1;

/// An ACL, like a self-relative descriptor, must be DWORD-aligned. The `ACL`
/// struct's own alignment is only 2, so it is stated rather than inferred.
const ACL_ALIGNMENT: usize = align_of::<u32>();

/// An empty ACL is a header and nothing else, rounded up to a DWORD.
const EMPTY_ACL_BYTES: u32 = 8;

/// Room for the header plus one access-allowed ACE naming the World SID.
const ONE_ACE_ACL_BYTES: u32 = 256;

/// An absolute security descriptor, kept alive together with everything it
/// points at.
///
/// This is exactly the shape that makes capture necessary: `descriptor` holds
/// raw pointers into `sid` and `acl`, so copying the struct alone would copy a
/// set of references to storage the caller owns.
pub(crate) struct Absolute {
    descriptor: Box<SECURITY_DESCRIPTOR>,
    _sid: AlignedBuf,
    _acl: Option<AlignedBuf>,
}

impl Absolute {
    /// A descriptor with an owner and group but no DACL at all.
    fn without_dacl() -> Self {
        let mut sid = world_sid();
        let mut descriptor = new_descriptor();

        set_owner_and_group(&mut descriptor, &mut sid);

        Self {
            descriptor,
            _sid: sid,
            _acl: None,
        }
    }

    /// A descriptor whose DACL is present and NULL: everyone gets everything.
    fn with_null_dacl() -> Self {
        let mut absolute = Self::without_dacl();

        // SAFETY: the descriptor is initialised and writable; a null ACL with
        // present set is the documented NULL-DACL form.
        let set = unsafe {
            SetSecurityDescriptorDacl(
                descriptor_ptr(&mut absolute.descriptor),
                TRUE,
                ptr::null_mut(),
                FALSE,
            )
        };
        assert_ne!(set, FALSE, "set a NULL DACL");

        absolute
    }

    /// A descriptor whose DACL is present and has no entries: nobody gets
    /// anything.
    fn with_empty_dacl() -> Self {
        let mut absolute = Self::without_dacl();
        let mut acl = new_acl(EMPTY_ACL_BYTES);

        // SAFETY: acl is an initialised ACL of the stated size, and the
        // descriptor is initialised and writable.
        let set = unsafe {
            SetSecurityDescriptorDacl(
                descriptor_ptr(&mut absolute.descriptor),
                TRUE,
                acl.as_mut_ptr().cast::<ACL>(),
                FALSE,
            )
        };
        assert_ne!(set, FALSE, "set an empty DACL");

        absolute._acl = Some(acl);
        absolute
    }

    /// A descriptor whose DACL grants the World SID everything.
    pub(crate) fn with_populated_dacl() -> Self {
        let mut absolute = Self::without_dacl();
        let mut acl = new_acl(ONE_ACE_ACL_BYTES);
        let mut sid = world_sid();

        // SAFETY: acl is an initialised ACL with room to spare, and sid holds a
        // well-known SID that Windows just wrote.
        let added = unsafe {
            AddAccessAllowedAce(
                acl.as_mut_ptr().cast::<ACL>(),
                ACL_REVISION,
                GENERIC_ALL,
                sid.as_mut_ptr().cast::<c_void>(),
            )
        };
        assert_ne!(added, FALSE, "add an access-allowed ACE");

        // SAFETY: as in with_empty_dacl.
        let set = unsafe {
            SetSecurityDescriptorDacl(
                descriptor_ptr(&mut absolute.descriptor),
                TRUE,
                acl.as_mut_ptr().cast::<ACL>(),
                FALSE,
            )
        };
        assert_ne!(set, FALSE, "set a populated DACL");

        absolute._acl = Some(acl);
        absolute
    }

    pub(crate) fn as_ptr(&self) -> *const c_void {
        ptr::from_ref(self.descriptor.as_ref()).cast::<c_void>()
    }

    /// The same descriptor, converted to self-relative form by Windows.
    ///
    /// This is what a caller who already holds a self-relative descriptor looks
    /// like to capture.
    fn to_self_relative(&self) -> AlignedBuf {
        let mut length: u32 = 0;
        // SAFETY: the null destination is the documented sizing call.
        let sized = unsafe {
            MakeSelfRelativeSD(self.as_ptr().cast_mut(), ptr::null_mut(), &raw mut length)
        };
        assert_eq!(sized, FALSE, "the sizing call is expected to fail");

        let mut blob = aligned_zeroed(length as usize, SELF_RELATIVE_ALIGNMENT);
        // SAFETY: blob is writable for exactly the reported length.
        let converted = unsafe {
            MakeSelfRelativeSD(
                self.as_ptr().cast_mut(),
                blob.as_mut_ptr().cast::<c_void>(),
                &raw mut length,
            )
        };
        assert_ne!(converted, FALSE, "convert to self-relative form");

        blob
    }
}

fn descriptor_ptr(descriptor: &mut SECURITY_DESCRIPTOR) -> *mut c_void {
    ptr::from_mut(descriptor).cast::<c_void>()
}

fn new_descriptor() -> Box<SECURITY_DESCRIPTOR> {
    let mut descriptor = Box::new(unsafe { std::mem::zeroed::<SECURITY_DESCRIPTOR>() });

    // SAFETY: the box is writable and large enough for a descriptor, which is
    // what InitializeSecurityDescriptor requires.
    let initialised = unsafe {
        InitializeSecurityDescriptor(
            descriptor_ptr(&mut descriptor),
            SECURITY_DESCRIPTOR_REVISION,
        )
    };
    assert_ne!(initialised, FALSE, "initialise an absolute descriptor");

    descriptor
}

fn set_owner_and_group(descriptor: &mut SECURITY_DESCRIPTOR, sid: &mut AlignedBuf) {
    // SAFETY: the descriptor is initialised, and sid holds a SID Windows wrote.
    let owner = unsafe {
        SetSecurityDescriptorOwner(
            descriptor_ptr(descriptor),
            sid.as_mut_ptr().cast::<c_void>(),
            FALSE,
        )
    };
    assert_ne!(owner, FALSE, "set the owner");

    // SAFETY: as above.
    let group = unsafe {
        SetSecurityDescriptorGroup(
            descriptor_ptr(descriptor),
            sid.as_mut_ptr().cast::<c_void>(),
            FALSE,
        )
    };
    assert_ne!(group, FALSE, "set the group");
}

fn new_acl(bytes: u32) -> AlignedBuf {
    let mut acl = aligned_zeroed(bytes as usize, ACL_ALIGNMENT);

    // SAFETY: acl is writable for exactly `bytes` bytes, which is what
    // InitializeAcl is told to use.
    let initialised = unsafe { InitializeAcl(acl.as_mut_ptr().cast::<ACL>(), bytes, ACL_REVISION) };
    assert_ne!(initialised, FALSE, "initialise an ACL");

    acl
}

fn world_sid() -> AlignedBuf {
    let mut sid = aligned_zeroed(SECURITY_MAX_SID_SIZE as usize, 8);
    let mut length = SECURITY_MAX_SID_SIZE;

    // SAFETY: sid is writable for length bytes, which is the documented maximum
    // any SID needs.
    let created = unsafe {
        CreateWellKnownSid(
            WinWorldSid,
            ptr::null_mut(),
            sid.as_mut_ptr().cast::<c_void>(),
            &raw mut length,
        )
    };
    assert_ne!(created, FALSE, "create the World SID");

    sid
}

fn capture(absolute: &Absolute) -> SecurityDescriptor {
    // SAFETY: the descriptor and everything it names outlive this call.
    unsafe { SecurityDescriptor::capture(absolute.as_ptr()) }.expect("capture the descriptor")
}

#[test]
fn an_absolute_descriptor_without_a_dacl_captures_as_absent() {
    let absolute = Absolute::without_dacl();

    let captured = capture(&absolute);

    assert_eq!(captured.dacl().expect("read the DACL"), AclState::Absent);
}

#[test]
fn an_absolute_descriptor_with_a_null_dacl_captures_as_null() {
    let absolute = Absolute::with_null_dacl();

    let captured = capture(&absolute);

    assert_eq!(captured.dacl().expect("read the DACL"), AclState::Null);
}

#[test]
fn an_absolute_descriptor_with_an_empty_dacl_captures_as_empty() {
    let absolute = Absolute::with_empty_dacl();

    let captured = capture(&absolute);

    assert_eq!(captured.dacl().expect("read the DACL"), AclState::Empty);
}

#[test]
fn the_three_dacl_outcomes_stay_distinct() {
    // The point of the type: these are three different grants, and a
    // representation that ran any pair of them together would silently change
    // what the resulting object permits.
    let absent = capture(&Absolute::without_dacl())
        .dacl()
        .expect("read the DACL");
    let null = capture(&Absolute::with_null_dacl())
        .dacl()
        .expect("read the DACL");
    let empty = capture(&Absolute::with_empty_dacl())
        .dacl()
        .expect("read the DACL");

    assert_ne!(absent, null);
    assert_ne!(absent, empty);
    assert_ne!(
        null, empty,
        "a NULL DACL allows all; an empty DACL allows none"
    );
}

#[test]
fn an_absolute_descriptor_with_a_populated_dacl_reports_its_entry_count() {
    let absolute = Absolute::with_populated_dacl();

    let captured = capture(&absolute);

    assert_eq!(
        captured.dacl().expect("read the DACL"),
        AclState::Populated(1)
    );
}

#[test]
fn a_descriptor_without_a_sacl_captures_as_absent() {
    let absolute = Absolute::with_populated_dacl();

    let captured = capture(&absolute);

    assert_eq!(captured.sacl().expect("read the SACL"), AclState::Absent);
}

#[test]
fn a_captured_descriptor_is_dword_aligned() {
    let captured = capture(&Absolute::with_populated_dacl());

    assert_eq!(
        captured.as_ptr() as usize % SELF_RELATIVE_ALIGNMENT,
        0,
        "a self-relative descriptor must be DWORD-aligned, which a boxed byte \
         slice would not guarantee"
    );
    assert!(!captured.is_empty());
    assert_eq!(captured.len(), captured.as_bytes().len());
}

#[test]
fn a_self_relative_descriptor_is_captured_byte_for_byte() {
    let absolute = Absolute::with_populated_dacl();
    let self_relative = absolute.to_self_relative();

    // SAFETY: the blob outlives this call and holds a valid descriptor.
    let captured = unsafe { SecurityDescriptor::capture(self_relative.as_ptr().cast::<c_void>()) }
        .expect("capture a self-relative descriptor");

    assert_eq!(captured.as_bytes(), self_relative.as_slice());
    assert_eq!(
        captured.dacl().expect("read the DACL"),
        AclState::Populated(1)
    );
}

#[test]
fn the_capture_survives_the_caller_dropping_everything_it_was_built_from() {
    // The property the whole design rests on: an absolute descriptor's owner,
    // group and DACL live in the caller's storage, so a capture that merely
    // copied the struct would be left holding dangling pointers.
    let captured = {
        let absolute = Absolute::with_populated_dacl();
        capture(&absolute)
    };

    assert_eq!(
        captured.dacl().expect("read the DACL"),
        AclState::Populated(1)
    );
    assert_eq!(captured.sacl().expect("read the SACL"), AclState::Absent);
}

#[test]
fn a_malformed_descriptor_is_refused_at_capture() {
    let zeroed = aligned_zeroed(size_of::<SECURITY_DESCRIPTOR>(), SELF_RELATIVE_ALIGNMENT);

    // SAFETY: the buffer outlives the call; its contents are not a valid
    // descriptor, which is what this asserts.
    let error = unsafe { SecurityDescriptor::capture(zeroed.as_ptr().cast::<c_void>()) }
        .expect_err("a zeroed descriptor has revision 0 and cannot be valid");

    assert_eq!(error.failure(), SecurityCaptureFailure::InvalidDescriptor);
    assert!(
        error.to_string().contains("IsValidSecurityDescriptor"),
        "unexpected message: {error}"
    );
}

#[test]
fn a_copy_equals_its_original_and_is_independent() {
    let captured = capture(&Absolute::with_populated_dacl());

    let copy = captured.try_clone().expect("copy the descriptor");

    assert_eq!(copy, captured);
    assert_ne!(copy.as_ptr(), captured.as_ptr());
    drop(captured);
    assert_eq!(copy.dacl().expect("read the DACL"), AclState::Populated(1));
}

#[test]
fn attributes_copy_with_and_without_a_descriptor() {
    let with = SecurityAttributes::new(Some(capture(&Absolute::with_populated_dacl())), true);
    let copy = with.try_clone().expect("copy the attributes");

    assert_eq!(copy, with);
    assert_ne!(
        copy.descriptor()
            .expect("the copy keeps its descriptor")
            .as_ptr(),
        with.descriptor().expect("the original has one").as_ptr(),
        "the copy owns its own blob"
    );

    let without = SecurityAttributes::new(None, false);
    assert_eq!(without.try_clone().expect("copy the attributes"), without);
}

#[test]
fn a_blob_the_machine_cannot_supply_is_an_allocation_failure() {
    // The seam every allocation of a descriptor goes through: the OS names the length, so no
    // descriptor can be made to ask for this much, but the function can.
    let error = allocate(usize::MAX / 2).expect_err("no machine has this");

    assert_eq!(error.failure(), SecurityCaptureFailure::Allocate);
    assert_eq!(
        error.raw_os_error(),
        Some(i32::try_from(ERROR_NOT_ENOUGH_MEMORY).expect("the code fits in i32"))
    );
    assert!(
        error.to_string().contains("allocating"),
        "unexpected message: {error}"
    );
}

#[test]
fn a_blob_the_machine_can_supply_is_zeroed_and_aligned() {
    let blob = allocate(64).expect("a small blob");

    assert_eq!(blob.len(), 64);
    assert!(blob.iter().all(|&byte| byte == 0));
    assert_eq!(blob.as_ptr() as usize % SELF_RELATIVE_ALIGNMENT, 0);
}

#[test]
fn the_underlying_error_of_a_failed_copy_is_the_one_a_caller_can_report_under_its_own_kind() {
    let error = allocate(usize::MAX / 2).expect_err("no machine has this");
    let source = error.into_source();

    assert_eq!(
        source.raw_os_error(),
        Some(i32::try_from(ERROR_NOT_ENOUGH_MEMORY).expect("the code fits in i32"))
    );
}

#[test]
fn descriptors_differing_in_their_dacl_are_not_equal() {
    let null = capture(&Absolute::with_null_dacl());
    let empty = capture(&Absolute::with_empty_dacl());

    assert_ne!(null, empty);
}

#[test]
fn null_attributes_are_the_caller_declining_rather_than_an_error() {
    // SAFETY: a null pointer is checked, never dereferenced.
    let captured = unsafe { SecurityAttributes::capture(ptr::null()) }
        .expect("a null lpSecurityAttributes is not a failure");

    assert!(
        captured.is_none(),
        "no attributes at all differs from attributes carrying no descriptor"
    );
}

#[test]
fn attributes_with_no_descriptor_keep_the_inheritance_choice() {
    let raw = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: TRUE,
    };

    // SAFETY: raw outlives the call and names no descriptor.
    let captured = unsafe { SecurityAttributes::capture(&raw) }
        .expect("capture attributes")
        .expect("the attributes were supplied");

    assert!(captured.descriptor().is_none());
    assert!(captured.inherit_handle());
}

#[test]
fn attributes_with_a_descriptor_capture_both_parts() {
    let absolute = Absolute::with_populated_dacl();
    let raw = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: absolute.as_ptr().cast_mut(),
        bInheritHandle: FALSE,
    };

    // SAFETY: raw and the descriptor it names both outlive the call.
    let captured = unsafe { SecurityAttributes::capture(&raw) }
        .expect("capture attributes")
        .expect("the attributes were supplied");

    assert!(!captured.inherit_handle());
    assert_eq!(
        captured
            .descriptor()
            .expect("a descriptor was supplied")
            .dacl()
            .expect("read the DACL"),
        AclState::Populated(1)
    );
}

#[test]
fn to_raw_rebuilds_the_win32_struct_from_the_capture() {
    let absolute = Absolute::with_populated_dacl();
    let captured = SecurityAttributes::new(Some(capture(&absolute)), true);

    let raw = captured.to_raw();

    assert_eq!(raw.nLength as usize, size_of::<SECURITY_ATTRIBUTES>());
    assert_eq!(raw.bInheritHandle, TRUE);
    assert_eq!(
        raw.lpSecurityDescriptor.cast_const(),
        captured
            .descriptor()
            .expect("a descriptor was supplied")
            .as_ptr()
    );
}

#[test]
fn to_raw_reports_a_null_descriptor_when_none_was_captured() {
    let captured = SecurityAttributes::new(None, false);

    let raw = captured.to_raw();

    assert!(raw.lpSecurityDescriptor.is_null());
    assert_eq!(raw.bInheritHandle, FALSE);
}

#[test]
fn a_capture_moves_and_shares_across_threads() {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}

    assert_send::<SecurityDescriptor>();
    assert_sync::<SecurityDescriptor>();
    assert_send::<SecurityAttributes>();
    assert_sync::<SecurityAttributes>();

    let captured = SecurityAttributes::new(Some(capture(&Absolute::with_populated_dacl())), true);

    let observed = std::thread::spawn(move || {
        captured
            .descriptor()
            .expect("a descriptor was supplied")
            .dacl()
            .expect("read the DACL")
    })
    .join()
    .expect("the worker did not panic");

    assert_eq!(observed, AclState::Populated(1));
}

#[test]
fn a_capture_failure_exposes_its_os_error_both_ways() {
    // `raw_os_error` survived replacement by `None`, `Some(0)`, `Some(1)`, and
    // `Some(-1)`, and `source` survived replacement by `None`. The test above
    // asserts the failure stage and the rendered message, neither of which
    // touches either accessor.
    //
    // The two routes are asserted against each other rather than against a
    // literal code: which error Windows reports for a zeroed descriptor is its
    // business, but whatever it is must reach a caller identically through the
    // typed accessor and through the standard `source` chain. That also rules
    // out every constant the sweep tried, including the plausible-looking ones.
    use std::error::Error as _;

    let zeroed = aligned_zeroed(size_of::<SECURITY_DESCRIPTOR>(), SELF_RELATIVE_ALIGNMENT);

    // SAFETY: the buffer outlives the call; its contents are not a valid
    // descriptor, which is what makes this fail.
    let error = unsafe { SecurityDescriptor::capture(zeroed.as_ptr().cast::<c_void>()) }
        .expect_err("a zeroed descriptor has revision 0 and cannot be valid");

    // Compared as `Option<i32>` on both sides, and deliberately *not* asserted to
    // be present or non-zero. This failure path takes `GetLastError` after
    // `IsValidSecurityDescriptor`, which Windows does not document as setting it,
    // so any particular value here is observed behaviour rather than a promise.
    //
    // Measured on this machine: it does set it, to 1338
    // (`ERROR_INVALID_SECURITY_DESCR`), and does so even when last-error is
    // poisoned with an unrelated value or cleared to 0 beforehand -- so the code
    // is neither stale nor absent in practice. That is exactly why it is not
    // asserted: an undocumented behaviour that happens to hold is the kind of
    // thing this crate binds to a specification instead, and a test demanding it
    // would be asserting the platform's incidental behaviour rather than this
    // crate's contract.
    //
    // What *is* this crate's contract is that both routes agree, which holds
    // whatever Windows reports -- including nothing at all.
    let source = error.source().expect("the OS error is the source");
    assert_eq!(
        source
            .downcast_ref::<std::io::Error>()
            .expect("the source is the io::Error behind the failure")
            .raw_os_error(),
        error.raw_os_error(),
        "the typed accessor and the source chain must report the same error"
    );
}
