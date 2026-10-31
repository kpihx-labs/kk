// Copyright (c) Mike Grier.
// Split from tests.rs at 3e48630.

//! The per-drive current-directory (`=X:`) arm, and the fixture that controls
//! it.
//!
//! Separated from the parent because it is the one cluster here with a fixture
//! of its own: a probe directory, a drive-letter allocator, a reader and writer
//! for the hidden entries, and a guard that puts a borrowed one back. Every
//! test below either sets that state or is about what the call does with it,
//! and nothing outside this module needs any of it.
//!
//! The two drive-relative ROOTING tests moved with the fixture rather than
//! staying beside the other rooting forms. That is a privacy constraint, not a
//! preference: a child module can see its ancestors' private items, but a
//! parent cannot see its child's, so a test left in the parent could not reach
//! the helpers here. They are also the two tests that are about this arm, so
//! the constraint and the responsibility agree.

use super::*;

#[test]
fn a_drive_relative_path_carries_its_component_and_the_current_drive_uses_the_process_directory() {
    // **The name says what the two assertions reach, and an earlier one did
    // not.** This was
    // `a_drive_relative_path_is_rooted_at_that_drive_and_not_the_process_directory`,
    // which claims more than anything here shows, in two separate ways. The
    // other-drive assertion is `ends_with("\\foo")`, which accepts ANY base
    // including the process directory -- so it cannot say "not the process
    // directory". And "rooted at that drive" is not even true in general: an
    // accepted entry is used verbatim and may name a directory on a different
    // drive entirely, which is the sibling test's whole point.
    //
    // The body already said it only bounds the arm. The name did not, and the
    // name is what a reader takes away -- the same defect as the test called
    // `..._is_neither_consulted_nor_rewritten` before it was renamed.
    //
    // The third rooting form, with two arms, which is the part that gets
    // missed:
    //
    //   * For a drive OTHER than the current one, Windows reads the hidden
    //     `=X:` entry recorded for it.
    //   * For the CURRENT drive the entry makes no difference to the result and
    //     the process current directory wins. Measured: setting `=Q:` while the
    //     process is on `Q:` changes nothing.
    //
    // **This test does not mutate `=X:`, but the call it exercises may.**
    // Measured: resolving `X:foo` for a non-current drive checks that drive's
    // entry and WRITES it to `X:\` when the entry is absent or rejected. An
    // accepted entry is left alone, and the current-drive form writes nothing
    // -- so this is not "every resolution", but it does mean an ordinary host
    // with no entry has one written merely by running this test. That is
    // a property of the call, documented in the module doc; it is noted here so
    // the next reader does not take "reads process state" at face value, as
    // four revisions of that doc did.
    //
    // **This test only BOUNDS the other-drive arm**, because it does not control
    // the entry: with no `=X:` set, an implementation that always used the
    // drive root would satisfy everything here. The arm is pinned properly by
    // `a_drive_relative_path_uses_that_drives_entry_verbatim_and_rewrites_a_bad_one`,
    // which sets the entry and draws its letter from a candidate list disjoint
    // from this one's, so the two cannot race.
    let cwd = current_directory();
    let cwd_drive = cwd.chars().next().filter(char::is_ascii_alphabetic);

    // The other-drive arm needs no drive letter from the current directory --
    // under a UNC current directory every letter is "other" -- so it runs
    // unconditionally and this test never degenerates to a silent skip.
    let other = probe_drive_from(probe_drives::ROOTED_AT_THAT_DRIVE, None);

    // This test controls no entry, but the CALL does: resolving for a
    // non-current drive writes `=X:` whenever the recorded entry is absent or
    // rejected, and on most hosts a letter chosen for being unused has no entry
    // at all. So merely observing the arm mutates process-global state, and
    // this was the one mutating case here without a guard -- the borrow is
    // needed exactly because the mutation is not the test's own doing.
    let _restore = BorrowedDriveEntry::take(other);
    let resolved = resolve(&format!("{other}:foo"));

    // Only what is invariant without controlling the entry. An earlier version
    // required the result to start with `X:\`, which the verbatim rule breaks;
    // its replacement compared against the current directory, which `other`
    // differs from by construction, so it could fire only if the entry happened
    // to equal the process directory exactly -- the same vacuity, respelled.
    // What survives every entry value is that the component is carried through.
    assert!(
        resolved.ends_with(r"\foo"),
        "the component is carried through whatever the entry holds: {resolved}"
    );

    // The current-drive arm, where the process directory wins over any `=X:`.
    // Only expressible when the current directory has a drive letter at all.
    if let Some(drive) = cwd_drive {
        assert_eq!(
            resolve(&format!("{drive}:foo")),
            format!(r"{}\foo", cwd.trim_end_matches('\\')),
            "on the current drive, the per-drive entry makes no difference to \
             the result and the process directory is used"
        );
    }
}

