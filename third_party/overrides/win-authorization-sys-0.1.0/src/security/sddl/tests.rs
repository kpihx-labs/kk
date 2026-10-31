// Copyright (c) Mike Grier.
// Split from crates/windows-namespace-request-sys/src/security/sddl/tests.rs at 11020848.

//! Tests for SDDL text to and from a descriptor.
//!
//! The expectations are what this machine's Windows did, measured: Windows parses the text, so what
//! it accepts, the codes it refuses with, and how it renders a descriptor are its own and are not
//! restated here from the documentation.

use windows_sys::Win32::Foundation::{
    ERROR_INVALID_ACL, ERROR_INVALID_DATATYPE, ERROR_INVALID_PARAMETER, ERROR_INVALID_SID,
};
use windows_sys::Win32::Security::{
    GetSecurityDescriptorControl, GetSecurityDescriptorLength, PSECURITY_DESCRIPTOR,
    SE_SELF_RELATIVE,
};
use wtf_string::Wtf16String;

use super::{SecurityDescriptor, SecurityInformation};
use crate::{AclState, SecurityCaptureFailure};

fn text(sddl: &str) -> Wtf16String {
    Wtf16String::from(sddl)
}

fn parse(sddl: &str) -> SecurityDescriptor {
    SecurityDescriptor::from_sddl(&text(sddl)).unwrap_or_else(|error| panic!("{sddl:?}: {error}"))
}

fn render(descriptor: &SecurityDescriptor, information: SecurityInformation) -> String {
    descriptor
        .to_sddl(information)
        .expect("write the descriptor as text")
        .to_string_lossy()
}

const OWNER_GROUP_DACL: SecurityInformation = SecurityInformation::from_bits(
    SecurityInformation::OWNER.bits()
        | SecurityInformation::GROUP.bits()
        | SecurityInformation::DACL.bits(),
);

const EVERYONE_ALL: &str = "D:P(A;;GA;;;WD)";

// --- Text to a descriptor ---------------------------------------------------------------------

#[test]
fn a_string_with_a_dacl_reads_back_the_state_of_that_dacl() {
    let populated_two = "D:(A;;GA;;;WD)(A;;GR;;;BA)";
    for (sddl, expected) in [
        (EVERYONE_ALL, AclState::Populated(1)),
        (populated_two, AclState::Populated(2)),
        ("D:AI(A;;GA;;;WD)", AclState::Populated(1)),
        ("D:(A;;GA;;;S-1-5-21-1-2-3-500)", AclState::Populated(1)),
        // The three grants a single nullable pointer runs together.
        ("D:NO_ACCESS_CONTROL", AclState::Null),
        ("D:", AclState::Empty),
        ("O:BA", AclState::Absent),
        ("", AclState::Absent),
    ] {
        assert_eq!(
            parse(sddl).dacl().expect("read the DACL"),
            expected,
            "{sddl:?}"
        );
    }
}

#[test]
fn a_string_with_a_sacl_reads_back_the_state_of_that_sacl_and_none_of_the_dacl() {
    let descriptor = parse("S:(AU;SA;GA;;;WD)");

    assert_eq!(
        descriptor.sacl().expect("read the SACL"),
        AclState::Populated(1)
    );
    assert_eq!(descriptor.dacl().expect("read the DACL"), AclState::Absent);
    assert_eq!(
        parse(EVERYONE_ALL).sacl().expect("read the SACL"),
        AclState::Absent
    );
}

#[test]
fn a_descriptor_from_a_string_is_self_relative_aligned_and_exactly_its_own_length() {
    for sddl in [EVERYONE_ALL, "O:BAG:SYD:P(A;;GA;;;WD)", "D:", ""] {
        let descriptor = parse(sddl);
        let pointer: PSECURITY_DESCRIPTOR = descriptor.as_ptr().cast_mut();

        let mut control = 0_u16;
        let mut revision = 0_u32;
        // SAFETY: the pointer is the descriptor's own, borrowed for the call, and both
        // out-parameters are writable.
        let read =
            unsafe { GetSecurityDescriptorControl(pointer, &raw mut control, &raw mut revision) };
        assert_ne!(read, 0, "{sddl:?}");
        assert_ne!(control & SE_SELF_RELATIVE, 0, "{sddl:?}: self-relative");
        assert_eq!(
            descriptor.as_ptr().addr() % align_of::<u32>(),
            0,
            "{sddl:?}: aligned"
        );
        // SAFETY: as above.
        let length = unsafe { GetSecurityDescriptorLength(pointer) } as usize;
        assert_eq!(descriptor.len(), length, "{sddl:?}: exactly the descriptor");
    }
}

