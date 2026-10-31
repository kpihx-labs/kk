// Copyright (c) Mike Grier.

//! Tests for the `GetFullPathNameW` entry.
//!
//! The negatives matter more than the positives here: this call does not verify
//! what it produces, and a suite that only ever resolved existing paths would
//! leave a reader believing it does.

use windows_sys::Win32::Foundation::{
    ERROR_ENVVAR_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS,
};
use wtf_string::Wtf16String;

use super::{FullPathError, ResolveFullPath};
use crate::anchored::PathCaptureError;
use crate::handle::tests::handle_allocation;
use crate::outcome::Win32Error;

mod drive_entry;

fn resolve(path: &str) -> String {
    ResolveFullPath::new(Wtf16String::from(path))
        .expect("the path has no NUL")
        .perform()
        .expect("resolve the path")
        .to_string_lossy()
}

#[test]
fn dot_and_dotdot_components_are_resolved() {
    assert_eq!(
        resolve(r"C:\Windows\System32\..\.\Temp"),
        r"C:\Windows\Temp"
    );
}

#[test]
fn a_trailing_dot_component_is_resolved_away() {
    assert_eq!(resolve(r"C:\Windows\."), r"C:\Windows");
}

#[test]
fn forward_slashes_are_normalised_to_backslashes() {
    assert_eq!(resolve("C:/Windows/System32"), r"C:\Windows\System32");
}

#[test]
fn an_already_absolute_path_is_returned_unchanged() {
    assert_eq!(resolve(r"C:\Windows"), r"C:\Windows");
}

#[test]
fn a_path_that_does_not_exist_resolves_perfectly_happily() {
    // Resolution does not verify the result: no error, and no check that any
    // component exists. A consumer wanting a verified path wants an open plus
    // GetFinalPathNameByHandleW.
    //
    // The components are process-specific and their absence is asserted first.
    // A hard-coded literal is only missing until some host happens to have it,
    // and this test would then be demonstrating that an EXISTING path resolves
    // -- which every other test here already covers.
    // `Path::exists` goes through `CreateFileW` on Windows, so it allocates a
    // handle and this suite serialises that -- see `ProbeDir::_allocating`.
    // This test predates the fixture and was the one filesystem call in the
    // module not covered by it.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");

    let missing = std::env::temp_dir().join(format!("wnrs-{}-absent", std::process::id()));
    let missing = missing.to_str().expect("the temp path is UTF-8");
    assert!(
        !std::path::Path::new(missing).exists(),
        "precondition: {missing} must not exist"
    );

    let doubled = format!(r"{missing}\also-absent\..\leaf.txt");
    assert_eq!(resolve(&doubled), format!(r"{missing}\leaf.txt"));
}