#[test]
fn the_current_drives_entry_does_not_affect_resolution_and_is_not_rewritten() {
    // The module doc states both halves of the current-drive arm as fact. Until
    // now nothing pinned either, and the assertion just above -- which looks
    // like it does -- cannot: it resolves `X:foo` WITHOUT controlling the entry
    // and compares against the process directory, and Windows keeps the current
    // drive's entry equal to that directory. So it reads the same either way.
    // Vacuous in precisely the way this crate keeps rediscovering, and the
    // reason the two arms need opposite fixtures: the sibling tests must AVOID
    // the current drive, and this one must be on it.
    //
    // **The name says what is observable, and an earlier one did not.** This
    // was `..._is_neither_consulted_nor_rewritten`, which claims the entry is
    // not READ -- and installing a value and watching the outcome cannot
    // separate "not read" from "read and ignored". That is the same overreach
    // this branch removed from the probe's "without consulting a device", and
    // the test correcting it committed it in its own name. What the two
    // assertions below reach is the pair of observable effects: the entry makes
    // no difference to the result, and it is not written back.
    let cwd = current_directory();
    let Some(drive) = cwd.chars().next().filter(char::is_ascii_alphabetic) else {
        // A UNC current directory has no drive letter, so there is no
        // current-drive arm to exercise. Not a skip of something testable.
        return;
    };
    let process_directory = format!(r"{}\foo", cwd.trim_end_matches('\\'));

    let probe_dir = probe_directory("current-drive");
    let probe = probe_dir.path.to_str().expect("the probe path is UTF-8");
    let _restore = BorrowedDriveEntry::take(drive);

    // The anti-vacuity check, made permanent rather than performed once by
    // hand: unless the two arms would give DIFFERENT answers, every assertion
    // below passes without distinguishing them, which is the failure this test
    // was written to correct.
    assert_ne!(
        process_directory,
        format!(r"{probe}\foo"),
        "precondition: the entry must name somewhere other than the process \
         directory, or honouring it and ignoring it look identical"
    );

    // No difference to the result. The entry is one the OTHER arm would honour
    // verbatim -- an existing directory in canonical form -- and it names
    // somewhere the process directory cannot be, because this test just created
    // it under a process-unique name. If the entry were HONOURED, the result
    // would be under `probe`.
    set_drive_entry(drive, Some(probe));
    assert_eq!(
        resolve(&format!("{drive}:foo")),
        process_directory,
        "the current drive's entry was set to {probe}, an entry the non-current \
         arm honours verbatim, and the process directory won anyway"
    );

    // Not rewritten, which needs a REJECTED entry to be visible: an accepted one
    // is left alone on both arms, so leaving it alone shows nothing. A child of
    // the probe directory cannot exist, and on the non-current arm that is
    // replaced by the drive root.
    let missing = probe_dir.path.join("no-such-child");
    let missing = missing.to_str().expect("the probe path is UTF-8");
    set_drive_entry(drive, Some(missing));
    let _ = resolve(&format!("{drive}:foo"));
    assert_eq!(
        drive_entry(drive).map(|v| v.to_string_lossy()).as_deref(),
        Some(missing),
        "an entry the non-current arm would have replaced with {drive}:\\ is \
         left untouched on the current drive"
    );
}

/// A directory that exists, is in canonical `X:\...` form, and is neither a
/// drive root nor the process current directory.
///
/// **The form is enforced, not assumed, and that distinction has now cost two
/// rounds.** An earlier version derived the probe from `current_directory()`,
/// which yields `C:` at a drive root -- drive-relative, not a directory -- and
/// both tests failed there. Its replacement used `std::env::temp_dir()`, which
/// is `%TMP%`/`%TEMP%` verbatim and carries no guarantee of a drive letter: with
/// temp redirected to a share, the probe is a UNC path, which
/// `GetFullPathNameW` rejects as an entry *on shape* -- the very rule the
/// caller is trying to pin. Folder redirection makes that an ordinary
/// configuration, not a contrived one.
///
/// So the temp directory is used only when it is drive-rooted, and otherwise
/// the fallback is `%SystemRoot%`, which is guaranteed to exist, to be
/// canonical, and not to be a drive root. Nothing is created in the fallback
/// case, so `created` records whether there is anything to remove.
///
/// Removal is a [`Drop`], matching this crate's own `Fixture` in
/// [`crate::handle`]'s tests. An earlier version cleaned up with a statement at
/// the end of each test and argued that a guard writing during unwinding could
/// panic and abort -- which is true of the `=X:` entry restore beside it, and
/// not of removing a directory. Conflating the two left a directory behind
/// after every failing assertion.
struct ProbeDir {
    path: std::path::PathBuf,
    created: bool,

