// Copyright (c) Mike Grier.

//! Root-relative rooting under a UNC current directory.
//!
//! An integration test, and a two-process one, because the property is not
//! expressible any other way. The current directory is **process-wide**, so a
//! test that wants to observe what `GetFullPathNameW` does under a UNC one has
//! to *be* a process that has a UNC one -- and it must not impose that on the
//! rest of the suite, which runs as threads in the same process.
//!
//! What it pins is the branch the module doc calls out and nothing else
//! reached: a root-relative path (`\foo`) is rooted at the **root of the
//! current directory**, which under a UNC current directory is the share root
//! `\\server\share\` and not a drive. The unit test for the same rule runs
//! under whatever directory launched the suite, so on ordinary drive-rooted CI
//! it only ever exercises the `C:\foo` half; it computes the UNC expectation
//! but never puts the call in a position to produce one.
//!
//! The child is this same test binary, re-executed with a marker in its
//! environment and a filter naming the one test to run. That keeps the
//! assertion beside the thing it asserts rather than in a fixture binary whose
//! drift nothing would catch.

use windows_namespace_request_sys::ResolveFullPath;
use wtf_string::Wtf16String;

/// Set on the child, absent on the parent, and carrying the directory the
/// parent placed it in.
///
/// **The value is load-bearing, and an earlier version ignored it.** Presence
/// alone was the signal, so a variable of this name already in the environment
/// -- a leftover from debugging the child by hand, say -- made the PARENT take
/// the child branch, in the ordinary drive-rooted directory cargo starts it in.
/// The test then failed on the UNC precondition, blaming the crate for an
/// environment collision.
///
/// Carrying the intended directory fixes that and buys a real check besides:
/// the child now confirms it is where the parent put it, rather than inferring
/// it from the shape of whatever directory it happens to hold.
const CHILD_MARKER: &str = "WNRS_UNC_CURRENT_DIRECTORY_CHILD";

/// The administrative share, which is the only UNC path a test can rely on
/// existing without creating one -- and creating one needs privileges the suite
/// must not assume it has.
const UNC_ROOT: &str = r"\\localhost\C$";

/// The child runs in a SUBDIRECTORY of the share, not at its root, and the
/// distinction is the whole test.
///
/// **At the share root the two rooting rules coincide**, so the first version of
/// this test could not tell them apart. Rooting `\foo` at the *root of* the
/// current directory and rooting it at the *whole* current directory both give
/// `\\localhost\C$\foo` when the current directory IS the root -- which is the
/// exact distinction the module doc cites this test as pinning. Measured one
/// level down, they separate: `\foo` gives `\\localhost\C$\foo` while `foo`
/// gives `\\localhost\C$\Windows\foo`.
///
/// Candidates rather than one name, because the share is `C:` and Windows need
/// not be installed there. Any enterable subdirectory works; these three are
/// the ones a Windows system drive has.
const UNC_SUBDIRECTORY_CANDIDATES: &[&str] = &["Windows", "Users", "ProgramData"];

const TEST_NAME: &str = "a_root_relative_path_takes_the_share_root_under_a_unc_current_directory";

// Ignored by default and run explicitly in CI, which is this repository's
// existing "ignored tier" pattern rather than a new one.
//
// `C$` is an ADMINISTRATIVE share. It is reachable for a member of
// Administrators and not for an ordinary user, and it can be switched off
// entirely -- so running by default would turn `cargo test` red for a
// non-administrator developer on a perfectly good host, reporting an
// environment limitation as a defect in the crate. Provisioning a share of our
// own is not an escape: creating one needs the same privileges.
//
// The alternative -- skipping quietly when the share is missing -- is the one
// thing that must not happen, because this is the ONLY coverage of the UNC
// branch and a silent skip would leave it unpinned while reporting otherwise.
// `#[ignore]` keeps that visible: an ignored test is counted and named in the
// output, where a skip inside a passing test is not.
#[test]
#[ignore = "needs a reachable administrative share for a UNC current directory; run in CI with --include-ignored"]
fn a_root_relative_path_takes_the_share_root_under_a_unc_current_directory() {
    if let Some(placed_at) = std::env::var_os(CHILD_MARKER) {
        let placed_at = placed_at
            .to_str()
            .expect("the parent passes a UTF-8 directory")
            .to_owned();
        assert_root_relative_takes_the_share_root(&placed_at);
        return;
    }

    // The precondition is asserted rather than silently skipped. A test that
    // quietly passes when it could not run is the vacuity this crate keeps
    // finding, and it is worse here than elsewhere: this is the only coverage
    // of the branch, so a silent skip would leave the doc unpinned while
    // reporting that it is pinned.
    assert!(
        std::path::Path::new(UNC_ROOT).is_dir(),
        "{UNC_ROOT} is not reachable, so no UNC current directory can be \
         established -- an environment limitation, not a failure of the \
         behaviour under test"
    );

    let working_directory = UNC_SUBDIRECTORY_CANDIDATES
        .iter()
        .map(|name| format!(r"{UNC_ROOT}\{name}"))
        .find(|path| std::path::Path::new(path).is_dir())
        .unwrap_or_else(|| {
            panic!(
                "none of {UNC_SUBDIRECTORY_CANDIDATES:?} exists under {UNC_ROOT}, \
                 so the child cannot be placed BELOW the share root -- and at the \
                 root the two rooting rules give the same answer, which is what \
                 this test exists to separate"
            )
        });

    let exe = std::env::current_exe().expect("the test binary knows its own path");
    let status = std::process::Command::new(exe)
        .arg("--exact")
        .arg(TEST_NAME)
        .arg("--nocapture")
        // Without this the child runs zero tests and exits 0 -- the parent
        // would report success having measured nothing at all. The test it is
        // told to run is the ignored one, so the filter alone is not enough.
        .arg("--include-ignored")
        .env(CHILD_MARKER, &working_directory)
        .current_dir(&working_directory)
        .status()
        .expect("re-execute this test binary with a UNC current directory");

    assert!(
        status.success(),
        "the child, running with {working_directory} as its current directory, \
         did not pass: {status}"
    );
}