#[test]
fn a_drive_letter_is_never_expanded() {
    // The hazard this entry does NOT close, asserted so the documentation and
    // the behaviour cannot drift apart: the result still starts with the drive
    // letter, whose meaning depends on the logon session at open time.
    let resolved = resolve(r"C:\Windows");

    assert!(
        resolved.starts_with("C:"),
        "the drive letter survives resolution: {resolved}"
    );
    assert!(
        !resolved.starts_with(r"\\?\"),
        "and it is not turned into a verbatim or device path: {resolved}"
    );
}

#[test]
fn a_relative_path_resolves_against_the_current_directory() {
    // Which is exactly why this must happen where the caller is: the process
    // current directory is shared mutable state.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");

    let current = std::env::current_dir().expect("read the current directory");
    let resolved = resolve("some-relative-name.txt");

    let expected = current.join("some-relative-name.txt");
    assert_eq!(
        resolved,
        expected.to_str().expect("the current directory is UTF-8")
    );
}

#[test]
fn a_deeply_nested_path_beyond_the_first_attempt_still_resolves() {
    // Forces the grow-the-buffer retry rather than assuming it works from
    // paths that always fitted the first attempt.
    let segment = "a-directory-with-a-deliberately-long-name";
    let mut path = String::from(r"C:\");
    for index in 0..12 {
        path.push_str(&format!("{segment}-{index:02}\\"));
    }
    path.push_str("file.txt");

    let resolved = resolve(&path);

    assert!(
        resolved.encode_utf16().count() > 260,
        "the fixture must exceed the first attempt: {} UTF-16 units",
        resolved.encode_utf16().count()
    );
    assert!(resolved.ends_with("file.txt"), "unexpected: {resolved}");
}

#[test]
fn an_empty_path_reports_the_raw_code() {
    let outcome = ResolveFullPath::new(Wtf16String::new())
        .expect("an empty path has no NUL")
        .perform();

    // Specifically the Win32 variant, not merely "an error". The point of the
    // split is that a caller can tell a refusal by Windows from this crate's own
    // retry giving up; asserting only `is_err` would pass just as happily if
    // every failure collapsed back into one shape.
    assert!(
        matches!(outcome, Err(FullPathError::Win32(_))),
        "an empty path names nothing, and Windows says so rather than this crate"
    );
}

#[test]
fn the_two_failures_are_distinguishable() {
    // The defect this replaced: the unstable case returned a synthesized
    // ERROR_INSUFFICIENT_BUFFER, a code Win32 also returns on its own, so these
    // two values were indistinguishable to a caller matching on the error.
    let refused = FullPathError::from(Win32Error::from_code(ERROR_INSUFFICIENT_BUFFER));
    let gave_up = FullPathError::Unstable {
        attempts: super::MAX_ATTEMPTS,
    };

    assert!(matches!(refused, FullPathError::Win32(_)));
    assert!(matches!(gave_up, FullPathError::Unstable { .. }));

    // And they do not read the same either, so a log distinguishes them too.
    assert_ne!(refused.to_string(), gave_up.to_string());
}

#[test]
fn an_unstable_failure_reports_the_attempts_it_made() {
    let error = FullPathError::Unstable {
        attempts: super::MAX_ATTEMPTS,
    };

    assert!(
        error.to_string().contains(&super::MAX_ATTEMPTS.to_string()),
        "the attempt count is the one diagnostic this variant carries: {error}"
    );
}

#[test]
fn only_a_win32_failure_carries_a_source() {
    use std::error::Error;

    // `Unstable` is this crate's own conclusion rather than something Windows
    // reported, so it has no underlying error to chain to.
    let refused = FullPathError::from(Win32Error::from_code(ERROR_INSUFFICIENT_BUFFER));
    let gave_up = FullPathError::Unstable {
        attempts: super::MAX_ATTEMPTS,
    };

    assert!(refused.source().is_some());
    assert!(gave_up.source().is_none());
}

#[test]
fn the_request_reports_the_path_it_was_built_from() {
    let request =
        ResolveFullPath::new(Wtf16String::from(r"C:\Windows")).expect("the path has no NUL");

    assert_eq!(request.path().to_string_lossy(), r"C:\Windows");
}

#[test]
fn an_interior_nul_is_refused_where_the_request_is_built() {
    // Win32 reads a path up to its first NUL, so the call would resolve a shorter path than the one
    // named and report success. A NUL anywhere in the content is refused, and the string's own
    // terminator is not content.
    for units in [
        &[0x0043, 0x003A, 0x005C, 0x0000, 0x0061][..],
        &[0x0000, 0x0061][..],
        &[0x0061, 0x0000][..],
        &[0x0000][..],
    ] {
        let error =
            ResolveFullPath::new(Wtf16String::from_units(units)).expect_err("a NUL truncates");
        assert!(matches!(error, PathCaptureError::InteriorNul), "{units:?}");
    }

    assert!(ResolveFullPath::new(Wtf16String::new()).is_ok());
    assert!(ResolveFullPath::new(Wtf16String::from(r"C:\a")).is_ok());
}

#[test]
fn a_request_is_cloneable_and_resolves_the_same_way() {
    // Unlike the handle-taking entries, this one owns no handle, so an
    // infallible Clone is honest here.
    let request = ResolveFullPath::new(Wtf16String::from(r"C:\Windows\.\System32"))
        .expect("the path has no NUL");
    let clone = request.clone();

    assert_eq!(
        clone
            .perform()
            .expect("the clone resolves")
            .to_string_lossy(),
        request
            .perform()
            .expect("the original resolves")
            .to_string_lossy()
    );
}

#[test]
fn a_resolution_performs_the_same_way_on_another_thread() {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}

    assert_send::<ResolveFullPath>();
    assert_sync::<ResolveFullPath>();

    let request = ResolveFullPath::new(Wtf16String::from(r"C:\Windows\.\System32"))
        .expect("the path has no NUL");

    let resolved = std::thread::spawn(move || {
        request
            .perform()
            .expect("resolve on a worker")
            .to_string_lossy()
    })
    .join()
    .expect("the worker did not panic");

    assert_eq!(resolved, r"C:\Windows\System32");
}

/// The current directory this process is running in, as `GetFullPathNameW`
/// would use it.
///
/// Read through the same API rather than `std::env::current_dir`, because the
/// two can disagree: `std` normalizes, and what these tests need is exactly the
/// string the call under test roots against.
fn current_directory() -> String {
    // A lone `.` is rooted at the current directory and then collapses to it.
    resolve(".")
}

#[test]
fn a_relative_path_is_rooted_at_the_process_current_directory() {
    // The rooting half of the documented contract. Asserted as a RELATION to
    // the current directory rather than against a literal, so it holds on any
    // machine and in any working directory.
    let expected = format!(r"{}\rel.txt", current_directory().trim_end_matches('\\'));
    assert_eq!(resolve("rel.txt"), expected);
}

#[test]
fn a_root_relative_path_takes_the_root_and_not_the_whole_directory() {
    // `\foo` is documented as taking only the ROOT of the current directory,
    // which is what distinguishes it from an ordinary relative path. Calling it
    // "the current drive" was wrong -- under a UNC current directory there is
    // no drive at all -- so this pins the property the doc actually claims.
    let cwd = current_directory();
    let resolved = resolve(r"\foo");

    // Computed, not inferred. An earlier version of this test derived the root
    // by trimming the result it was checking, and guarded its one distinguishing
    // assertion behind a length comparison -- so with the current directory at a
    // drive root, where a root-relative path and a relative one coincide, the
    // guard was false and the test passed having asserted almost nothing. A
    // check whose strength depends on where it runs is the vacuous pass this
    // repository keeps paying for.
    let root = root_of(&cwd);
    assert_eq!(
        resolved,
        format!(r"{root}foo"),
        "a root-relative path is rooted at the root of the current directory \
         ({cwd}, root {root}), and carries none of its subtree"
    );
}

/// The root of an absolute Windows path, including its trailing separator.
///
/// C:\a\b gives C:\, and \\server\share\a gives \\server\share\ -- which
/// is why this returns a *root* rather than a drive: under a UNC current
/// directory there is no drive letter to return.
fn root_of(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\") {
        // server\share, then everything after it.
        let mut parts = rest.splitn(3, '\\');
        let server = parts.next().unwrap_or_default();
        let share = parts.next().unwrap_or_default();
        return format!(r"\\{server}\{share}\");
    }
    // Stated rather than assumed: the only caller passes a resolved absolute
    // path, which is always at least `C:\`, but `split_at` would panic on an
    // index rather than say why.
    assert!(
        path.len() >= 2 && path.is_char_boundary(2),
        "root_of expects an absolute path, got {path:?}"
    );
    let (drive, _) = path.split_at(2);
    format!(r"{drive}\")
}

#[test]
fn a_legacy_device_name_short_circuits_rooting() {
    // The exception to "roots a path that is not fully qualified", and the one
    // a caller passing an untrusted name has to know about: these do not become
    // files under the current directory.
    //
    // **Asserted as the complete mapping, not as "somewhere in the device
    // namespace".** `starts_with(r"\\.\")`, which this used, would pass if
    // `CON` resolved to `\\.\NUL` -- and the contract names which device each
    // spelling reaches, so a test binding only to the prefix does not bind to
    // the contract. Measured: the name is carried through unchanged.
    for name in [
        "CON", "NUL", "PRN", "AUX", "CONIN$", "CONOUT$", "COM1", "LPT9",
    ] {
        assert_eq!(
            resolve(name),
            format!(r"\\.\{name}"),
            "{name} reaches ITS device, not merely some device"
        );
    }
}

#[test]
fn the_device_form_accepts_trailing_colons_dots_spaces_and_any_casing() {
    // Every spelling the module doc claims reaches a device. A filter written
    // from a narrower reading of the rule would let these through.
    //
    // The full result again rather than the prefix, which also pins what the
    // device form does to the spelling: trailing colons, dots and spaces are
    // dropped, and case is carried through untouched.
    for (spelling, expected) in [
        ("CON", r"\\.\CON"),
        ("CON:", r"\\.\CON"),
        ("CON::", r"\\.\CON"),
        ("CON.", r"\\.\CON"),
        ("CON ", r"\\.\CON"),
        ("con", r"\\.\con"),
        ("cOn:", r"\\.\cOn"),
    ] {
        assert_eq!(
            resolve(spelling),
            expected,
            "{spelling:?} is a device spelling, and reaches this exact device"
        );
    }
}

#[test]
fn superscript_digits_are_device_names_too() {
    // The members a hand-written denylist omits, and which this crate's own
    // documentation asserted did not exist until a review measured them. If a
    // future Windows build stops accepting them this test says so, which is the
    // whole reason it is here rather than left as prose.
    for spelling in [
        "COM\u{00b9}",
        "COM\u{00b2}",
        "COM\u{00b3}",
        "LPT\u{00b9}",
        "LPT\u{00b2}",
        "LPT\u{00b3}",
    ] {
        let resolved = resolve(spelling);
        assert!(
            resolved.starts_with(r"\\.\"),
            "{spelling:?} uses a superscript digit and still names a device: {resolved}"
        );
    }
}

#[test]
fn a_device_name_with_anything_around_it_is_rooted_normally() {
    // The other half, and the one that keeps the rule from being read as "any
    // input containing a device name". Without these the test above would pass
    // just as well against an implementation that mapped far too much.
    for spelling in [
        "CON.txt", r"a\CON", r".\CON", "CON:x", "COM0", "COM10", "CONIN",
    ] {
        let resolved = resolve(spelling);
        assert!(
            !resolved.starts_with(r"\\.\"),
            "{spelling:?} is not a bare device name, so it must be rooted: {resolved}"
        );
    }
}

#[test]
fn a_fully_qualified_path_is_unaffected_by_the_current_directory() {
    // The lexical half, stated as the invariance the rooting half lacks: this
    // is what makes "not lexical as a whole" a claim about the OTHER half only.
    // `C:\a` need not exist, which is the same fact the existence test pins.
    assert_eq!(resolve(r"C:\a\..\b"), r"C:\b");
    assert_eq!(resolve("C:/a/b//c"), r"C:\a\b\c");
}

#[test]
fn trailing_dot_and_space_trimming_differs_between_final_and_intermediate_components() {
    // The module doc says the rewrite trims trailing dots and spaces. Until now
    // that was only exercised through a final `.` component (which is the
    // separate `.`-collapsing rule) and through device spellings (which take
    // the short-circuit and never reach the ordinary path). Neither pins this.
    //
    // **An earlier version of this test generalised the rule, and the
    // generalisation is false.** It carried one intermediate case, `C:\a.\b` ->
    // `C:\a\b`, under the comment "trimming applies per component, not only at
    // the end of the path". Measured, a component's position decides what it
    // loses:
    //
    //   final:        C:\name...   -> C:\name     (a RUN of dots goes)
    //                 C:\name      -> C:\name     (trailing spaces go)
    //   intermediate: C:\a.\b      -> C:\a\b      (ONE trailing dot goes)
    //                 C:\a.b.\c    -> C:\a.b\c    (inner dots are not special)
    //                 C:\a...\b    -> unchanged   (a run does NOT go)
    //                 C:\a \b      -> unchanged   (a space does NOT go)
    //                 C:\a. \b     -> unchanged
    //
    // **The preservation cases are the load-bearing half**, and their absence
    // is what let the wrong generalisation stand: with only the transforming
    // inputs, an implementation trimming every component's trailing dots and
    // spaces satisfies the entire test while being wrong about three of the
    // five intermediate spellings. A reviewer demonstrated exactly that against
    // the previous seven assertions.
    //
    // Every case measured before being written down -- which was also claimed
    // last time, and was true of the cases present. What was not measured was
    // the sentence generalising them.
    for (input, expected) in [
        // The final component loses any run of trailing dots and spaces.
        (r"C:\name.", r"C:\name"),
        (r"C:\name ", r"C:\name"),
        (r"C:\name...", r"C:\name"),
        (r"C:\name   ", r"C:\name"),
        (r"C:\name. ", r"C:\name"),
        // An extension is not special: the trailing dot goes, the rest stays.
        (r"C:\name.txt.", r"C:\name.txt"),
        // An intermediate component loses a single trailing dot ...
        (r"C:\a.\b", r"C:\a\b"),
        (r"C:\a.b.\c", r"C:\a.b\c"),
        // ... and nothing else. These are the cases that fail the broad rule.
        (r"C:\a...\b", r"C:\a...\b"),
        (r"C:\a \b", r"C:\a \b"),
        (r"C:\a. \b", r"C:\a. \b"),
        (r"C:\a b \c", r"C:\a b \c"),
    ] {
        assert_eq!(resolve(input), expected, "trimming {input:?}");
    }
}

#[test]
fn a_name_containing_a_device_word_is_rooted_under_the_current_directory() {
    // Strengthens the device-negative control. Asserting only "not `\\.\`" is
    // too weak: an implementation that returned every relative input unchanged
    // would satisfy it while rooting nothing. These assert the full resolved
    // path, so the rooting guarantee is actually covered.
    //
    let base = current_directory();
    let base = base.trim_end_matches('\\');
    // `CON:x` is here because it was NOT, and the negative test alone cannot
    // carry it: that test asserts only `!starts_with("\\\\.\\")`, which the
    // unrooted literal `CON:x` satisfies. So the one spelling whose rooting is
    // least obvious -- a device word followed by a colon, where `CON:` itself
    // IS a device -- was the one nothing pinned. Measured: `<cwd>\CON:x`, with
    // the colon carried through untouched.
    for name in ["CON.txt", "CONIN", "COM0", "COM10", r"a\CON", "CON:x"] {
        assert_eq!(
            resolve(name),
            format!(r"{base}\{name}"),
            "{name:?} is not a bare device name, so it roots under the current \
             directory rather than merely avoiding the device namespace"
        );
    }

    // `.\CON` is asserted separately rather than excluded from the loop, which
    // is what an earlier version did on the grounds that the `.` collapses and
    // the expected form would need a special case. It needs one, so it gets
    // one: with the case left to the device-NEGATIVE test alone, the only claim
    // made about `.\CON` was that it does not start with `\\.\` -- and an
    // implementation returning it unchanged satisfies that, which is precisely
    // the too-weak assertion this test exists to strengthen.
    assert_eq!(
        resolve(r".\CON"),
        format!(r"{base}\CON"),
        "the `.` collapses and the result roots under the current directory, so \
         a leading `.\\` is enough to take the name out of the device \
         short-circuit without taking it out of ordinary rooting"
    );

    // The root-relative form, which the module doc states and nothing pinned.
    // Its expectation is the current directory's ROOT rather than `base`, so it
    // cannot join the loop above.
    let root = root_of(&current_directory());
    assert_eq!(
        resolve(r"\CON"),
        format!("{root}CON"),
        "a leading separator roots the device word at the current directory's \
         root instead of reaching the device"
    );
}

#[test]
fn nul_is_the_one_device_word_a_path_around_it_does_not_save() {
    // **The exception to the test above, and it was found by trying to write
    // the general rule.** A review asked for the `\CON` case on the grounds
    // that the doc states it; the doc stated it of the whole device SET, having
    // been written from `CON` alone. Measured across all eight accepted names,
    // seven root normally once anything precedes them and `NUL` does not --
    // `NUL` short-circuits as the final component of any path at all.
    //
    // Had the requested assertion been written as the general rule it was
    // phrased as ("a root-relative device word roots at the root"), it would
    // have pinned a false claim, which is the failure this branch exists to
    // stop rather than repeat.
    let base = current_directory();
    let base = base.trim_end_matches('\\');
    let root = root_of(&current_directory());

    // The seven that behave as the doc says, in the form that separates them.
    for name in ["CON", "PRN", "AUX", "CONIN$", "CONOUT$", "COM1", "LPT1"] {
        assert_eq!(
            resolve(&format!(r".\{name}")),
            format!(r"{base}\{name}"),
            "{name:?} stops being a device once a path precedes it"
        );
        assert_eq!(
            resolve(&format!(r"C:\{name}")),
            format!(r"C:\{name}"),
            "{name:?} is an ordinary component of a fully-qualified path"
        );
    }

    // And `NUL`, which does not -- INCLUDING from a fully-qualified path, so a
    // rooted result is not by itself evidence that a path names a file.
    for input in [r"\NUL", r".\NUL", r"a\NUL", r"C:\NUL"] {
        assert_eq!(
            resolve(input),
            r"\\.\NUL",
            "{input:?} still reaches the device: NUL is not saved by a path in \
             front of it, where every other device word is"
        );
    }

    // A suffix is what takes it out, which is the boundary of the exception --
    // and the doc names both spellings, so both are asserted. The root-relative
    // form alone would leave the bare ones uncovered, which is how `CON:x` came
    // to be pinned by a predicate the wrong answer satisfied.
    assert_eq!(
        resolve(r"\NUL.txt"),
        format!("{root}NUL.txt"),
        "an extension takes even NUL out of the device namespace"
    );
    for (input, expected) in [("NUL.txt", "NUL.txt"), ("NUL:x", "NUL:x")] {
        assert_eq!(
            resolve(input),
            format!(r"{base}\{expected}"),
            "{input:?} is a suffixed name, so it roots under the current \
             directory like any other"
        );
    }

    // And the boundary is a suffix, not merely "more characters": a trailing
    // colon or space still reaches the device, exactly as for `CON`. Without
    // these the rule above would read as "anything after NUL saves it".
    for input in ["NUL::", "NUL "] {
        assert_eq!(
            resolve(input),
            r"\\.\NUL",
            "{input:?} is still the device: trailing colons and spaces are part \
             of the device form, not a suffix that escapes it"
        );
    }
}