    /// Held for the fixture's whole lifetime, because this fixture opens
    /// handles and this suite serialises that.
    ///
    /// `handle_allocation()`'s read guard means "I may open handles"; its write
    /// guard means "no other test may, while I reason about a specific handle
    /// value". Eleven tests across four modules take the write guard to assert
    /// things like a closed handle's value not being reused, and any test
    /// allocating a handle beside them can make those assertions fail for a
    /// reason that has nothing to do with what they pin.
    ///
    /// This fixture allocates: `create_dir` and `remove_dir` here, and the
    /// `Path::exists` / `is_file` preconditions its tests run while it is
    /// alive -- `std`'s Windows metadata goes through `CreateFileW`. Holding
    /// the guard on the fixture covers all of them for the whole test, which a
    /// guard taken at each call site would not.
    ///
    /// A test holding one of these must not take a second read guard: `std`
    /// does not promise recursive read locking is deadlock-free.
    _allocating: std::sync::RwLockReadGuard<'static, ()>,
}

impl ProbeDir {
    /// The drive letter this probe lives on, if it has one.
    ///
    /// The caller needs it to pick a probe drive that is *not* this one:
    /// resolving `W:foo` against an entry naming a directory that is itself on
    /// `W:` cannot demonstrate that an accepted entry is used verbatim across
    /// drives, which is the property being pinned.
    fn drive(&self) -> Option<char> {
        self.path
            .as_os_str()
            .to_string_lossy()
            .chars()
            .next()
            .filter(char::is_ascii_alphabetic)
    }
}

impl Drop for ProbeDir {
    fn drop(&mut self) {
        if self.created {
            let _ = std::fs::remove_dir(&self.path);
        }
    }
}

fn probe_directory(tag: &str) -> ProbeDir {
    // Taken before the first filesystem call, and handed to the fixture so it
    // outlives this function. See `ProbeDir::_allocating`.
    let _allocating = handle_allocation()
        .read()
        .expect("the lock is not poisoned");

    // The full shape an accepted `=X:` entry must have, not just its first
    // three characters: rooted at `X:\`, with no `.` or `..` component and no
    // forward slash. `a_rejected_drive_entry_is_replaced_by_the_drive_root`
    // shows each of those spellings is rejected while naming the same existing
    // directory, so a base carrying one would turn that test's CONTROL
    // assertion -- "the same directory in canonical form is accepted" -- into a
    // rejection, and it would fail for a reason unrelated to what it pins.
    //
    // **A review read that as reachable through a non-canonical `%TMP%`. It is
    // not, and the measurement is here so the next reader need not repeat it.**
    // `std::env::temp_dir` goes through `GetTempPath2W`, which normalises what
    // it finds: `C:/Users/.../Temp`, `...\Temp\.`, `...\Temp\..\Temp`,
    // `...\Temp\\` and even the drive-relative `C:Users\...` all came back as
    // `C:\Users\...\Temp\`. The one spelling passed through verbatim is
    // `\\?\C:\...`, which is not drive-rooted and so takes the fallback below.
    //
    // The check is widened anyway. It costs nothing, it covers the fallback
    // base too, and it is the difference between a precondition that is
    // enforced and one that is argued -- which is the distinction this whole
    // change exists to hold.
    let canonical_drive_rooted = |p: &std::path::Path| {
        let s = p.as_os_str().to_string_lossy().into_owned();
        let mut chars = s.chars();
        matches!(
            (chars.next(), chars.next(), chars.next()),
            (Some(d), Some(':'), Some('\\')) if d.is_ascii_alphabetic()
        ) && !s.contains('/')
            && !s.split('\\').any(|c| c == "." || c == "..")
    };

    let temp = std::env::temp_dir();
    if canonical_drive_rooted(&temp) {
        let path = temp.join(format!("wnrs-{}-{tag}", std::process::id()));

        // `create_dir`, not `create_dir_all`, and the difference is ownership
        // rather than parents. `create_dir_all` SUCCEEDS on a directory that
        // already exists, so setting `created` after it recorded a claim this
        // fixture had not established -- and `Drop` then removes the path on
        // the strength of that claim. The name is `%TEMP%\wnrs-<pid>-<tag>`,
        // which an interrupted earlier run leaves behind and which Windows can
        // hand back to a later process when it reuses the PID. The blast radius
        // is small, because `remove_dir` refuses a non-empty directory -- but
        // "small" is not the point. Deleting something on an ownership claim
        // nothing checked is the same defect as asserting a mechanism nothing
        // measured, and this file exists to stop doing that.
        //
        // An existing directory is still perfectly usable as a probe; it is
        // just not ours to remove.
        let created = match std::fs::create_dir(&path) {
            Ok(()) => true,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
            Err(e) => panic!("create the probe directory {}: {e}", path.display()),
        };

        return ProbeDir {
            path,
            created,
            _allocating,
        };
    }

    // Not creating anything here, so no write permission is needed on a host
    // whose temp directory is redirected off a drive letter.
    //
    // **It must also differ from the process current directory**, which the
    // temp branch gets for free -- it creates a uniquely named child -- and this
    // branch does not. Cargo launched from `%SystemRoot%` on a host with a UNC
    // temp directory would otherwise hand back the current directory itself,
    // and a probe indistinguishable from the current directory cannot separate
    // "the entry was honoured" from "the entry was ignored". `System32` is the
    // second candidate for the same reason `probe_drive_from` takes a list:
    // one value that is usually right is not a guarantee.
    let system_root = std::path::PathBuf::from(
        std::env::var("SystemRoot").expect("SystemRoot is set on Windows"),
    );
    let cwd = current_directory();
    let cwd = cwd.trim_end_matches('\\');
    let distinct = |p: &std::path::Path| {
        !p.as_os_str()
            .to_string_lossy()
            .trim_end_matches('\\')
            .eq_ignore_ascii_case(cwd)
    };

    let path = [system_root.clone(), system_root.join("System32")]
        .into_iter()
        .find(|p| canonical_drive_rooted(p) && distinct(p))
        .unwrap_or_else(|| {
            panic!(
                "no fallback probe directory is both canonical and distinct \
                 from the current directory {cwd}"
            )
        });

    ProbeDir {
        path,
        created: false,
        _allocating,
    }
}
/// The candidate drive letters, one list per test that mutates a `=X:` entry.
///
/// **Centralised so the properties these tests depend on are CHECKED rather
/// than restated.** Both were previously prose -- a doc comment saying the
/// lists are disjoint, and an archived note enumerating them -- and prose
/// drifted: the archive named five lists after the sixth had been added, so a
/// reader picking letters for a seventh would have consulted an inventory
/// missing three of the eighteen letters already in use. Nothing checked
/// either claim, because nothing could: the lists were literals at six call
/// sites with no table to read.
///
/// [`the_probe_drive_candidate_lists_are_disjoint_and_large_enough`] now reads
/// this table, so adding a list that collides -- or one too short for
/// [`probe_drive_from`]'s guarantee -- fails a test instead of a review.
mod probe_drives {
    pub const ROOTED_AT_THAT_DRIVE: &[char] = &['X', 'Y', 'P'];
    pub const VERBATIM_ENTRY: &[char] = &['W', 'U', 'N'];
    pub const REJECTED_ENTRY: &[char] = &['V', 'T', 'M'];
    pub const LONG_ENTRY: &[char] = &['R', 'S', 'K'];
    pub const BORROW_GUARD: &[char] = &['G', 'H', 'J'];
    pub const EMPTY_VS_ABSENT: &[char] = &['E', 'F', 'B'];