#[test]
fn two_texts_for_one_descriptor_give_the_same_descriptor() {
    // The text form is not canonical: a rights mask and its alias name the same right.
    let spelled_out = parse("D:(A;;0x1f01ff;;;WD)");
    let aliased = parse("D:(A;;FA;;;WD)");

    assert_eq!(spelled_out, aliased);
    assert_ne!(spelled_out, parse("D:(A;;GA;;;WD)"));
}

#[test]
fn parsing_the_same_text_twice_gives_equal_descriptors_that_do_not_share_memory() {
    let first = parse(EVERYONE_ALL);
    let second = parse(EVERYONE_ALL);

    assert_eq!(first, second);
    assert_ne!(first.as_ptr(), second.as_ptr());
    assert_eq!(first.try_clone().expect("copy"), first);
}

#[test]
fn a_dacl_of_fifteen_hundred_entries_is_read_whole() {
    let mut sddl = String::from("D:");
    for _ in 0..1500 {
        sddl.push_str("(A;;GR;;;WD)");
    }

    let descriptor = parse(&sddl);

    assert_eq!(
        descriptor.dacl().expect("read the DACL"),
        AclState::Populated(1500)
    );
    assert_eq!(render(&descriptor, SecurityInformation::DACL), sddl);
}

// --- What Windows refuses, with its own code --------------------------------------------------

fn refused(sddl: &Wtf16String) -> (SecurityCaptureFailure, Option<i32>) {
    let error = SecurityDescriptor::from_sddl(sddl).expect_err("Windows refuses this text");
    (error.failure(), error.raw_os_error())
}

#[test]
fn text_windows_refuses_is_its_own_code_unaltered() {
    for (sddl, code) in [
        ("not sddl", ERROR_INVALID_PARAMETER),
        // The parts and the aliases are case sensitive.
        ("d:p(a;;ga;;;wd)", ERROR_INVALID_PARAMETER),
        ("D:(Z;;GA;;;WD)", ERROR_INVALID_DATATYPE),
        ("D:(A;;GA;;;ZZ)", ERROR_INVALID_SID),
    ] {
        assert_eq!(
            refused(&text(sddl)),
            (
                SecurityCaptureFailure::InvalidSddl,
                Some(code.cast_signed())
            ),
            "{sddl:?}"
        );
    }
}

#[test]
fn a_dacl_with_more_entries_than_an_acl_can_hold_is_the_codes_for_it() {
    let mut sddl = String::from("D:");
    for _ in 0..6000 {
        sddl.push_str("(A;;GR;;;WD)");
    }

    assert_eq!(
        refused(&text(&sddl)),
        (
            SecurityCaptureFailure::InvalidSddl,
            Some(ERROR_INVALID_ACL.cast_signed())
        )
    );
}

#[test]
fn an_unpaired_surrogate_is_refused_and_does_not_panic() {
    let lone = Wtf16String::from_units(&[0x44, 0x3A, 0xD800]);

    assert_eq!(
        refused(&lone),
        (
            SecurityCaptureFailure::InvalidSddl,
            Some(ERROR_INVALID_PARAMETER.cast_signed())
        )
    );
}

#[test]
fn a_string_with_an_interior_nul_is_refused_before_windows_reads_it() {
    // Windows would read up to the NUL and accept what is before it, which is a different
    // descriptor than the string says.
    let mut units: Vec<u16> = EVERYONE_ALL.encode_utf16().collect();
    units.push(0);
    units.extend("garbage".encode_utf16());

    assert_eq!(
        refused(&Wtf16String::from_units(&units)),
        (
            SecurityCaptureFailure::InteriorNul,
            Some(ERROR_INVALID_PARAMETER.cast_signed())
        )
    );
    assert_eq!(
        refused(&Wtf16String::from_units(&[0])),
        (
            SecurityCaptureFailure::InteriorNul,
            Some(ERROR_INVALID_PARAMETER.cast_signed())
        )
    );
}

// --- A descriptor to text ---------------------------------------------------------------------

