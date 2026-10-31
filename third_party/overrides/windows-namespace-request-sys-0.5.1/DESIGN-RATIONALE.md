# Design rationale: windows-namespace-request-sys

Tier 2. How the decisions in [DESIGN-NOTES.md](DESIGN-NOTES.md) were reached --
alternatives considered, drafts that were wrong, and the reasoning that got
discarded along the way. Consulted for "why", never for "what is true now": if
this file and Tier 1 disagree, **Tier 1 wins**.

Cross-referenced by decision ID.

## D-18: `GetFullPathNameW` is not lexical

The decision is in [DESIGN-NOTES.md](DESIGN-NOTES.md) -> `D-18`. What follows is
the record of getting there, which is unusually worth keeping because the same
mistake recurred in nine successive wordings, enumerated below.

### The shape of the error, which never changed

Every wrong draft did the same thing: **named a mechanism the evidence did not
reach.** The subject moved -- what the call does, what a number measures, which
alternatives cost less -- but the failure did not. That is why the decision
records the constraint rather than only the conclusion: a reader who takes only
"it is not lexical" away from D-18 has the answer without the thing that keeps
producing wrong answers.

### The drafts

1. **"This call is lexical."** The original text, contradicted by its own next
   sentence, which said it resolves against the process current directory. A
   lexical canonicalizer is a pure function of its input; this reads mutable
   process state.

2. **"It resolves relative components and `.`/`..` against the process current
   directory."** The first correction, which overshot. Collapsing `.`/`..` has
   an output that is a function of the input alone -- `C:\a\..\b` becomes
   `C:\b` under any current directory, and whether or not `C:\a` exists.
   Measured under two different current directories.

   **This entry itself carried the error it describes, for twenty-three
   rounds.** It read "pure string work and reads no process state at all", and
   two different current directories giving the same answer shows the output
   does not depend on that state, not that nothing was read. Every correction in
   this file was aimed at the *rooting* half; the overreach in the **lexical**
   half was the sentence doing the correcting, which is why nobody looked at it.
   Whether rooting reads process state is a separate question, and it is
   answered observably: changing the current directory changes the result.

3. **"The probe measures roughly 212 ns per resolution."** It does not. The
   probe reports a construct-and-drop cycle whose total contains an allocation
   and a drop, and its own output says so.

4. **"The probe deliberately declines to decompose that total."** An
   over-correction of (3). It does decompose: the report states that recycling
   an already-resolved path pays ~42 ns against ~210 ns and attributes the
   difference to the resolution. What the probe withholds is the **mechanism** --
   whether any of it is a kernel transition -- not the division.

5. **"Roughly 168 ns is this call's measured share."** Taking (4)'s split at face
   value. The subtraction spans the whole preparation step, which makes two heap
   allocations of the crate's own against the clone's one, plus a builder chain.
   Timed directly with no allocation in the loop, the call is about 110 ns --
   the draft overstated it by roughly half.

6. **"Either alternative is cheaper."** Said of `PathCchCanonicalizeEx` and
   `PathAllocCanonicalize` from the first commit onwards, and never measured.
   Nothing in this repository benchmarks them, Microsoft documents behaviour
   rather than relative cost, and `PathAllocCanonicalize` allocates its own
   result. Removed rather than substantiated, because the decision rests on
   rooting semantics and never needed it.

7. **"Touches no filesystem."** The claim that survived longest, because it is
   nearly right. What Microsoft documents is that the function does not verify
   that the resulting path is valid or names an existing file -- a statement
   about *verification*, not about I/O.

8. **"Observation cannot close that gap."** The correction to (7), and wrong in
   a more interesting way: observation *did* close it, in the affirmative
   direction, the moment anyone looked at the right form. Resolving `X:foo` for
   a non-current drive distinguishes an existing directory from an existing
   *file* from a missing one, and rewrites the `=X:` entry when it is not a
   directory. That is a filesystem query and a write to the process environment,
   in a call four drafts had described as reading process memory.

   The reasoning behind those drafts was sound -- the current directory is in
   the PEB, the `=X:` variables are in the environment block, both are process
   memory -- and the conclusion was false. It is the cleanest example in this
   file of the standing failure: a mechanism argued from where the data lives
   rather than measured. Every earlier draft at least *knew* it was asserting a
   mechanism; this one thought it was declining to.