    /// Every list above. A new list that is not added here is not covered by
    /// the disjointness test, so keep them together.
    pub const ALL: &[(&str, &[char])] = &[
        ("ROOTED_AT_THAT_DRIVE", ROOTED_AT_THAT_DRIVE),
        ("VERBATIM_ENTRY", VERBATIM_ENTRY),
        ("REJECTED_ENTRY", REJECTED_ENTRY),
        ("LONG_ENTRY", LONG_ENTRY),
        ("BORROW_GUARD", BORROW_GUARD),
        ("EMPTY_VS_ABSENT", EMPTY_VS_ABSENT),
    ];
}

#[test]
fn the_probe_drive_candidate_lists_are_disjoint_and_large_enough() {
    for (name, list) in probe_drives::ALL {
        // `probe_drive_from` excludes at most two letters -- the current drive
        // and the probe directory's drive -- so three candidates guarantee a
        // survivor. This is the premise of the panic in that function, checked
        // here rather than left to the caller as the doc comment used to.
        assert!(
            list.len() >= 3,
            "{name} has {} candidates, and at most two can be excluded, so \
             fewer than three cannot guarantee a survivor",
            list.len()
        );

        let mut seen = list.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), list.len(), "{name} repeats a letter");
    }

    for (a_name, a) in probe_drives::ALL {
        for (b_name, b) in probe_drives::ALL {
            if a_name == b_name {
                continue;
            }
            let shared: Vec<char> = a
                .iter()
                .copied()
                .filter(|c| b.iter().any(|d| d.eq_ignore_ascii_case(c)))
                .collect();
            assert!(
                shared.is_empty(),
                "{a_name} and {b_name} share {shared:?}, so the two tests can \
                 select the same drive and race under libtest's \
                 thread-per-test model"
            );
        }
    }
}