fn assert_root_relative_takes_the_share_root(placed_at: &str) {
    let cwd = std::env::current_dir().expect("the child has a current directory");
    let cwd = cwd.to_str().expect("the current directory is UTF-8");

    // Proving the child is where the parent put it, against the parent's own
    // record rather than against the shape of the directory. Two things can go
    // wrong and this separates them: a process can LOSE a UNC current directory
    // (`cmd.exe` refuses one and falls back to the Windows directory), and this
    // branch can be entered by something that is not the parent's child at all,
    // when `CHILD_MARKER` is already set in the environment.
    assert!(
        cwd.eq_ignore_ascii_case(placed_at.trim_end_matches('\\')),
        "the child is at {cwd}, not the {placed_at} the parent chose. If no \
         parent placed it there, {CHILD_MARKER} is set in this environment and \
         should be unset -- that is a collision, not a defect in the crate"
    );

    assert!(
        cwd.starts_with(r"\\"),
        "precondition: the child must actually hold a UNC current directory, \
         not a drive-rooted one: {cwd}"
    );

    // **And below the share root**, which the first version of this test did
    // not check because it did not need to -- it ran AT the root, where the two
    // rules being separated give the same answer. This assertion is the one
    // that keeps the test from going vacuous again if the parent's directory
    // choice ever regresses.
    assert_ne!(
        cwd.trim_end_matches('\\'),
        UNC_ROOT,
        "precondition: the child must be BELOW the share root. At the root, \
         rooting at the root of the current directory and rooting at the whole \
         current directory agree, so neither assertion below can tell them apart"
    );

    let resolved = ResolveFullPath::new(Wtf16String::from(r"\foo"))
        .expect("the path has no NUL")
        .perform()
        .expect("a root-relative path resolves");

    // **Compared case-sensitively on purpose, and a review read that as a
    // portability bug.** The concern was that a host recording the share as
    // `LOCALHOST` would fail this equality against the lowercase literal. It
    // cannot, because both sides descend from `UNC_ROOT`: the parent builds the
    // child's working directory from it, and the root this call returns is the
    // root of that directory. Measured -- a process started with
    // `\\LOCALHOST\C$\Windows` reports exactly that back from
    // `current_dir()`, and likewise for `\\localhost\c$\Windows`, so Windows
    // preserves the spelling it was given rather than canonicalising it.
    //
    // The precondition above is case-INSENSITIVE for a different reason: it
    // compares against a value that may have crossed a process boundary by a
    // route this test does not control, so it accepts what a Windows path
    // comparison would.
    assert_eq!(
        resolved.to_string_lossy(),
        format!(r"{UNC_ROOT}\foo"),
        "a root-relative path takes the ROOT of the current directory, which \
         under a UNC current directory is the share root -- so \"the current \
         drive\", as a draft of the module doc said, has no referent here"
    );

    // The contrast, and the reason the assertion above means anything. An
    // ordinary relative path takes the WHOLE current directory, so the two
    // resolutions differ by everything below the share root. Without this, a
    // resolver that ignored the leading separator entirely would still satisfy
    // the equality above whenever the child happened to sit at the root.
    let relative = ResolveFullPath::new(Wtf16String::from("foo"))
        .expect("the path has no NUL")
        .perform()
        .expect("a relative path resolves");

    assert_eq!(
        relative.to_string_lossy(),
        format!(r"{cwd}\foo"),
        "an ordinary relative path takes the whole current directory, not its \
         root -- the two rules are distinguishable here and identical at the \
         share root"
    );
}