9. **"A filesystem query rather than a syntax test."** Written in the paragraph
   correcting (8), and wrong the same way within a single round. Having found
   that an entry naming a missing directory or an existing *file* is rejected,
   the draft concluded the gate was existence and not shape. Measured, shape
   gates it too and independently: `C:/Windows/System32`,
   `C:\Windows\System32\.`, `C:\Windows\System32\..\System32` and
   `\\?\C:\Windows\System32` are each rejected while naming the same existing
   directory that `C:\Windows\System32` is accepted for. Acceptance is also
   literal -- `C:\Windows\` yields `C:\Windows\\foo`, with no normalisation at
   the join.

   The tell was the word *rather*. Ruling an alternative out is a strictly
   stronger claim than establishing the one you measured, and needs its own
   evidence; three observations of the existence check said nothing about
   shape. Both halves are now pinned by
   `a_rejected_drive_entry_is_replaced_by_the_drive_root` so the next draft
   cannot restate this from memory.

### Two facts that were measured, then asserted too narrowly

Both were found by review after the correction had already shipped, and both
were *enumerations* -- which is the form this kind of error likes.

- **The rooting forms.** Stated first as one case (the current directory), then
  two, and finally three: a relative path takes the current directory, a
  root-relative path takes only that directory's *root* (which is
  `\\server\share\` under a UNC current directory, so "current drive" was
  wrong), and a drive-relative path naming a drive OTHER than the current one
  takes the entry recorded for that drive from `=C:`, while on the current drive
  that entry makes no difference and the process directory wins. The
  current-drive arm is a fourth correction to the same enumeration, found the
  same way as the first three and after them: the count "three forms" was itself
  one of the things stated more confidently than measured.

- **The device set.** The short-circuit was first described as "exact-match
  only", which `CON:` disproves; then enumerated as `CON`/`NUL`/`PRN`/`AUX`/
  `COM1`-`9`/`LPT1`-`9`/`CONIN$`/`CONOUT$`, which omits the superscript
  spellings `COM\u{00b9}`, `COM\u{00b2}`, `COM\u{00b3}` and their `LPT`
  equivalents. Those are exactly the members a hand-written denylist misses, and
  the documentation asserted a closed list without them until a review measured
  it. The tests in [tests.rs](src/full_path/tests.rs) now pin every documented
  spelling so the next omission fails CI instead of a review -- each positive
  spelling against the device path it produces, and each negative one against
  its full rooted result.

  **A third correction, and the one that shows why "the device set" was the
  wrong unit all along.** A review asked for the documented `\CON` case to be
  pinned, since the doc stated it and no test covered it. Writing that
  assertion as the general rule the request was phrased in -- "a root-relative
  device word roots at the current directory's root" -- would have pinned a
  false claim. Measured across all eight accepted names, `NUL` alone
  short-circuits as the final component of ANY path: `\NUL`, `.\NUL`, `a\NUL`
  and `C:\NUL` all reach `\\.\NUL`, while every `CON`, `PRN`, `AUX`, `CONIN$`,
  `CONOUT$`, `COM1` and `LPT1` spelling roots. So a fully qualified path can
  still name a device.

  Every earlier entry here is a claim generalised from too few observations.
  This one was generalised from one MEMBER to a set, which is the same error in
  a dimension nobody had checked -- the enumeration of the set was audited
  twice, and the assumption that its members behave alike never was.

  That distinction between kinds of pinning was itself a later correction, and
  it is the reason the sentence now says which KIND of pinning each case gets. The claim "pins every
  documented spelling" was written while one negative spelling, `CON:x`, was
  covered only by `!starts_with("\\.\\")` -- a predicate the unrooted literal
  satisfies. So the claim was true of the list and false of the strength, which
  is the same shape as the trimming generalisation two entries below.

### The measurement that was itself unmeasured

The sharpest instance in this whole sequence is not about `GetFullPathNameW` at
all. The test helper that reads a `=X:` entry folded "empty value" and "absent
name" into one answer, and said so in a comment that called the equivalence
*measured*: `SetEnvironmentVariableW(name, "")` was reported to succeed and then
read back exactly as a name that was never set.

It does not. `GetEnvironmentVariableW` returns `0` for both, and the last error
is the only thing that separates them -- so a measurement that never cleared the
last error first could read nothing but whatever an earlier call had left there.
Cleared and re-measured, the two are distinct, for an ordinary name and an `=X:`
name alike:

| state | return | last error |
|---|---|---|
| set to `""` | `0` | `ERROR_SUCCESS` |
| deleted | `0` | `ERROR_ENVVAR_NOT_FOUND` |

An earlier note also recorded that `ERROR_ENVVAR_NOT_FOUND` "never surfaced even
for genuinely absent variables", which has the same cause and one more: the
deletion under test had not happened, because the null that deletes an entry had
been marshalled as an empty string instead. Two layers of the harness agreeing
with each other is not a measurement.

The consequence was live rather than cosmetic. `BorrowedDriveEntry` restores a
borrowed entry on unwind precisely so a panicking test cannot leak process
state; with the two answers collapsed, restoring an inherited *empty* entry
**deleted** it. The guard destroyed the state it existed to preserve, in exactly
one case, and only that case.

Pinned by `an_empty_drive_entry_is_distinguished_from_an_absent_one` in
[drive_entry.rs](src/full_path/tests/drive_entry.rs), and verified by re-introducing the collapse,
which makes it fail.

The general form is worth keeping separately from the specific fact: **a
comment that says "measured" is a claim about a procedure, and the procedure can
be wrong in ways the result never reveals.** Every other entry in this file is an
assertion that outran its evidence. This one had evidence, and the evidence was
of something else.

### The sweep, and its arithmetic

The wrong word had spread well beyond where it was reported. The consuming
probe's checklist item named [full_path.rs](src/full_path.rs) only; that file
held **three of the nine** on its own -- the module doc and both doc examples --
with two more in [path.rs](src/path.rs), two in
[tests.rs](src/full_path/tests.rs), one in an acceptance comment and one in
[DESIGN-NOTES.md](DESIGN-NOTES.md). Nine across five files, from a report naming
one. The reported site was a sample, not the population.

### Why the alternatives were never seriously in contention

`PathCchCanonicalizeEx` and `PathAllocCanonicalize` canonicalize without
rooting, which sounds like a strictly smaller job and therefore an easy win. It
is the wrong trade for this crate: a path that is still relative has its meaning
settled on the worker thread at execution time, against a current directory any
thread may have changed in between. That is precisely the race preparation
exists to close, so the "smaller job" omits the part being bought.

Recorded here rather than only in Tier 1 so the next reader who notices the
cheaper-looking API does not have to re-derive why it was declined.

## D-19: grafting a relative path

The decision is in [DESIGN-NOTES.md](DESIGN-NOTES.md) -> `D-19`. This is the record of how it was
reached, with the measurements behind it. They were taken on 2026-10-15 on Windows 11 with a probe
test that was not kept (it failed on purpose so libtest would replay its output), the process
current directory set to `C:\Windows\System32`, the long-path flags below not including a
long-path-aware host.

**How the direction arrived.** The engineer asked what `prepare` did that was not Win32's own, said
a path should reach Win32 untouched with no length limit and no checking of its own, and then, once
"resolve at build time" was read as "when the object is built" and not at compile time, held that the
need to capture the root at construction remained, with relative paths resolved textually against it
later. Windows provides a lexical join, `PathAllocCombine`, which [D-18](DESIGN-NOTES.md#d-18) had not
weighed (it weighed only the canonicalisers), so it was measured.

**`PathAllocCombine` against `GetFullPathNameW`** (base = the process current directory):

| Input | `PathAllocCombine`, flags 0 | `GetFullPathNameW` |
|---|---|---|
| `a\b`, `.\a`, `..\a`, `a\..\b`, `..\..\..\..\x` | the same | the same |
| `\a\b` | `C:\a\b` | `C:\a\b` |
| `a.`, `a ` | `...\a` | `...\a` |
| `a/b` | `...\a/b` (slash kept) | `...\a\b` |
| `C:a`, `C:a\b`, `Q:a` | `C:a`, `C:a\b`, `Q:a` (unresolved) | resolved |
| `C:` | `C:\` | `C:\Windows\System32` |
| `C:\x\..\y` | `C:\y` | `C:\y` |
| `C:/x/y` | `C:/x/y` | `C:\x\y` |
| `\\?\C:\x\.\y` | `C:\x\y` (prefix dropped, `.` collapsed) | `\\?\C:\x\y` |
| `\\.\PhysicalDrive0` | `\\PhysicalDrive0` | `\\.\PhysicalDrive0` |
| `\\.\C:\x\..\y` | `\\C:\y` | `\\.\C:\y` |
| `CON`, `NUL`, `COM1` | `...\System32\CON` and so on | `\\.\CON` and so on |
| empty | the base | error 87 |

With a UNC base `\\server\share\dir\sub`: `a\b` and `\a\b` join correctly, and `..\..\..\x` gives
`\\server\x`, past the share root. With a verbatim base `\\?\C:\work\proj` the result loses its
prefix. Long results (a base of about 350 units): flags 0 fail with `0x800700ce`;
`PATHCCH_ALLOW_LONG_PATHS` gives a `\\?\` path; adding
`PATHCCH_FORCE_ENABLE_LONG_NAME_PROCESS` gives a plain long path. `PATHCCH_DO_NOT_NORMALIZE_SEGMENTS`
keeps a trailing dot or space that the default strips.

**`CreateFileW` on a device name, spelled four ways** (open for query; the number is the Win32 error,
or the file type):

| Name | bare | joined onto the directory | `x\name`, `x` missing | `drivers\name`, `drivers` exists |
|---|---|---|---|---|
| `NUL` | char device | char device | 3 | char device |
| `CON`, `con` | 87 | 2 | 3 | 2 |
| `CONIN$`, `CONOUT$` | char device | 2 | 3 | 2 |
| `PRN`, `AUX`, `COM1`, `COM2`, `LPT1` | 2 | 2 | 3 | 2 |
| `NUL.txt` | 2 | 2 | 3 | 2 |

So `NUL` is a device in any position and a join is faithful to it, where `GetFullPathNameW`'s habit of
resolving `a\NUL` to `\\.\NUL` is not what `CreateFileW` does with `a\NUL` (it fails with error 3 when
`a` is missing). `CON`, `CONIN$` and `CONOUT$` change meaning under a join, which is rule 5 of the
decision. `PRN`, `AUX`, `COM1`, `COM2` and `LPT1` fail the same way in every spelling here, so nothing
is shown about them either way.

**What was weighed and set aside.** A combine call with the long-path flags, for the reasons in the
decision. A classification that grafts everything but the forms known to be wrong, which would make
the list of wrong forms the policy; the engineer's rule was that the graft is allowed "when we can
show that the child path is clearly safe", so the rule names what is safe and lets the rest through
untouched.

## D-20: one overlapped-safe `DeviceIoControl` helper

The decision is in [DESIGN-NOTES.md](DESIGN-NOTES.md) -> `D-20`. This is how it was reached.

**The problem was visible before any code.** The crate's handle-taking entries (`QueryVolumeInformation`
and the rest) call functions that take no `OVERLAPPED`, so how a handle was opened never mattered.
`DeviceIoControl` is the first call that does take one, and the same crate that cannot see how a
handle was opened is the one that has to call it.

**What was weighed.**

- **Ask the handle how it was opened.** The only route to the overlapped flag is an `Nt*` query,
  which the repository's DOCUMENTED APIS ONLY rule forbids, and
  `GetFileInformationByHandleEx` does not report it.
- **Pass a null `OVERLAPPED`.** Documented to fail unpredictably on an overlapped handle, and
  unpredictable is not an outcome a catalogue entry can report.
- **Associate a port of the helper's own, or change the handle's completion-notification mode.**
  Either changes state on a handle the caller owns and shares; a request borrows a duplicate, not
  the right to configure it.
- **Give every call an event and wait for it.** What was chosen. It is the same call on both kinds of
  handle, and it costs an event creation per call, which is the price of not knowing.
- **The event's low bit.** Without it a handle on a completion port gets a packet when the call
  completes, which reaches a consumer's loop as a completion it did not ask for. A sabotage that
  removes the bit is caught by a test that associates the handle with a port and checks nothing
  is queued after the call.

**Why the tests use two controls.** `FSCTL_QUERY_VOLUME_NUMA_INFO` is the entry's own control and the
one whose buffer-size failure is documented, but a volume may have no answer, so tests on it stand
down when there is none. The first sabotage sweep showed what that costs: telling the helper the
buffer was empty turned every call into a failure, and every test that tolerated "no answer" passed.
`FSCTL_GET_COMPRESSION` is answered by every file on an NTFS or ReFS volume, so the helper's tests
assert success on it and do not stand down; that closed the hole.

**How the pending path came to be pinned.** A named-pipe listen (`FSCTL_PIPE_LISTEN`) was the obvious
control that pends until a client connects. It failed with `ERROR_INVALID_FUNCTION`, so it was removed
rather than worked around. The first sabotage entries for the pending path were then caught only
because the full suite happened to make a call pend, and one survived for the same reason; a throwaway
test that counted `ERROR_IO_PENDING` over 20000 calls with the pending branch disabled showed 0, so
those entries were removed as neither a control nor a declared blind spot. The engineer's question
("I guess add a sabotage for it?") then led to the seam: split the helper at the one call that makes it a
control, and drive the rest with a call that does pend on demand.

**What was weighed for that call.** A fake that sets `OVERLAPPED.Internal` and `InternalHigh` and
signals the event by hand would pend deterministically too, but it relies on fields the documentation
reserves for the system and on how `GetOverlappedResult` reads them, which is the incidental-behaviour
binding the platform rules forbid. An overlapped `ReadFile` on a pipe uses only documented behaviour.

**A defect the seam work found in the tests.** The helper's tests opened handles without taking the read
side of `handle_allocation`, so on roughly one run in thirty another test, which holds the write side
while it reasons about a closed handle's value, saw a handle reused and panicked, poisoning the lock for
every test that follows. Measured as 1 mass failure in 60 runs before the fix and none in 200 after.
Separately, two `watch` tests shared the fixture label `watch-defaults` and removed each other's
directory, failing 8 of 30 runs; each now has its own.

**What this did not settle at the time.** `DeviceIoControl`'s own behaviour when it pends was unchecked, for
want of a control that does so on demand. `FSCTL_REQUEST_OPLOCK` is one, found in `NR-2.2`
([D-21](#d-21)'s rationale below).

## D-21: the disk-extents entry

The decision is in [DESIGN-NOTES.md](DESIGN-NOTES.md) -> `D-21`. This is how it was reached.

**A premise in the checklist was wrong.** `NR-2.2` was written as "first, give the helper an input buffer, since
this item and the next two need one". This control takes none; `NR-2.3` and `NR-2.4`
(`IOCTL_STORAGE_QUERY_PROPERTY`) do. The input buffer was done under `NR-2.2` anyway, because it had been
scheduled there and because the one control found that pends on demand, `FSCTL_REQUEST_OPLOCK`, takes input, so
the two were the same piece of work; but the entry itself did not need it.

**Finding the control that pends.** The candidate was `FSCTL_REQUEST_OPLOCK`. A throwaway test requested a read
oplock on an overlapped handle and had another thread open the file for write: the call pended and did not
return in 25 seconds, so it was killed by a timer outside the test (an in-test wait would have hung the run). The
same test with the other thread writing a byte completed in 304 ms against a 300 ms delay, with the oplock's
original level `1` and new level `0`. So a write-open does not break a read oplock and a write does, and the
control pends exactly as long as the test says. The real test uses that, and the same request with no input,
which the device refuses at once, is what shows the input is what made it work.

**Why a `Vec<DiskExtent>` of `u64`s and not the raw reply.** A consumer given the bytes would have to know the
layout, the `ERROR_MORE_DATA` convention and where the count is, which is what an entry exists to carry. `u64`
is what an offset and a length in bytes are; the structure's `LARGE_INTEGER` is signed, and a negative value is
not a length, so it is reported as malformed and not cast. `i64`s that a caller then casts would put the same
check in every caller. `AlignedBuf` was not used: the reply is read with `from_ne_bytes`, so no alignment is
needed, and a `Vec<u8>` allocated with `try_reserve_exact` fails as an error the caller receives.

**The fake device had the wrong code, and the real one showed it.** The documentation suggests
`ERROR_INSUFFICIENT_BUFFER` for a buffer smaller than the header, and the first fake and the first real-device
test both said so. The real-device test failed (87, not 122), a sweep of every size from 0 to 48 found the
boundary (below the count plus one extent: 87; from it up: the answer), and the fake was corrected to match. The
fake is the only way to test the multi-extent half on this machine, so its one measured behaviour is stated where
it is built, and the documented half (`ERROR_MORE_DATA` with the count) is stated as not observed.

**What was weighed and set aside.** Sharing `win-volume-sys`'s `extents.rs` instead of writing this: that crate
is a working, released one, and the rule for speculative duplicate work is to build beside it and decide when the
new one is proven, so the decision is `NR-2.6`. Capping the count the device reports, to refuse a huge
allocation: a cap is a number this crate would have to choose and defend, the count comes from the volume driver,
and the failure is already an error the caller receives; not decided against for ever, but nothing shows it is
needed.

## D-22: storage properties

The decision is in [DESIGN-NOTES.md](DESIGN-NOTES.md) -> `D-22`. This is how it was reached.

**The `NR-2.3` "check first" was the first question, and it was cheap.** The checklist said to check that an
`IOCTL_STORAGE_*` code behaves through the helper before building on it, because `FSCTL_PIPE_LISTEN` had not.
A throwaway test opened the disk with no access and asked for the property with output buffers of 0, 4, 7, 8,
16, 20, 24, 64 and 4096 bytes and with inputs shorter than the structure. That one run gave the whole size
protocol (7: bad length; 8: the header alone; 9 up: more data with the header written; 4096: the reply) and
the reply's shape, and a second probe printing only numeric fields showed the descriptor's own size equals what
remains of the reply. Nothing in the reply was recorded beyond those facts: it holds the device's identifiers,
which identify the machine.

**What was weighed for the reply type.**

- *Parsed, with typed identifiers inside the descriptors.* Declined for now, which the engineer confirmed
  ("at this time, no. We can revisit later if necessary"), for the gradient reason: which identifiers a key
  uses is the locality helper's question, which has not been asked yet, and a typed surface here would be an
  answer to it.
- *Raw bytes of the whole reply only.* A consumer would redo the offset checks every time, and the checks are
  where a damaged reply becomes an out-of-bounds read; so the sections are delimited and bounds-checked here and
  handed back as slices.
- *One generic entry that takes a property id and returns bytes.* Raised with the engineer, who said yes; built
  as `QueryStorageProperty` (below). Nothing argued it had no value, and it lets a consumer reach a property
  this crate has no entry for.

**Why the zeroed-buffer and `u32` reader moved.** The second entry to size a buffer from a reply needed the
same two helpers `volume_extents` had written privately; the mono-repo rule is to fix the layer, so they are now
in the helper's module and `volume_extents` uses them. Its sabotage entry for the allocation moved with the
function.

**The sweep and the redundant check.** The first sweep caught all twenty-one entries, but only after a pass over
the survivors a mutant could hide behind: a length guard in `decode` was refused anyway by the first offset read
that fell outside the header, so mutating it changed no outcome, and it was deleted rather than defended; and the
tests for a call that claims more bytes than it was given room for would have passed with the claim check
removed, because the header the fake wrote was empty, so they now write a valid one, and the test for a reply
shorter than its header asserts the number of calls, which is what distinguishes refusing it from asking again.

**A name collision, and what it showed.** The oplock test in `device_control` had its own `u32_at` that panicked
on a short buffer; the shared one returns `None`. The test now uses the shared one, so the one reader every entry
relies on is the one the earliest tests exercise.

## D-22 addendum: the generic storage property query

**What a generic entry exposes that a typed one hid.** A throwaway probe asked the disk about the cases the
unique-id entry never reached. An exists query on a real property succeeds with nothing written; an unknown
property fails (`ERROR_INVALID_FUNCTION`) for exists and standard alike; a mask query and an undefined query
type fail with `ERROR_NOT_SUPPORTED`. Those shaped the entry: an exists query cannot go through the
learn-the-size loop, which would refuse an empty reply as shorter than its header, so it is its own single call.

**A wrong conclusion that a second probe corrected.** The first probe sent additional parameters of zero bytes
and got the same reply for every length, and the module documentation was written to say a property that takes
none accepts and ignores them (measured). The test written from it failed on a nonzero byte: the unique-id
property refuses 0xA5 with `ERROR_INVALID_PARAMETER`. A second probe over values and lengths showed the first
additional byte is read by that property (0 gives 112 bytes, 1 gives 140) and ignored by the device property.
The claim was withdrawn from both modules before anything was committed, the test now covers both properties,
and the byte's effect on the answer is what shows additional parameters arriving at a real device. The cause was
a probe whose input could not distinguish "ignored" from "zero"; it is recorded because the same shape of probe
would have been trusted again.

**A lead, not an interpretation.** The larger reply for a first byte of 1 may carry more identifiers than the
usual one. The typed entry sends no parameters, so it does not ask. The generic entry is how a consumer gets the
larger reply, and what the byte means is not recorded as anything but an observation. If the locality helper
needs those identifiers, that is the moment to ask what the byte selects and whether the typed entry should
offer it.

**Why `Arc<[u8]>` for the parameters.** A request's `try_clone` returns a handle-capture error and has no
allocation-failure case, and the crate's rule is that an allocation that fails is an error the caller receives.
Sharing the parameters makes the copy allocation-free, so the rule holds without a new error variant. The
input that is sent is built fallibly (`zeroed_output`), at `perform`, where an error can be returned.

**Newtypes over the raw values.** Both the property id and the query type are `i32` in Windows, and a call that
swaps them compiles and asks a different question. Newtypes make the swap a compile error, and `from_raw` keeps
every value Windows may define later reachable, so the types narrow nothing. Only the query type has named
constants, because the three documented values are all that enumeration has; the property ids are `windows-sys`'
and are not reproduced.

## D-23: a disk's NUMA node

The decision is in [DESIGN-NOTES.md](DESIGN-NOTES.md) -> `D-23`. This is how it was reached.

**The documentation check came first, and found less than the other entries had.** Looking up the structure's
reference page returned a 404, and the enumeration page's row for `StorageDeviceNumaProperty` is one sentence
that names no structure, where the unique-id row names its structure and points at the driver kit. `windows-sys`
carries `STORAGE_DEVICE_NUMA_PROPERTY` from the SDK header, so the layout is public, but "public header" and
"documented" are not the same, and the repository's rule is about the second. The decision to proceed on the
header's footing is recorded in `D-23` as one the engineer can overrule, with the generic entry as the way
around it, rather than as settled: the entry was asked for, the property is documented, and the structure is the
one the property's own header ships, but the weaker footing is a fact the engineer should have in front of them.

**One probe, which found the thing worth recording.** A throwaway test asked the property with output buffers of
0, 7, 8, 11, 12, 13, 64 and 512 bytes, for the standard and the exists query. The standard answers were the whole
size protocol and they differed from the unique-id property's: `ERROR_INSUFFICIENT_BUFFER` below the header, and
a success with the header alone between the header and the reply. The `storage_property` module documentation
had stated the unique-id behaviour as the call's, with "(measured)" on it. The loop was already correct because
it reads the size from the header of whichever comes back, but the documentation would have misled whoever added
the next property, so it was rewritten as a table with one row per property measured, and a test pins this
property's row.

**What was weighed.**

- *A single entry for both NUMA questions* (volume and disk). Rejected: they are different calls on different
  handles, and the point of asking per disk is that a volume's answer is not every disk's.
- *Reporting `u32::MAX` as `None`.* Rejected for the reason `QueryVolumeNumaInfo` gave: it would be a sentinel
  this crate chose, and nothing documented says it means none.
- *A test comparing the disk's node with the volume's.* Written, and removed: it printed two numbers and compared
  nothing, and on a one-node machine nothing would distinguish a correct comparison from a wrong one. A test
  that cannot fail is not a test.

**Why the descriptor tests use constructed replies.** A one-node machine reports zero, which is also what a field
read from the wrong place would give when the bytes are zeros. So the field-placement tests build replies with
distinct values in each field, and the sabotages for a node read from the wrong field are caught by those, not by
the real disk.

## D-24: the dependency query

The decision is in [DESIGN-NOTES.md](DESIGN-NOTES.md) -> `D-24`. This is how it was reached.

**What the probes showed that the item did not say.** The item asked for the call and for a statement of what
an unelevated run can reach. Probing it with a buffer whose version field was 0 or 3 gave
`ERROR_INVALID_LEVEL`, and with 1 or 2 it succeeded, so the version is an input the caller sets. Sizes below 72
were `ERROR_INVALID_PARAMETER` and not the `ERROR_INSUFFICIENT_BUFFER` that a size-learning loop looks for, so
a loop that started small would have been refused instead of told a size. The raised first size follows from
that.

**The code `0xC03A0015` looked foreign and was not.** `HOST_VOLUMES` on a volume that is not virtual returned
3225026581, and the first reading was that no Win32 constant names a value that large, so it was recorded as an
`NTSTATUS`. That was wrong: `ERROR_VIRTDISK_NOT_VIRTUAL_DISK` is that value, and the virtdisk errors are defined
in that range. The reading was formed without searching `windows-sys` for the number, which is the check that
would have settled it. It is carried as the call returned it, and the test compares against the named constant.

**Names, and the choice not to trust the buffer.** The structure's name fields are pointers. Reading them
directly would have been simplest and would have trusted a reply this machine could never produce, so each
pointer is converted to an offset against the reply's own address and bounds-checked, and the string must end
inside the bytes the call said it used. Both checks matter independently: a pointer exactly at the end of the
reply is refused for lying outside it, and a string that reaches the end with no room for its terminator is
refused for that, and the tests assert which reason was given because the second check would otherwise hide
the first being removed.

**Why the sabotage entries are built replies and not the device.** No entry could be produced here. The
alternatives were an elevated run, which is not available, or leaving the entry decoding untested; the
decoding is the larger part of the module, so it is tested against replies laid out with `offset_of!` from the
structure's declared types, with every field given a value no other field has, and against a fake device that
asserts the buffer is aligned, never smaller than 72 bytes, and has the version set on every attempt including
the regrow.

**What was weighed.** `with_capacity` raises a size below 72 to 72 and does not refuse it: the call's own
refusal of a smaller buffer is a fact about the call, and the request has no use for a first size it could not
make.

## D-25: the SDDL conversion

**Text only, hiding the binary form.** The engineer asked whether the descriptor could be exposed only
as SDDL, so that no conversion function is public. The binary form is already public in a published
crate (`capture`, `as_ptr`, `as_bytes`), so hiding it would be a breaking removal that still converts
inside, and a request has to hold the binary form anyway: `CreateFileW` takes a pointer, and a
request is built on one thread and performed on another ([D-8](DESIGN-NOTES.md#d-8)). Storing the
text and converting in `perform` would have moved the failure to the worker and repeated the
conversion, and the string would still have been validated at construction. The restriction was
pointed at durable-ioring instead, which decides which of this crate's parameters an open may use
([DI-D-63](../durable-ioring/DESIGN-NOTES.md#di-d-63)), and the engineer's ruling was that the
conversion functions are public here.

**Trusting the reported length.** The first version of `to_sddl` read the text through the length
Windows reports, which is documented to include the terminator. A probe on strings of several shapes
found the figure is not reliable, and the first symptom was a NUL on the end of one rendering and
a refusal for several others that Windows had not made: the check this crate made on the length was
what failed. Reading up to the terminator is the standard contract for a string Windows returns, and
needs no length.

**A mask newtype, not a bare integer.** The mask is over `OBJECT_SECURITY_INFORMATION`, which Windows
extends (labels, attributes, scopes, the protected and unprotected DACL bits), so the five parts the
documentation names are constants and the rest travel through `from_bits`, as the flag types of
[D-12](DESIGN-NOTES.md#d-12) do.

## D-26: `ReOpenFile`

**A borrowed handle.** The request could have borrowed the original for the lifetime of the request,
as a `BorrowedHandle`. That ties every request to the scope that built it, and a request here is
built on one thread and performed on another ([D-8](DESIGN-NOTES.md#d-8)); an owned duplicate costs
one `DuplicateHandle` and removes the lifetime, which is what the volume hint of
`OpenFileByIdentifier` already does.

**Defaults.** The calls that reopen are usually made to change one thing about how a file is open,
which suggests defaulting the others to the original's. `ReOpenFile` has no such mode: the three
values are the new open's, and a share mode of zero is an exclusive one. A default chosen here would
be a policy about which opens to allow, which this crate leaves to its consumers
([DI-D-63](../durable-ioring/DESIGN-NOTES.md#di-d-63)).

**Directories.** Every directory attempt was refused with access denied, across access masks, flags
and share modes, and across the directory handles tried. The measurement is recorded and a test
asserts it, so that a change in what Windows answers is seen rather than assumed.

## D-27: the path entries are built fallibly

**Where an interior NUL is reported.** `NR-3.5` left two places open: the constructors, or `perform`.
`perform` returns `Outcome<T>`, whose error is the raw Win32 code and nothing else, so reporting the
NUL there needs either a new error on every entry's `Request` impl or a code Windows did not
produce, and the crate does not invent codes (see `FullPathError::Unstable`, which exists for that
reason). The constructors were already going to be fallible for `OpenFile` and `WatchDirectory`,
since reading the current directory can fail and the graft is computed there, and a caller who can
still act on a bad path is on the submitting thread, which is where a handle that cannot be captured
is failed too ([D-7](DESIGN-NOTES.md#d-7)). So the choice was one error type at construction against
two places for one kind of failure.

**`ResolveFullPath::new` fallible for one variant.** It reads no directory, so only `InteriorNul` can
come out of it, and a `Result` whose error type has variants it never produces is a small
untidiness. The alternative was adding an `InteriorNul` variant to `FullPathError`, which is already
`#[non_exhaustive]` and already the error of its `perform`, so it would not break a caller. That
leaves a NUL to be found on whatever thread performs the call, a worker that submitted nothing, and
it makes this one entry the odd one out in where the three report the same thing. The shared type
was preferred, and the variant that can only come from the other two is documented on the
constructor.