/// A drive letter to probe with, drawn from `candidates` and guaranteed to be
/// neither the current drive nor `avoid`.
///
/// **Every candidate is checked, which an earlier version did not do.** It took
/// a preferred letter and a fallback, tested only the preferred one, and
/// returned the fallback unvalidated -- so when the preferred letter was
/// excluded the caller could still be handed the current drive. Measured: with
/// the process on `U:` and `%TEMP%` on a `subst`-ed `W:`, the verbatim test
/// selected `U` and then asserted the *other-drive* contract while exercising
/// the *current-drive* arm, which is the one case where the entry makes no
/// difference to the result.
/// It failed, but the mode is worse than a failure: the helper's own doc
/// promised a guarantee it never enforced.
///
/// Three candidates against at most two exclusions, so one always survives. The
/// panic remains because that argument is about the caller's list, which this
/// function cannot see -- but the argument is no longer only an argument:
/// [`the_probe_drive_candidate_lists_are_disjoint_and_large_enough`] checks it
/// against every list in [`probe_drives`].
///
/// Callers pass disjoint lists, so no two tests can select the same letter and
/// race under libtest's thread-per-test model.
fn probe_drive_from(candidates: &[char], avoid: Option<char>) -> char {
    let cwd = current_directory();
    // A UNC current directory has no drive letter, so nothing collides there.
    let current = cwd.chars().next().filter(char::is_ascii_alphabetic);
    let taken = |c: char| {
        current.is_some_and(|d| d.eq_ignore_ascii_case(&c))
            || avoid.is_some_and(|d| d.eq_ignore_ascii_case(&c))
    };

    *candidates.iter().find(|&&c| !taken(c)).unwrap_or_else(|| {
        panic!(
            "every candidate of {candidates:?} is excluded by the current \
                 drive ({current:?}) or the probe drive ({avoid:?})"
        )
    })
}
/// Reads one of the hidden `=X:` per-drive current-directory entries.
///
/// Through Win32 rather than `std::env`, which rejects a key containing `=`
/// outright and so cannot address these at all.
fn drive_entry(drive: char) -> Option<Wtf16String> {
    let name = Wtf16String::from(format!("={drive}:").as_str());
    // Start small and grow to whatever Windows asks for. The API's two return
    // conventions differ: on success it reports the units written EXCLUDING the
    // terminator, and on an undersized buffer it reports the capacity REQUIRED
    // INCLUDING it. Treating the second as the first indexes past the buffer and
    // panics -- while trying to preserve a legitimate long entry, so the failure
    // would land before the test could restore the process state it borrowed.
    let mut buffer = vec![0u16; 256];
    loop {
        // Zero is TWO different answers, and the last error is the only thing
        // that separates them -- so it is cleared first, because the value left
        // by some earlier call would otherwise be read as this one's.
        //
        // SAFETY: no preconditions.
        unsafe { windows_sys::Win32::Foundation::SetLastError(ERROR_SUCCESS) };

        // SAFETY: the name is NUL-terminated and the buffer is writable for
        // the length passed.
        let written = unsafe {
            windows_sys::Win32::System::Environment::GetEnvironmentVariableW(
                name.as_terminated_ptr(),
                buffer.as_mut_ptr(),
                u32::try_from(buffer.len()).unwrap_or(u32::MAX),
            )
        };
        let written = written as usize;
        if written == 0 {
            // **An earlier version of this comment claimed, as measured, that
            // an empty value and an absent name are the same state and cannot
            // be told apart. That was wrong, and wrong in this crate's
            // signature way: the measurement behind it never cleared the last
            // error, so it could only ever have seen whatever was already
            // there.** Cleared first and re-measured, the two are distinct, for
            // an ordinary name and an `=X:` name alike:
            //
            //   set to ""  -> returns 0, last error ERROR_SUCCESS
            //   deleted    -> returns 0, last error ERROR_ENVVAR_NOT_FOUND
            //
            // The difference is not academic here. Collapsing both to `None`
            // makes the restoration in `BorrowedDriveEntry` DELETE an inherited
            // empty entry rather than put it back -- losing exactly the process
            // state the guard exists to preserve.
            //
            // SAFETY: no preconditions.
            let last = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            return match last {
                ERROR_SUCCESS => Some(Wtf16String::from_units(&[])),
                ERROR_ENVVAR_NOT_FOUND => None,
                // Anything else is neither answer, and folding it into "absent"
                // would make the guard delete an entry over a transient error.
                other => panic!("reading ={drive}: failed with error {other}"),
            };
        }
        if written < buffer.len() {
            // Kept as WTF-16 units rather than going through String: a lossy
            // conversion would replace an unpaired surrogate, so restoring the
            // entry afterwards would write back something the process did not
            // start with.
            return Some(Wtf16String::from_units(&buffer[..written]));
        }
        buffer = vec![0u16; written];
    }
}
/// Sets or clears one of the hidden `=X:` entries.
fn set_drive_entry(drive: char, value: Option<&str>) {
    set_drive_entry_units(drive, value.map(Wtf16String::from).as_ref());
}