#[test]
fn a_descriptor_written_as_text_is_the_text_windows_renders() {
    for sddl in [
        EVERYONE_ALL,
        "D:(A;;GA;;;WD)(A;;GR;;;BA)",
        "D:NO_ACCESS_CONTROL",
        "D:AI(A;;GA;;;WD)",
        "D:(A;;GA;;;S-1-5-21-1-2-3-500)",
        "O:BAG:SYD:P(A;;GA;;;WD)",
        "S:(AU;SA;GA;;;WD)",
    ] {
        let all = SecurityInformation::OWNER
            | SecurityInformation::GROUP
            | SecurityInformation::DACL
            | SecurityInformation::SACL;
        assert_eq!(render(&parse(sddl), all), sddl);
    }
}

#[test]
fn the_text_is_not_always_the_text_the_descriptor_was_built_from() {
    assert_eq!(
        render(&parse("D:(A;;0x1f01ff;;;WD)"), SecurityInformation::DACL),
        "D:(A;;FA;;;WD)"
    );
}

#[test]
fn the_text_carries_exactly_the_string_and_no_terminator() {
    // Windows reports a length for the text that is the string's length plus one for some strings,
    // plus two for others, and no more than the string's for others. A copy that trusted it would
    // end in a NUL or cut the last character off; these are strings of each kind.
    for sddl in [
        EVERYONE_ALL,
        "D:(A;;GA;;;WD)(A;;GR;;;BA)",
        "O:BA",
        "D:",
        "D:NO_ACCESS_CONTROL",
    ] {
        let written = parse(sddl)
            .to_sddl(SecurityInformation::OWNER | SecurityInformation::DACL)
            .expect("write the descriptor as text");

        assert!(!written.has_interior_nul(), "{sddl:?}");
        assert_eq!(written.to_string_lossy(), sddl, "{sddl:?}");
    }
}

#[test]
fn the_mask_selects_the_parts_that_are_written() {
    let descriptor = parse("O:BAG:SYD:P(A;;GA;;;WD)");

    assert_eq!(render(&descriptor, SecurityInformation::OWNER), "O:BA");
    assert_eq!(render(&descriptor, SecurityInformation::GROUP), "G:SY");
    assert_eq!(render(&descriptor, SecurityInformation::DACL), EVERYONE_ALL);
    assert_eq!(
        render(
            &descriptor,
            SecurityInformation::OWNER | SecurityInformation::DACL
        ),
        "O:BAD:P(A;;GA;;;WD)"
    );
    assert_eq!(
        render(&descriptor, OWNER_GROUP_DACL),
        "O:BAG:SYD:P(A;;GA;;;WD)"
    );
}

#[test]
fn a_part_the_descriptor_does_not_have_is_left_out() {
    let descriptor = parse(EVERYONE_ALL);

    assert_eq!(render(&descriptor, SecurityInformation::OWNER), "");
    assert_eq!(render(&descriptor, SecurityInformation::SACL), "");
    assert_eq!(render(&descriptor, SecurityInformation::NONE), "");
}

#[test]
fn text_made_from_a_descriptor_makes_the_same_descriptor() {
    for sddl in [
        EVERYONE_ALL,
        "D:NO_ACCESS_CONTROL",
        "D:",
        "O:BAG:SYD:P(A;;GA;;;WD)",
        "S:(AU;SA;GA;;;WD)",
    ] {
        let original = parse(sddl);
        let all = SecurityInformation::OWNER
            | SecurityInformation::GROUP
            | SecurityInformation::DACL
            | SecurityInformation::SACL;

        let again = SecurityDescriptor::from_sddl(&original.to_sddl(all).expect("write"))
            .expect("read what was written");

        assert_eq!(again, original, "{sddl:?}");
    }
}

#[test]
fn the_selection_names_the_win32_values_and_combines() {
    assert_eq!(SecurityInformation::NONE.bits(), 0);
    assert_eq!(SecurityInformation::OWNER.bits(), 1);
    assert_eq!(SecurityInformation::GROUP.bits(), 2);
    assert_eq!(SecurityInformation::DACL.bits(), 4);
    assert_eq!(SecurityInformation::SACL.bits(), 8);
    assert_eq!(SecurityInformation::LABEL.bits(), 16);
    assert_eq!(SecurityInformation::from_bits(0x40).bits(), 0x40);
    assert_eq!(SecurityInformation::default(), SecurityInformation::NONE);

    let both = SecurityInformation::OWNER | SecurityInformation::DACL;
    assert_eq!(both.bits(), 5);
    assert!(both.contains(SecurityInformation::OWNER));
    assert!(both.contains(SecurityInformation::DACL));
    assert!(both.contains(SecurityInformation::NONE));
    assert!(!both.contains(SecurityInformation::GROUP));
    assert!(!SecurityInformation::OWNER.contains(SecurityInformation::DACL));
}