**An empty path is not refused.** `prepare` refused it with its own failure. A request that does
refuse it adds a check this crate's other entries do not have, and `CreateFileW` already answers an
empty path with a code. `win-durable-ioring`'s open keeps refusing it, as invalid input, because it
did before and a caller of that crate must not see a difference.

## D-28: the forms of a path

**Why not `GetFullPathNameW`'s output.** It is the call this crate already makes to resolve a path, it
is documented, and it does no I/O in the common case, so it was the first thing to try as the
oracle: classify a string by what it returns. The run showed it cannot be one. It normalises an
exactly-backslash `\\?\` path that `CreateFileW` leaves alone, it reads `\??\` as a rooted path on the
current drive where `CreateFileW` takes it as an NT path, and it turns a `/` after `UNC` into a
backslash where `CreateFileW` keeps it as a name character. A consumer that refused a path because
`GetFullPathNameW` called it local would be wrong in each of those cases, and the cases are the
ones that reach the network.

**Why not `PathCch*`.** They are lexical, they do not read `\??\` at all, and they were already ruled
out for the graft by [D-19](DESIGN-NOTES.md#d-19) for returning a drive-relative path unresolved.

**Why not the function `CreateFileW` itself uses.** The conversion of a DOS path to an NT path is
done below `CreateFileW` by a routine that is not a documented Win32 function, and this repository
does not bind one (the rule on documented APIs only), so its answers are reached by behaviour, which
is what the table is.

**Why measure the open and not the documentation.** The documentation names `\\?\`, `\\.\` and UNC
paths. What it leaves for a measurement is the spelling: whether `//?/` is a verbatim prefix, whether
`UNC` is matched without regard to case, whether a slash after `UNC` is a separator, and whether a
`.` under each prefix is removed. Those are exactly the questions a refusal that is made on a string
has to get right, and a wrong answer to the first or fourth is a path that reaches the network with
nothing to stop it.