/// [`set_drive_entry`], taking the exact units a [`drive_entry`] read returned.
///
/// Restoration goes through this rather than through `&str`, so an entry
/// containing an unpaired surrogate is put back byte for byte.
fn set_drive_entry_units(drive: char, value: Option<&Wtf16String>) {
    assert!(
        try_set_drive_entry_units(drive, value),
        "set ={drive}: entry"
    );
}

/// [`set_drive_entry_units`] without the assertion, reporting success instead.
///
/// Separate because the restoration in [`BorrowedDriveEntry`] runs during
/// unwinding, where a panic would abort the process and destroy the report of
/// the failure that started the unwind.
fn try_set_drive_entry_units(drive: char, value: Option<&Wtf16String>) -> bool {
    let name = Wtf16String::from(format!("={drive}:").as_str());
    let value_ptr = value
        .as_ref()
        .map_or(core::ptr::null(), |v| v.as_terminated_ptr());
    // SAFETY: both pointers are NUL-terminated; a null value clears the entry.
    let ok = unsafe {
        windows_sys::Win32::System::Environment::SetEnvironmentVariableW(
            name.as_terminated_ptr(),
            value_ptr,
        )
    };
    ok != 0
}

/// Borrows one drive's `=X:` entry and puts it back when the test ends,
/// **whether or not the test panicked**.
///
/// Restoring on the last line of the test is not enough, and the hazard is not
/// theoretical: `=X:` is process-global, `cargo test` runs tests as threads in
/// ONE process, and every assertion between the save and the restore is a place
/// the entry can be abandoned. What a sibling test would then inherit is not
/// merely a stale value but one no host would produce -- a path to a directory
/// that no longer exists once the probe directory is removed, or the 1200-unit
/// value that `a_long_drive_entry_round_trips_through_the_reader` installs on
/// purpose. The reader above already names this ("the entry is then never
/// restored") without defending against it; this is the defence.
///
/// A failed restore is dropped rather than asserted, for the reason given on
/// [`try_set_drive_entry_units`].
struct BorrowedDriveEntry {
    drive: char,
    saved: Option<Wtf16String>,
}

impl BorrowedDriveEntry {
    fn take(drive: char) -> Self {
        Self {
            drive,
            saved: drive_entry(drive),
        }
    }
}

impl Drop for BorrowedDriveEntry {
    fn drop(&mut self) {
        let restored = try_set_drive_entry_units(self.drive, self.saved.as_ref());

        // Silence is bought only where it buys something. While unwinding, a
        // panic here aborts the process and destroys the report of the failure
        // that started the unwind, so a failed restore is worth less than the
        // diagnosis it would replace. On the ordinary path there is no such
        // trade: staying quiet would let the suite carry on with corrupted
        // process-global state and fail somewhere unrelated, which is the
        // hardest kind of failure to read.
        assert!(
            restored || std::thread::panicking(),
            "restoring ={}: failed, leaving process-global state corrupted for \
             every test that follows",
            self.drive
        );
    }
}

