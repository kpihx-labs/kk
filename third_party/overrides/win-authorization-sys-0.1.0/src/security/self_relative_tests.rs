// Copyright (c) Mike Grier.

//! A self-relative descriptor whose parts are not one contiguous span after the header.
//!
//! The self-relative format locates each part by an offset from the header, so parts may share
//! bytes or have space between them, and `GetSecurityDescriptorLength` -- which counts the parts --
//! is then not the span the descriptor occupies. Both layouts below validate, and capture must
//! read each part where its offset says and own every byte the copy names.

use std::ffi::c_void;
use std::ptr;

use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_NOACCESS, PAGE_PROTECTION_FLAGS, PAGE_READWRITE,
    VirtualAlloc, VirtualFree, VirtualProtect,
};

use super::{AclState, SecurityDescriptor, SecurityInformation};

/// `SE_SELF_RELATIVE`.
const SELF_RELATIVE: u16 = 0x8000;
/// `SE_DACL_PRESENT`.
const DACL_PRESENT: u16 = 0x0004;
/// The size of the self-relative header: revision, padding, control, then four offsets.
const HEADER: usize = 20;
/// Where in the header each part's offset is.
const OWNER_AT: usize = 4;
const GROUP_AT: usize = 8;
const SACL_AT: usize = 12;
const DACL_AT: usize = 16;
/// `S-1-1-0`, Everyone: revision 1, one sub-authority, the world authority, sub-authority 0.
const EVERYONE: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0];
/// An empty ACL: revision 2, padding, a size of 8, no entries, padding.
const EMPTY_ACL: [u8; 8] = [2, 0, 8, 0, 0, 0, 0, 0];
/// The page size these tests lay a descriptor out against.
const PAGE: usize = 4096;

fn header(control: u16, owner: u32, group: u32, sacl: u32, dacl: u32) -> [u8; HEADER] {
    let mut bytes = [0_u8; HEADER];
    bytes[0] = 1;
    bytes[2..4].copy_from_slice(&control.to_le_bytes());
    bytes[OWNER_AT..OWNER_AT + 4].copy_from_slice(&owner.to_le_bytes());
    bytes[GROUP_AT..GROUP_AT + 4].copy_from_slice(&group.to_le_bytes());
    bytes[SACL_AT..SACL_AT + 4].copy_from_slice(&sacl.to_le_bytes());
    bytes[DACL_AT..DACL_AT + 4].copy_from_slice(&dacl.to_le_bytes());
    bytes
}

/// The offset a part is named at in a self-relative blob.
fn offset_in(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes")) as usize
}

/// Every part the captured copy names starts inside the copy.
fn names_nothing_outside_itself(captured: &SecurityDescriptor) {
    let bytes = captured.as_bytes();
    for at in [OWNER_AT, GROUP_AT, SACL_AT, DACL_AT] {
        let offset = offset_in(bytes, at);
        assert!(
            offset < bytes.len(),
            "the part at header offset {at} is named at {offset}, past the copy's {} bytes",
            bytes.len()
        );
    }
}

/// Two pages, the second inaccessible, so that a read past the end of the first faults.
struct GuardedPage {
    base: *mut c_void,
}

impl GuardedPage {
    fn new() -> Self {
        // SAFETY: a fresh reservation of two pages with no address requested.
        let base = unsafe {
            VirtualAlloc(
                ptr::null(),
                2 * PAGE,
                MEM_RESERVE | MEM_COMMIT,
                PAGE_READWRITE,
            )
        };
        assert!(!base.is_null(), "allocate two pages");
        let mut previous: PAGE_PROTECTION_FLAGS = 0;
        // SAFETY: the second page is inside the allocation just made.
        let protected = unsafe {
            VirtualProtect(
                base.cast::<u8>().wrapping_add(PAGE).cast(),
                PAGE,
                PAGE_NOACCESS,
                &raw mut previous,
            )
        };
        assert_ne!(protected, 0, "make the second page inaccessible");
        Self { base }
    }

    /// Writes `bytes` so that they end exactly where the readable page does.
    fn place_at_end(&mut self, bytes: &[u8]) -> *const c_void {
        let start = self.base.cast::<u8>().wrapping_add(PAGE - bytes.len());
        // SAFETY: `start` and the `bytes.len()` bytes after it are in the readable page, which this
        // value owns.
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), start, bytes.len()) };
        start.cast_const().cast()
    }
}

impl Drop for GuardedPage {
    fn drop(&mut self) {
        // SAFETY: the whole allocation, released once.
        unsafe { VirtualFree(self.base, 0, MEM_RELEASE) };
    }
}

#[test]
fn an_owner_and_group_that_share_one_sid_are_read_where_they_are() {
    // 32 bytes, of which `GetSecurityDescriptorLength` counts 44 (the SID twice); the blob ends at
    // the guard page, so reading 44 from it faults.
    let mut bytes = header(SELF_RELATIVE, 20, 20, 0, 0).to_vec();
    bytes.extend_from_slice(&EVERYONE);
    let mut page = GuardedPage::new();
    let source = page.place_at_end(&bytes);

    // SAFETY: a valid self-relative descriptor, live for the call.
    let captured = unsafe { SecurityDescriptor::capture(source) }.expect("capture the descriptor");

    names_nothing_outside_itself(&captured);
    let text = captured
        .to_sddl(SecurityInformation::OWNER | SecurityInformation::GROUP)
        .expect("write the owner and group")
        .to_string_lossy();
    assert_eq!(text, "O:WDG:WD");
}

#[test]
fn a_part_after_a_gap_is_read_where_its_offset_says() {
    // The DACL is at 512, after a gap; `GetSecurityDescriptorLength` counts 28, so a copy of that
    // many bytes would name a DACL 484 bytes past its own end.
    const DACL: usize = 512;
    let mut bytes = vec![0_u8; DACL + EMPTY_ACL.len()];
    bytes[..HEADER].copy_from_slice(&header(SELF_RELATIVE | DACL_PRESENT, 0, 0, 0, 512));
    bytes[DACL..].copy_from_slice(&EMPTY_ACL);
    let mut page = GuardedPage::new();
    let source = page.place_at_end(&bytes);

    // SAFETY: a valid self-relative descriptor, live for the call.
    let captured = unsafe { SecurityDescriptor::capture(source) }.expect("capture the descriptor");

    names_nothing_outside_itself(&captured);
    assert_eq!(captured.dacl().expect("the DACL"), AclState::Empty);
    assert!(
        captured.len() < bytes.len(),
        "the gap is not carried: {} bytes",
        captured.len()
    );
}

#[test]
fn a_contiguous_descriptor_is_rebuilt_to_the_same_parts() {
    // The ordinary layout, parts one after another, as Windows writes it.
    let descriptor = SecurityDescriptor::from_sddl(&wtf_string::Wtf16String::from(
        "O:BAG:SYD:P(A;;FA;;;WD)(D;;FW;;;AU)",
    ))
    .expect("a descriptor");
    let all = SecurityInformation::OWNER | SecurityInformation::GROUP | SecurityInformation::DACL;

    // SAFETY: a valid self-relative descriptor this test owns.
    let captured = unsafe { SecurityDescriptor::capture(descriptor.as_ptr()) }.expect("capture");

    names_nothing_outside_itself(&captured);
    assert_eq!(
        captured.to_sddl(all).expect("text").to_string_lossy(),
        descriptor.to_sddl(all).expect("text").to_string_lossy()
    );
}
