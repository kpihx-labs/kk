// Copyright (c) Mike Grier.

//! Tests for the captured root and the graft.
//!
//! What each rule does is tested through `anchored_at`, which takes the root as a value, so that no
//! test here depends on or changes the process current directory. The one test that must change it
//! runs in a child process of its own.

use super::*;

const DRIVE_DIRECTORY: &str = r"C:\work";
const UNC_DIRECTORY: &str = r"\\server\share\dir";

fn anchor(root: &str, child: &str) -> AnchoredPath {
    AnchoredPath::anchored_at(Wtf16String::from(child), &Wtf16String::from(root))
}

fn assert_grafted(root: &str, child: &str, expected: &str) {
    let anchored = anchor(root, child);
    assert!(anchored.is_grafted(), "{child:?} under {root:?} is grafted");
    assert_eq!(anchored.win32().to_string_lossy(), expected);
    assert_eq!(anchored.given().to_string_lossy(), child);
}

fn assert_not_grafted(root: &str, child: &str) {
    let anchored = anchor(root, child);
    assert!(
        !anchored.is_grafted(),
        "{child:?} under {root:?} is not grafted"
    );
    assert_eq!(anchored.win32().to_string_lossy(), child);
    assert_eq!(anchored.given().to_string_lossy(), child);
}

// --- The graft --------------------------------------------------------------------------------

#[test]
fn a_relative_child_is_joined_to_the_root_with_a_backslash() {
    assert_grafted(DRIVE_DIRECTORY, r"a\b.txt", r"C:\work\a\b.txt");
    assert_grafted(DRIVE_DIRECTORY, "a", r"C:\work\a");
    assert_grafted(UNC_DIRECTORY, "a", r"\\server\share\dir\a");
}