**Why the example asserts nothing.** What it prints is what one machine did. A test that asserted it
would fail on another Windows build for a reason that is a finding, not a defect. The example is the
means of redoing the measurement when a row is in doubt.

**Why the classifier was withdrawn.** It was planned (`NR-5.2` to `NR-5.4`) so that durable-ioring
could refuse a path that is evidently remote before the open, to avoid a file left behind by an
unbuffered create and a network call made before a refusal. The engineer's answer, 2026-10-16, was
that finding out late is acceptable, and that the check on the opened handle is the only one needed.
That left the classifier with one consumer that had dropped it, a new public type in a published crate,
and one row (`\??\`) whose behaviour this repository found rather than read, which the
platform-integrity rule says not to bind to. The graft rules it would also have derived from stayed
as they are. It would be reopened by a consumer that needs to know a path is remote before it makes any
call.

## D-31: the typed `FileIdInfo` entry

**Delegating to the untyped entry.** The typed entry could have held a `QueryFileInformation` for the class and
read the structure out of the buffer it returns, so that `GetFileInformationByHandleEx` is called from one place
in the crate. That allocates a buffer for a structure of 24 bytes and reads it back through a cast; calling into
a `FILE_ID_INFO` on the stack allocates nothing and needs no cast. The shared part, the faithful report of the
code, is [D-10](DESIGN-NOTES.md#d-10)'s primitive either way, and the class value is shared through
`FileInformationClass::ID`, so the direct call was taken.

**A `From<FILE_ID_INFO>` impl.** It would let a caller holding the structure convert it, and it would make the
dependency's type part of the surface, which [D-13](DESIGN-NOTES.md#d-13) refuses. The conversion is private.

**Where the agreement with the enumeration crate is tested.** That crate depends on this one, so a test here that
used it would be a dependency cycle through a dev-dependency, built against a second copy of this crate. The test
is in the enumeration crate's integration tests, where both are already in reach, and this crate tests the same
agreement against the `FileIdExtdDirectoryInfo` record the enumeration crate reads.

**Refusal codes.** The codes observed for a pipe, `NUL` and a volume are not on the page. Naming them in the tests
would assert the machine; comparing each with the untyped entry's code for the same handle asserts what this
entry promises, that the code passes through unaltered.

## D-32: sharing rather than duplicating

**How the question was put.** The assistant first proposed keeping `CapturedHandle` as the type entries take,
holding a `SharedHandle`, with a second route that still duplicated for a caller holding only a borrow. The
engineer asked whether the wrapper was wanted or merely there, and whether the duplicating route would be added
if the crate were started now. Taken item by item (the list in D-32), every job the wrapper did came from
duplicating, and the borrow-only caller already has two routes outside this crate, std's `try_clone_to_owned` and
`win-handles-sys`'s `duplicate_handle_with_access`. Keeping either would have been compatibility with the crate's
own first design and nothing more.

**The values std allows in an `OwnedHandle`.** Its documentation says one "may have the value `-1`, which in
`OwnedHandle` always represents a valid handle value, such as the current process handle", and "may have the
value `NULL`", which a detached console produces. D-7 refused both before duplicating; D-32 does not refuse them,
because an entry's call answers for them, and an answer from the call is what [D-10](DESIGN-NOTES.md#d-10)
reports.

**The consumers.** `win-durable-ioring` duplicated handles its `OpenedFile` already held as a `SharedHandle` to
ask their final path and kind; `windows-file-enumeration-sys` read its volume serial privately rather than
duplicate its directory handle for [D-31](DESIGN-NOTES.md#d-31)'s entry. Both are the cost the duplication
imposed, and both go with it.

**`new`'s parameter, settled by `NR-6.2`.** `NR-6.1` gave `new` `impl Into<SharedHandle>` so that a capturing
caller kept building through the transition `From`; with that conversion deleted, the question was whether to
narrow it to `SharedHandle`. Narrowing excludes nothing a caller could pass, since `SharedHandle` converts to
itself, and adds `SharedHandle::from` at every call site that holds a `File` or an `OwnedHandle`, which is most of
them, in this crate's own tests and in its consumers. So it stays.

## D-30 addendum: building a path entry from an `AnchoredPath` (`NR-6.3`)

**Why a named constructor and not `From<AnchoredPath>`.** Both entries are built by chaining `with_*` calls onto a
constructor, and `OpenFile::from_anchored(path).with_...` says which input it starts from where
`OpenFile::from(path)` does not; a `From` would also invite `path.into()` at a call site, where the reader cannot
see that the anchoring already happened. The constructor is the only addition, so nothing else is offered for
choosing between.

## D-33: the typed `FileBasicInfo` entry

**Why the times are not converted.** A `SystemTime` would read better at a call site, but a `FILETIME` before 1970 or
past what `SystemTime` holds on a platform would need a refusal or a clamp of this crate's own, and the page offers
neither; the value is the call's, and a consumer converts it where it knows what it wants. The attributes stay a raw
`u32` for the same reason D-29 carries a `FileType` as it came: the constants list grows, and a typed set would drop
a bit it did not know.

## D-34: querying into the caller's buffer

**Why `&mut AlignedBuf` and not `&mut [u8]`.** An `AlignedBuf` states its alignment as a property of the type, so
the caller who chose it can see it, and the same buffer is what `perform` returns. (As first written this said a check
of alignment was what the page gives no ground for. That read the call's page alone; the ground is in the structure
pages, below.) The same buffer is what `perform` returns, so a consumer
can keep the one `perform` gave it.

**Why the buffer is not cleared.** Clearing would cost a pass over the buffer on every batch, which is the cost the
form exists to remove, and would add a guarantee about bytes no consumer reads: a batch is walked by its own
next-entry offsets, and a fixed-size class's structure is the bytes it occupies.

**How the alignment refusal was reached (2026-10-20).** A review of the release branch found that
`FILE_ID_BOTH_DIR_INFO` and `FILE_FULL_DIR_INFO` each "must be aligned on a" "(8-byte) boundary", that both classes
are reachable through `FileInformationClass::from_raw`, and that a misaligned buffer fails with `ERROR_NOACCESS`. A
sweep of every structure the call's page lists found a third, `FILE_STREAM_INFO`. The pages count as the call's own:
the call's page names each as the `lpFileInformation` type, so they say what the buffer is. Nothing unsound had been
possible -- the call writes bytes into a byte buffer -- so this is the contract, not memory safety. Considered and
not taken: **requiring a buffer stated at 8 or more**, deterministic for a caller but a refusal of calls the pages
allow (a class with no requirement, or a buffer that lies on the boundary); and **a buffer type that guarantees 8**,
which would narrow every class for the sake of five. The refusal is reachable end to end only with an empty buffer,
which lies at its stated alignment; where a non-empty one lies is the allocator's.