#[test]
fn a_drive_relative_path_uses_that_drives_entry_verbatim_and_rewrites_a_bad_one() {
    // The arm the sibling test can only BOUND. Without controlling the entry,
    // an implementation that always used the drive root would satisfy every
    // assertion there, because a host with no `=X:` entry cannot tell the two
    // rules apart.
    //
    // **Controlling the entry is not the hazard it looks like**, and that is
    // what unblocked this test. The objection was that `=X:` is process-global
    // while these tests share a process. But the call under test writes that
    // entry itself whenever it is absent or rejected, so this state is already
    // mutated by the code being exercised. What keeps the tests from
    // interfering is not that -- it is that each takes a drive letter no other
    // one can choose.
    //
    // The probe directory is created FIRST so the letter can avoid its drive as
    // well as the current one: an entry naming a directory on the same drive it
    // is recorded for would be honoured, the test would pass, and the
    // cross-drive property below would go unexercised.
    let probe_dir = probe_directory("verbatim");
    let probe = probe_dir.path.to_str().expect("the probe path is UTF-8");
    let drive = probe_drive_from(probe_drives::VERBATIM_ENTRY, probe_dir.drive());
    let _restore = BorrowedDriveEntry::take(drive);

    assert_ne!(
        Some(drive.to_ascii_uppercase()),
        probe_dir.drive().map(|d| d.to_ascii_uppercase()),
        "the probe directory must be on a different drive, or the assertion \
         below cannot show the entry is honoured ACROSS drives"
    );

    set_drive_entry(drive, Some(probe));
    assert_eq!(
        resolve(&format!("{drive}:foo")),
        format!(r"{probe}\foo"),
        "an entry naming an existing directory is honoured verbatim, even onto \
         a different drive -- so \"that drive's own current directory\" is the \
         convention the entry usually holds, not a guarantee about the result"
    );

    // Verbatim means verbatim, including the join. The module doc records that
    // an accepted entry ending in a separator yields a DOUBLED one, and nothing
    // pinned it -- so the observation could have stopped being true without CI
    // noticing, which is the drift this change exists to close rather than
    // commit again.
    set_drive_entry(drive, Some(&format!(r"{probe}\")));
    assert_eq!(
        resolve(&format!("{drive}:foo")),
        format!(r"{probe}\\foo"),
        "an entry is accepted with a trailing separator and concatenated \
         without normalising the join"
    );

    // An entry that does not name an existing directory is rejected, and the
    // call rewrites it to the drive root rather than leaving it stale.
    //
    // Derived from the probe directory rather than hard-coded: a literal like
    // `C:\no-such-directory-for-this-test` is only missing until some host
    // happens to have it, and the test would then assert the rejected case
    // against an accepted one. A child of a directory this test just created
    // cannot exist unless something else creates it in between.
    let missing = probe_dir.path.join("no-such-child");
    assert!(
        !missing.exists(),
        "precondition: the rejected entry must name nothing: {}",
        missing.display()
    );
    set_drive_entry(
        drive,
        Some(missing.to_str().expect("the probe path is UTF-8")),
    );
    assert_eq!(
        resolve(&format!("{drive}:foo")),
        format!(r"{drive}:\foo"),
        "an entry that names nothing is rejected in favour of the drive root"
    );
    assert_eq!(
        drive_entry(drive).map(|v| v.to_string_lossy()).as_deref(),
        Some(format!(r"{drive}:\").as_str()),
        "and the call REWROTE the entry: this is a query that mutates the \
         process environment block"
    );

    // Absent entirely, the entry is created rather than merely read.
    set_drive_entry(drive, None);
    assert_eq!(drive_entry(drive), None, "precondition: entry cleared");
    let _ = resolve(&format!("{drive}:foo"));
    assert_eq!(
        drive_entry(drive).map(|v| v.to_string_lossy()).as_deref(),
        Some(format!(r"{drive}:\").as_str()),
        "resolving created the entry on a host that had none"
    );
}

#[test]
fn a_rejected_drive_entry_is_replaced_by_the_drive_root() {
    // Acceptance needs BOTH a shape and an existence check, and a draft of the
    // module doc claimed it was "a filesystem query rather than a syntax test"
    // -- having measured only the existence half. Every value below names an
    // existing directory, so anything rejected here is rejected on shape alone.
    //
    // Pinned because the distinction is not guessable and the doc asserts it.
    let probe_dir = probe_directory("shape");
    let accepted = probe_dir.path.to_str().expect("the probe path is UTF-8");
    let drive = probe_drive_from(probe_drives::REJECTED_ENTRY, probe_dir.drive());
    let _restore = BorrowedDriveEntry::take(drive);

    // The control: this exact directory IS accepted in canonical form, so the
    // rejections below cannot be blamed on the directory itself.
    set_drive_entry(drive, Some(accepted));
    assert_eq!(
        resolve(&format!("{drive}:foo")),
        format!(r"{accepted}\foo"),
        "control: the same directory in canonical form is accepted"
    );

    // Same directory, spellings that are not fully-qualified `X:\...` form.
    // Each names something that exists; each is rejected anyway.
    for spelling in [
        accepted.replace('\\', "/"),
        format!(r"{accepted}\."),
        format!(
            r"{accepted}\..\{}",
            accepted.rsplit('\\').next().unwrap_or("")
        ),
        format!(r"\\?\{accepted}"),
    ] {
        set_drive_entry(drive, Some(&spelling));
        assert_eq!(
            resolve(&format!("{drive}:foo")),
            format!(r"{drive}:\foo"),
            "{spelling:?} names an existing directory but is rejected on shape"
        );
        assert_eq!(
            drive_entry(drive).map(|v| v.to_string_lossy()).as_deref(),
            Some(format!(r"{drive}:\").as_str()),
            "and the rejected entry is written back as the drive root"
        );
    }

    // The type check, which is a separate necessary condition from both the
    // shape above and the existence check in the sibling test. The module doc
    // has listed an existing FILE among the rejections since the drive-entry
    // work, and nothing pinned it -- so the one observation distinguishing
    // "names a directory" from "names something" lived only in prose.
    //
    // Not a file this test creates: `ProbeDir` may be the read-only
    // `%SystemRoot%` fallback, where creating one needs privileges the suite
    // must not assume. This one is present on every Windows host by
    // construction, and setting an entry to a file does not touch the file.
    let system_file = std::path::PathBuf::from(
        std::env::var("SystemRoot").expect("SystemRoot is set on Windows"),
    )
    .join("System32")
    .join("kernel32.dll");
    assert!(
        system_file.is_file(),
        "precondition: the rejected entry must name an existing FILE: {}",
        system_file.display()
    );
    let system_file = system_file.to_str().expect("the system path is UTF-8");

    set_drive_entry(drive, Some(system_file));
    assert_eq!(
        resolve(&format!("{drive}:foo")),
        format!(r"{drive}:\foo"),
        "{system_file} exists and is canonical, and is rejected anyway because \
         it is not a directory -- so existence alone is not the gate"
    );
    assert_eq!(
        drive_entry(drive).map(|v| v.to_string_lossy()).as_deref(),
        Some(format!(r"{drive}:\").as_str()),
        "and an entry naming a file is written back as the drive root too"
    );
}

#[test]
fn a_long_drive_entry_round_trips_through_the_reader() {
    // The reader grows its buffer, and this is what proves it. Windows reports
    // an undersized buffer by returning the REQUIRED capacity rather than the
    // units written, so a reader that treats the two alike slices past its own
    // buffer and panics -- while preserving a legitimate entry, which is the
    // worst moment for it, because the entry is then never restored.
    //
    // 1200 units is comfortably past the 256 the reader starts with and past
    // the 1024 an earlier fixed-size version used, and is a legitimate value: a
    // per-drive entry is a path, and long paths reach far beyond this.
    let drive = probe_drive_from(probe_drives::LONG_ENTRY, None);
    let _restore = BorrowedDriveEntry::take(drive);

    let long = format!(r"C:\{}", "a".repeat(1200));
    set_drive_entry(drive, Some(&long));

    let read_back = drive_entry(drive).expect("the entry was just set");
    assert_eq!(
        read_back.to_string_lossy(),
        long,
        "a long entry survives the read, so the buffer grew instead of truncating"
    );
}

#[test]
fn a_borrowed_drive_entry_is_restored_even_when_the_borrower_panics() {
    // The guard exists for the unwinding path, and a suite that passes never
    // takes it -- so trusting it would mean shipping an untested defence
    // against the exact failure it is there for. This takes the path on
    // purpose.
    let drive = probe_drive_from(probe_drives::BORROW_GUARD, None);

    // The outer guard is not ceremony. This test installs a sentinel to watch
    // the inner guard put back, and without it that install would destroy
    // whatever the process inherited -- so the test for not losing borrowed
    // state would itself lose some. The inner guard still takes the unwinding
    // path; the outer one covers this test's own borrow.
    let _outer = BorrowedDriveEntry::take(drive);
    let sentinel = format!(r"C:\borrowed-entry-{}", std::process::id());
    set_drive_entry(drive, Some(&sentinel));

    let outcome = std::panic::catch_unwind(|| {
        let _restore = BorrowedDriveEntry::take(drive);
        set_drive_entry(drive, Some(r"C:\the-borrowed-value"));
        panic!("expected: this panic exercises the restore-on-unwind path");
    });
    assert!(
        outcome.is_err(),
        "precondition: the borrower must actually panic, or the unwinding path \
         is not the thing being measured"
    );

    assert_eq!(
        drive_entry(drive).map(|v| v.to_string_lossy()).as_deref(),
        Some(sentinel.as_str()),
        "the guard put the entry back while unwinding, where an end-of-test \
         restore would have been skipped"
    );
}

#[test]
fn an_empty_drive_entry_is_distinguished_from_an_absent_one() {
    // **This pins a correction, not a discovery.** The reader used to fold both
    // into `None`, on a recorded measurement that an empty value and an absent
    // name are the same state. They are not; the measurement behind that claim
    // never cleared the last error, so it could only have read whatever an
    // earlier call left behind -- the same "stated more precisely than the
    // evidence reaches" failure this crate keeps meeting, committed inside the
    // comment that called itself measured.
    //
    // The consequence is what makes it worth a test rather than a fix: with the
    // two collapsed, restoring an inherited EMPTY entry deletes it, so the guard
    // written to preserve process state destroys it in exactly one case.
    let drive = probe_drive_from(probe_drives::EMPTY_VS_ABSENT, None);
    let _outer = BorrowedDriveEntry::take(drive);

    set_drive_entry(drive, Some(""));
    let empty = drive_entry(drive);
    assert_eq!(
        empty.as_ref().map(|v| v.to_string_lossy()),
        Some(String::new()),
        "an entry set to the empty string reads back as PRESENT and empty"
    );

    set_drive_entry(drive, None);
    assert_eq!(
        drive_entry(drive),
        None,
        "and a deleted entry reads back as absent, which is the answer the \
         empty one must not be confused with"
    );

    // The two are distinct in the round trip as well as in the read, which is
    // the property restoration actually depends on.
    set_drive_entry_units(drive, empty.as_ref());
    assert_eq!(
        drive_entry(drive).as_ref().map(|v| v.to_string_lossy()),
        Some(String::new()),
        "restoring an empty entry puts back an empty entry, not an absent one"
    );
}