#[test]
fn a_root_that_ends_in_a_backslash_gets_no_second_one() {
    assert_grafted(r"C:\", "a", r"C:\a");
    assert_grafted(r"C:\work\", "a", r"C:\work\a");
    assert_grafted(r"\\server\share\", "a", r"\\server\share\a");
}

#[test]
fn dot_components_are_kept_for_win32_to_collapse() {
    assert_grafted(DRIVE_DIRECTORY, r".\a", r"C:\work\.\a");
    assert_grafted(DRIVE_DIRECTORY, r"..\a", r"C:\work\..\a");
    assert_grafted(DRIVE_DIRECTORY, r"a\..\b", r"C:\work\a\..\b");
    assert_grafted(DRIVE_DIRECTORY, "a.", r"C:\work\a.");
    assert_grafted(DRIVE_DIRECTORY, "a ", r"C:\work\a ");
}

#[test]
fn a_forward_slash_inside_the_child_is_kept() {
    assert_grafted(DRIVE_DIRECTORY, "a/b", r"C:\work\a/b");
}

#[test]
fn nothing_is_prefixed_or_limited() {
    let long = "a".repeat(400);
    let anchored = anchor(DRIVE_DIRECTORY, &long);
    assert_eq!(
        anchored.win32().to_string_lossy(),
        format!(r"C:\work\{long}")
    );
    assert!(!anchored.win32().to_string_lossy().starts_with(r"\\?\"));
}

#[test]
fn units_that_are_not_text_are_carried_as_they_are() {
    let child = Wtf16String::from_units(&[0x0061, 0xD800, 0x0062]);
    let anchored = AnchoredPath::anchored_at(child, &Wtf16String::from(DRIVE_DIRECTORY));

    let expected: Vec<u16> = r"C:\work\"
        .encode_utf16()
        .chain([0x0061, 0xD800, 0x0062])
        .collect();
    assert_eq!(anchored.win32().as_units(), expected);
}

#[test]
fn a_root_with_nothing_in_it_grafts_nothing() {
    // A join would give `\a`, which is root-relative: a different path than the one named.
    assert_not_grafted("", "a");
}

#[test]
fn the_path_as_given_is_unaltered_by_a_graft() {
    let anchored = anchor(DRIVE_DIRECTORY, "a");
    assert_eq!(
        anchored.given().as_units(),
        "a".encode_utf16().collect::<Vec<_>>()
    );
}

// --- Each rule, both ways: a child that meets it, and one that breaks only that rule ---------

#[test]
fn rule_1_an_empty_child_is_not_grafted() {
    assert_not_grafted(DRIVE_DIRECTORY, "");
    assert_grafted(DRIVE_DIRECTORY, "a", r"C:\work\a");
}

#[test]
fn rule_2_a_child_that_begins_with_a_separator_is_not_grafted() {
    for child in [
        r"\a\b",
        "/a/b",
        r"\\server\share",
        r"\\?\C:\a",
        r"\\.\PhysicalDrive0",
        "//server/share",
        r"\",
        "/",
    ] {
        assert_not_grafted(DRIVE_DIRECTORY, child);
    }
    assert_grafted(DRIVE_DIRECTORY, r"a\b", r"C:\work\a\b");
}

#[test]
fn rule_3_a_child_with_a_colon_is_not_grafted() {
    for child in [
        "C:a",
        "C:",
        r"D:\a",
        "name:stream",
        r"a\name:stream",
        "a:",
        "name::$DATA",
    ] {
        assert_not_grafted(DRIVE_DIRECTORY, child);
    }
    assert_grafted(DRIVE_DIRECTORY, "name", r"C:\work\name");
}

#[test]
fn rule_4_a_parent_component_is_refused_only_under_a_unc_directory() {
    for child in [r"..\a", r"a\..\b", r"a\..", "..", r"a/../b"] {
        assert_not_grafted(UNC_DIRECTORY, child);
        assert!(anchor(DRIVE_DIRECTORY, child).is_grafted());
    }
    assert_grafted(UNC_DIRECTORY, r"a\b", r"\\server\share\dir\a\b");
    assert_grafted(UNC_DIRECTORY, r".\a", r"\\server\share\dir\.\a");
}

#[test]
fn rule_4_only_a_component_that_is_two_dots_is_a_parent() {
    for child in [r"a..\b", r"..a", "...", r"a\...\b", "a..", r"x\..y"] {
        assert!(
            anchor(UNC_DIRECTORY, child).is_grafted(),
            "{child:?} has no `..` component"
        );
    }
}

#[test]
fn rule_4_a_unc_directory_is_one_that_begins_with_two_separators() {
    for root in [r"\\server\share", r"\\?\UNC\server\share", "//server/share"] {
        assert_not_grafted(root, r"..\a");
    }
    for root in [r"C:\work", r"\work", r"\", "C:"] {
        assert!(anchor(root, r"..\a").is_grafted(), "{root:?} is not UNC");
    }
}

#[test]
fn rule_6_a_verbatim_or_device_directory_is_not_grafted_onto() {
    // Win32 does not normalise a `\\?\` path, so `.` and `..`, a forward slash and trailing dots and
    // spaces in the child would be read literally after the join. It does normalise a `\\.\` path
    // (D-28), so for that prefix the rule is wider than its reason, and errs toward not grafting.
    for root in [
        r"\\?\C:\work",
        r"\\?\C:\",
        r"\\?\UNC\server\share\dir",
        r"\\.\C:\work",
        r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\dir",
    ] {
        for child in ["a", r".\a", r"..\a", "a/b", "a.", "a ", r"a\b"] {
            assert_not_grafted(root, child);
        }
    }
}

#[test]
fn rule_6_only_the_two_prefixes_with_backslashes_are_those_of_a_verbatim_or_device_path() {
    // Close to the prefix and not it: the third unit is something else, the fourth is not a
    // backslash, or the separators are forward slashes, which Win32 does not read as the prefix.
    for root in [
        r"\\?x\a",
        r"\\.x\a",
        r"\\??\a",
        r"\\server\share\dir",
        r"\?\C:\work",
        r"C:\?\work",
    ] {
        assert!(
            anchor(root, "a").is_grafted(),
            "{root:?} is no verbatim or device prefix"
        );
    }
    // Forward slashes are not the prefix, so rule 6 does not decline these; they are UNC-shaped for
    // rule 4, which a child with no `..` does not meet.
    assert!(anchor("//?/C:/work", "a").is_grafted());
}

#[test]
fn rule_6_does_not_depend_on_the_child_the_other_rules_would_graft() {
    // The same child grafts under an ordinary directory and does not under a verbatim one, so it is
    // the directory that decides.
    assert!(anchor(DRIVE_DIRECTORY, r".\a").is_grafted());
    assert_not_grafted(r"\\?\C:\work", r".\a");
}

#[test]
fn rule_5_every_reserved_name_is_not_grafted() {
    let names = [
        "CON",
        "PRN",
        "AUX",
        "NUL",
        "CONIN$",
        "CONOUT$",
        "COM0",
        "COM1",
        "COM2",
        "COM3",
        "COM4",
        "COM5",
        "COM6",
        "COM7",
        "COM8",
        "COM9",
        "LPT0",
        "LPT1",
        "LPT2",
        "LPT3",
        "LPT4",
        "LPT5",
        "LPT6",
        "LPT7",
        "LPT8",
        "LPT9",
        "COM\u{b9}",
        "COM\u{b2}",
        "COM\u{b3}",
        "LPT\u{b9}",
        "LPT\u{b2}",
        "LPT\u{b3}",
    ];
    for name in names {
        assert_not_grafted(DRIVE_DIRECTORY, name);
    }
}

#[test]
fn rule_5_the_comparison_ignores_case() {
    for name in [
        "con", "Con", "cOn", "nul", "Aux", "prn", "conin$", "ConOut$", "com1", "lpt9",
    ] {
        assert_not_grafted(DRIVE_DIRECTORY, name);
    }
}

#[test]
fn rule_5_an_extension_and_trailing_spaces_do_not_save_a_name() {
    for name in [
        "CON.txt",
        "con.tar.gz",
        "NUL.",
        "CON ",
        "CON   ",
        "CON .txt",
        "COM1.x",
        "LPT1 ",
        "AUX.",
    ] {
        assert_not_grafted(DRIVE_DIRECTORY, name);
    }
}

#[test]
fn rule_5_a_name_in_any_component_of_the_child_counts() {
    for child in [
        r"a\CON",
        r"CON\a",
        r"a\NUL\b",
        "a/con/b",
        r"a\COM1.txt\b",
        r"..\CON",
    ] {
        assert_not_grafted(DRIVE_DIRECTORY, child);
    }
    assert_grafted(DRIVE_DIRECTORY, r"a\b\c", r"C:\work\a\b\c");
}

#[test]
fn rule_5_a_name_that_is_not_one_is_grafted() {
    for name in [
        "CONSOLE",
        "COM",
        "COM10",
        "COMA",
        "CON1",
        "NULL",
        "CONIN",
        "CONOUT",
        "AUX1",
        "PRN1",
        "LPT",
        "LPT10",
        "xCON",
        "CONx",
        "COM\u{b4}",
        "COM\u{660}",
        "a.CON",
        ".CON",
        "..CON",
        "C ON",
        "CÖN",
    ] {
        let anchored = anchor(DRIVE_DIRECTORY, name);
        assert!(anchored.is_grafted(), "{name:?} is not a reserved name");
    }
}

#[test]
fn rule_5_nul_is_not_grafted_though_a_graft_would_change_nothing() {
    // D-19 lists NUL among the reserved names, and the rule errs toward not grafting; `NUL` opens the
    // device whether or not anything precedes it, so the two answers agree on what is opened.
    assert_not_grafted(DRIVE_DIRECTORY, "NUL");
    assert_not_grafted(DRIVE_DIRECTORY, r"C:\work\NUL");
}

#[test]
fn rule_5_a_non_ascii_unit_never_matches_an_ascii_letter() {
    // U+212A is the Kelvin sign and U+0131 the dotless i, which fold to `k` and `i` under Unicode
    // case folding and are not ASCII.
    assert!(anchor(DRIVE_DIRECTORY, "CON\u{212a}").is_grafted());
    assert!(anchor(DRIVE_DIRECTORY, "PRN\u{0131}").is_grafted());
    assert!(anchor(DRIVE_DIRECTORY, "AUX\u{0131}").is_grafted());
    assert!(anchor(DRIVE_DIRECTORY, "\u{0131}").is_grafted());
    // A unit whose low byte is an ASCII letter is not that letter.
    assert!(anchor(DRIVE_DIRECTORY, "\u{143}ON").is_grafted());
    assert!(anchor(DRIVE_DIRECTORY, "CO\u{14e}").is_grafted());
    assert!(anchor(DRIVE_DIRECTORY, "COM\u{131}").is_grafted());
}

#[test]
fn the_reserved_name_check_on_a_component() {
    for component in ["CON", "con.", "Com5", " ", "", "NUL.txt."] {
        let units: Vec<u16> = component.encode_utf16().collect();
        let expected = matches!(component, "CON" | "con." | "Com5" | "NUL.txt.");
        assert_eq!(is_reserved_device_name(&units), expected, "{component:?}");
    }
}

// --- The current directory --------------------------------------------------------------------

#[test]
fn a_path_whose_shape_refuses_a_graft_reads_no_directory() {
    // Not observable from a call count, but from the path: an absolute path, a device and a stream
    // are all returned as given by `new`, and the error path is not taken for them.
    for path in [
        r"C:\a\b",
        r"\\server\share",
        r"\\.\PhysicalDrive0",
        "NUL",
        "",
        "name:stream",
    ] {
        let anchored = AnchoredPath::new(Wtf16String::from(path)).expect("no directory is needed");
        assert!(!anchored.is_grafted());
        assert_eq!(anchored.win32().to_string_lossy(), path);
    }
}

#[test]
fn an_interior_nul_is_refused_whatever_the_shape_of_the_path() {
    // A path that would graft, one that would not, and one that is a device: the refusal comes
    // first and does not depend on the rules.
    for units in [
        &[0x0061, 0x0000, 0x0062][..],
        &[0x0043, 0x003A, 0x005C, 0x0000, 0x0061][..],
        &[0x0000, 0x0061][..],
        &[0x004E, 0x0055, 0x004C, 0x0000, 0x0061][..],
    ] {
        let error = AnchoredPath::new(Wtf16String::from_units(units)).expect_err("a NUL truncates");
        assert!(matches!(error, PathCaptureError::InteriorNul), "{units:?}");
    }
}

#[test]
fn a_nul_at_the_end_is_the_one_that_is_refused_too() {
    // The string's own terminator is not content; a NUL the caller put last is.
    let error = AnchoredPath::new(Wtf16String::from_units(&[0x0061, 0x0000]))
        .expect_err("a trailing NUL is content");
    assert!(matches!(error, PathCaptureError::InteriorNul));
    assert!(error.to_string().contains("NUL"));
    assert!(std::error::Error::source(&error).is_none());
}

#[test]
fn new_grafts_onto_the_current_directory() {
    let anchored = AnchoredPath::new(Wtf16String::from(r"a\b")).expect("a directory is captured");
    let directory = std::env::current_dir().expect("a current directory");
    let directory = directory.to_str().expect("a UTF-8 current directory");

    assert!(anchored.is_grafted());
    assert_eq!(
        anchored.win32().to_string_lossy(),
        format!(r"{}\a\b", directory.trim_end_matches('\\')),
    );
}

#[test]
fn the_terminated_pointer_reads_the_path_handed_to_win32() {
    let anchored = anchor(DRIVE_DIRECTORY, "a");
    let pointer = anchored.win32_terminated();
    let expected: Vec<u16> = r"C:\work\a".encode_utf16().collect();

    // SAFETY: the pointer is to the content of `anchored`, which is live, followed by the terminator.
    let read = unsafe {
        let mut length = 0;
        while *pointer.add(length) != 0 {
            length += 1;
        }
        std::slice::from_raw_parts(pointer, length)
    };
    assert_eq!(read, expected);

    let plain = anchor(DRIVE_DIRECTORY, r"\a");
    // SAFETY: as above.
    assert_eq!(unsafe { *plain.win32_terminated() }, u16::from(b'\\'));
}

const CHILD_MARKER: &str = "WNRS_ANCHORED_CHILD";
const CHILD_TEST: &str =
    "anchored::tests::the_graft_names_the_directory_captured_not_the_current_one";

// The process current directory is shared by every test in the process, so a test that changes it
// runs in a child process of its own, as `tests/unc_current_directory.rs` does.
#[test]
fn the_graft_names_the_directory_captured_not_the_current_one() {
    if let Some(started_in) = std::env::var_os(CHILD_MARKER) {
        let moved_to = std::env::temp_dir();
        assert_ne!(
            std::path::Path::new(&started_in),
            moved_to,
            "precondition: the directory moved to is not the one the child started in"
        );

        let anchored = AnchoredPath::new(Wtf16String::from(r"a\b")).expect("a directory");
        let captured = std::env::current_dir().expect("a current directory");
        std::env::set_current_dir(&moved_to).expect("change the current directory");
        assert_ne!(
            std::env::current_dir().expect("a current directory"),
            captured,
            "precondition: the current directory changed after the capture"
        );

        let captured = captured.to_str().expect("UTF-8").trim_end_matches('\\');
        assert_eq!(
            anchored.win32().to_string_lossy(),
            format!(r"{captured}\a\b")
        );
        // The same request built now captures the new directory.
        let later = AnchoredPath::new(Wtf16String::from(r"a\b")).expect("a directory");
        assert_ne!(
            later.win32().to_string_lossy(),
            anchored.win32().to_string_lossy()
        );
        return;
    }

    // Spawning allocates the pipe handles, which must not reuse a value a test holding the write
    // lock is reasoning about.
    let _allocating = crate::handle::tests::handle_allocation()
        .read()
        .expect("the lock is not poisoned");
    let start = std::env::var_os("SystemRoot").expect("Windows names its directory");
    let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .arg("--exact")
        .arg(CHILD_TEST)
        .arg("--nocapture")
        .env(CHILD_MARKER, &start)
        .current_dir(&start)
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

// --- The retry around GetCurrentDirectoryW -----------------------------------------------------

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// A fake call that answers from `directory`, as `GetCurrentDirectoryW` does.
fn answering(directory: &str) -> impl FnMut(&mut [u16]) -> Result<u32, Win32Error> {
    let directory = units(directory);
    move |buffer| {
        if directory.len() < buffer.len() {
            buffer[..directory.len()].copy_from_slice(&directory);
            Ok(directory.len() as u32)
        } else {
            Ok(directory.len() as u32 + 1)
        }
    }
}

#[test]
fn a_directory_that_fits_is_read_on_the_first_attempt() {
    let mut calls = 0;
    let mut answer = answering(r"C:\work");
    let read = read_current_directory(|buffer| {
        calls += 1;
        answer(buffer)
    })
    .expect("fits");

    assert_eq!(read.to_string_lossy(), r"C:\work");
    assert_eq!(calls, 1);
}

#[test]
fn a_directory_longer_than_the_first_buffer_is_read_on_the_second() {
    let long = format!(r"C:\{}", "d".repeat(500));
    let mut calls = 0;
    let mut answer = answering(&long);
    let read = read_current_directory(|buffer| {
        calls += 1;
        answer(buffer)
    })
    .expect("grows");

    assert_eq!(read.to_string_lossy(), long);
    assert_eq!(calls, 2);
}

#[test]
fn a_directory_of_the_largest_size_the_first_buffer_holds_is_read_on_the_first_attempt() {
    let largest = format!(r"C:\{}", "d".repeat(FIRST_ATTEMPT_CHARS - 1 - 3));
    assert_eq!(largest.len(), FIRST_ATTEMPT_CHARS - 1);
    let mut calls = 0;
    let mut answer = answering(&largest);
    let read = read_current_directory(|buffer| {
        calls += 1;
        answer(buffer)
    })
    .expect("fits");

    assert_eq!(read.to_string_lossy(), largest);
    assert_eq!(calls, 1);
}

#[test]
fn a_directory_one_past_that_takes_a_second_attempt() {
    let one_past = format!(r"C:\{}", "d".repeat(FIRST_ATTEMPT_CHARS - 3));
    let mut calls = 0;
    let mut answer = answering(&one_past);
    let read = read_current_directory(|buffer| {
        calls += 1;
        answer(buffer)
    })
    .expect("grows");

    assert_eq!(read.to_string_lossy(), one_past);
    assert_eq!(calls, 2);
}

#[test]
fn a_refusal_is_reported_with_its_code_and_not_retried() {
    let mut calls = 0;
    let error = read_current_directory(|_| {
        calls += 1;
        Err(Win32Error::from_code(5))
    })
    .expect_err("refused");

    assert_eq!(calls, 1);
    assert!(matches!(error, PathCaptureError::CurrentDirectory(code) if code.code() == 5));
    assert!(error.to_string().contains("GetCurrentDirectoryW"));
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn a_size_that_keeps_changing_is_given_up_on() {
    let mut calls = 0;
    let error = read_current_directory(|buffer| {
        calls += 1;
        Ok(buffer.len() as u32 + 100)
    })
    .expect_err("never settles");

    assert_eq!(calls, MAX_ATTEMPTS);
    assert!(matches!(
        error,
        PathCaptureError::CurrentDirectoryUnstable { attempts } if attempts == MAX_ATTEMPTS
    ));
    assert!(error.to_string().contains("changed"));
    assert!(std::error::Error::source(&error).is_none());
}

#[test]
fn a_size_that_settles_on_the_last_attempt_is_read() {
    let mut calls = 0;
    let read = read_current_directory(|buffer| {
        calls += 1;
        if calls < MAX_ATTEMPTS {
            Ok(buffer.len() as u32 + 100)
        } else {
            buffer[0] = u16::from(b'C');
            Ok(1)
        }
    })
    .expect("settles");

    assert_eq!(read.to_string_lossy(), "C");
    assert_eq!(calls, MAX_ATTEMPTS);
}
