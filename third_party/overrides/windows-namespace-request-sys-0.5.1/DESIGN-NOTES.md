# Design notes: windows-namespace-request-sys

Decisions for this crate, recorded before implementation. Pending work is in the
workspace [CHECKLIST-thread-ambient.md](../../CHECKLIST-thread-ambient.md),
milestones M24 to M26. The workspace-level context -- the data plane versus the
namespace plane, and why the namespace plane is synchronous-only -- is in the
workspace [DESIGN-NOTES.md](../../DESIGN-NOTES.md), which is authoritative for
anything this file does not cover.

## Intent

Win32's namespace and metadata surface -- opening, querying, closing -- is
synchronous-only. A call that blocks on a dead network path blocks the thread
that made it, and no overlapped form exists. This crate makes such a call
**capturable as a value**: an owned parameter set that can be built on one
thread and executed faithfully on another.

It does not schedule anything. It is the catalogue-plus-faithful-execution
layer, testable with no ring, no pool, and no async anywhere near it.

## <a id="d-1"></a>D-1: A request excludes ambient context

A request carries call **parameters**. It does not carry the impersonation
token, error mode, or any other thread-scoped state the call runs under; that is
[windows-thread-ambient-sys](../windows-thread-ambient-sys/DESIGN-NOTES.md)'s
subject, and this crate does not depend on it.

The two are **siblings, not a stack**, and the argument is symmetry: a request
can be executed with no captured context at all, and a context is useful to work
that never opens a file. Whoever owns both pairs them at the submission site.

An earlier draft justified this from an audit observation -- that a surveyed
consumer used no ambient state. That was bad evidence and is recorded here so it
is not repeated: a consumer which has not needed something yet says nothing
about whether the layering is right. The claim rests on the symmetry above.

## <a id="d-2"></a>D-2: A request performs its call, and chooses no delivery model

An entry executes its Win32 call faithfully and reports what happened. It does
not decide how the result reaches anyone, and in particular it does **not**
choose what an opened handle is for.

So an opened handle comes back **plain and unassociated**. Associating a handle
with a completion port -- including via `CreateThreadpoolIo` -- permanently
prevents `IoRing` use of it, and the choice must be made before association by a
layer that knows the destination. An entry that associated on the caller's
behalf would foreclose a fork it has no standing to decide.

That is measured, and the measurement is now executable rather than a claim in
prose: see
[windows-platform-probes](../windows-platform-probes/DESIGN-NOTES.md) ->
`d-completion-port`, and `cargo run -p windows-platform-probes --bin
probe-completion-port`. Worth knowing how close it came to being recorded
backwards -- the first version of that probe concluded the *opposite*, because
it checked where the completion arrived rather than whether the operation
succeeded, and so read a refusal (`ERROR_INVALID_PARAMETER`, zero bytes) as a
success. The corrected probe judges a read on its result code, its byte count,
and the bytes it actually landed, and carries the negative controls that make a
failure attributable.

"Faithfully" also constrains how an entry reports failure -- the raw code
preserved unaltered, and snapshotted before any cleanup can overwrite it. That
is stated once, with its mechanism, in [D-10](#d-10) rather than repeated here.

## <a id="d-3"></a>D-3: One entry per Win32 call

A catalogue entry corresponds to exactly one Win32 call. A consumer needing two
makes two requests and sequences them itself, on the client side, observing the
first result before deciding the second.

A compound entry is added only under a **measured** performance argument, and
would be a fusion of entries that already exist rather than a capability they
lack. The cost of not fusing is a real round trip, and that cost is inherent to
remoting rather than particular to this rule: for a *single* operation the
remoted form is always slower than the synchronous one, because the win is
concurrency and thread-freedom rather than latency.

## <a id="d-4"></a>D-4: A request owns duplicates of any handle it names

**Superseded by [D-32](#d-32)** (`NR-6.2`): a request shares a `SharedHandle` and nothing is duplicated. The measured
state-sharing below still holds, since a request names the caller's kernel object.

Five of the round-one entries take a handle rather than a path. A request
**duplicates** such a handle at capture and owns the duplicate for its life.

The distinction a caller reasoning in value semantics will get wrong, stated
plainly: **a path is a value and is copied; a handle is a reference to a kernel
object, and duplicating it shares that object rather than cloning it.** So a
request is self-contained with respect to **lifetime** -- it cannot be left
pointing at a handle its originator has closed -- and is **not** isolated with
respect to **state**.

Where that has teeth is measured, not reasoned. See
[windows-platform-probes](../windows-platform-probes/DESIGN-NOTES.md), which
pins these as tests:

| Question | Measured |
|---|---|
| Does a duplicate share directory-enumeration state? | **Yes** -- it continues where the source stopped |
| Control: do two separate opens share it? | **No** -- a separate open restarts |
| Does closing the duplicate disturb the source? | **No** |
| Do single-shot metadata queries disturb an enumeration in progress? | **No**, on the handle or on a duplicate |

The third row is what makes D-4 safe at all: a request may own a duplicate and
drop it without damaging the handle its caller kept. The first means the
contract must say that a duplicate is **not** an independent enumeration and
that an independent traversal needs a fresh open. The fourth narrows the hazard
usefully -- only the two directory-enumeration classes mutate shared state, and
every other query is a pure read that composes freely.

## <a id="d-5"></a>D-5: The round-one entry list is audited, and its omissions are deliberate

The entries below are the union of what three real consumers call:
[windows-file-watcher](../windows-file-watcher/DESIGN-NOTES.md) and
[windows-file-enumeration-sys](../windows-file-enumeration-sys/DESIGN-NOTES.md)
in this repository, and `MikeGrier/Globazog-rs` at commit `55a0b1ae`.

| # | Entry | Needed by |
|---|---|---|
| 1 | `CreateFileW` | all three |
| 2 | `OpenFileById` | watcher |
| 3 | `FindFirstChangeNotificationW` | watcher |
| 4 | `CloseHandle` and variant close routines | all three |
| 5 | `GetFileInformationByHandleEx` | all three |
| 6 | `GetFileInformationByHandle` (non-`Ex`) | watcher |
| 7 | `GetFinalPathNameByHandleW` | watcher; Globazog via `canonicalize` |
| 8 | `GetVolumeInformationByHandleW` | watcher |
| 9 | `GetFullPathNameW` | enumeration |

**Deliberately out of round one**, so a later reader can tell a considered
omission from an unexamined one: `DeleteFileW`, `MoveFileExW`,
`CreateDirectoryW`, `RemoveDirectoryW`, `SetFileAttributesW`,
`SetFileInformationByHandle`, `GetFileAttributesExW`, `CreateSymbolicLinkW`,
`CreateHardLinkW`, `QueryDosDeviceW`, and `FindFirstFileExW` /
`FindNextFileW`. No audited consumer calls any of them. Every audited open is
`OPEN_EXISTING` against a **directory**; nothing creates a file.

Two entries are distinct calls rather than variants, which is easy to miss:
`OpenFileById` is a second open primitive with no creation disposition, and
`GetFileInformationByHandle` is not a class of `GetFileInformationByHandleEx`.

### The `CreateFileW` entry keeps parameters no consumer uses

No audited consumer passes a `SECURITY_ATTRIBUTES` or an `hTemplateFile`; all
pass null. Both are supported anyway.

This is not speculative scope. An entry that cannot express two of its own
call's parameters is a **narrowed** `CreateFileW`, and narrowing a platform
entry to fit the consumers currently in view is the anti-pattern this
repository's platform-integrity rule names. The completeness of a single entry
is a levelness requirement, not a guess about future demand. Recorded here so
the absence of a consumer is not later mistaken for an oversight.

## <a id="d-6"></a>D-6: Membership is decided by blocking, not by marshaling difficulty

An entry belongs in the catalogue because it is a blocking namespace call a
consumer needs performed off its own thread -- **not** because it is awkward to
marshal.

The distinction matters because the easiest entry to build is also one of the
most important. `GetFileInformationByHandleEx` needs almost no marshaling work:
its inputs are a handle, a scalar class, and a buffer size, with no pointer into
caller memory anywhere. By a difficulty test it would look like a candidate for
omission. By the correct test it is the most-called namespace operation across
all three audited consumers, and the call whose lack of an overlapped form is
why an unassociated handle had to become a first-class destination at all.

A catalogue selected by implementation difficulty would be a catalogue shaped by
our convenience rather than by consumer need.

## <a id="d-7"></a>D-7: A handle that cannot be captured fails at construction

**Withdrawn -- with nothing duplicated there is no capture to fail; [D-32](#d-32) records why none of its refusals is kept** (`NR-6.2`).

Handle capture validates the source and duplicates it on the **calling** thread,
and every way that can go wrong is a construction error. The alternative --
storing the caller's raw value and discovering the problem when the entry runs
-- would report a caller's mistake on a worker, to code holding neither the
source handle nor any way to correct it. Capture happens where the caller can
still do something about it.

Three inputs are refused before `DuplicateHandle` is reached, and the reason is
not tidiness:

- **Null.** Never a handle.
- **`INVALID_HANDLE_VALUE`.** This is the trap that makes validation load-bearing
  rather than defensive. `INVALID_HANDLE_VALUE` and the current-process
  pseudo-handle are the **same value**, `-1`, so `DuplicateHandle` accepts it and
  returns a perfectly valid handle -- to the current process. An unchecked
  `CreateFileW` failure passed straight into capture would therefore *succeed*,
  and the request would carry a process handle where a file handle was meant.
- **The remaining pseudo-handles** (`-2` through `-6`: current thread, the three
  token forms, and the reserved `-3`). A pseudo-handle is not a reference to a
  kernel object; it is a constant the *using* thread resolves against itself. Its
  meaning would therefore change when the request moved to a worker, which is
  precisely what this crate exists to prevent.

Everything else reaches `DuplicateHandle` and is judged by Windows.
An already-closed handle fails there, with `ERROR_INVALID_HANDLE`.

The duplicate is taken with `DUPLICATE_SAME_ACCESS`, because a request must be
able to perform exactly the call its caller opened the handle for, and
non-inheritable, so capturing a handle never widens what a child process
reaches.

Duplication is fallible, so the type offers `try_clone` rather than `Clone`. The
result refers to the same kernel object, with everything D-4 says about that.

## <a id="d-8"></a>D-8: Security attributes are captured as a self-relative, owned blob

A `SECURITY_ATTRIBUTES` is not a value. It points at a security descriptor, and
an **absolute** descriptor is itself a structure of raw pointers to an owner
SID, a group SID, a DACL and a SACL that are quite possibly on the caller's
stack. Copying the struct would copy those pointers, so a request built on a
submitter and run on a worker would be reading the submitter's dead stack
frame. Capture therefore converts to the **self-relative** form -- every part at
an offset inside one contiguous blob -- and owns that blob.

Validation runs at capture, on the calling thread, for the same reason handle
capture does (see [D-7](#d-7)): a descriptor Windows will reject is the caller's
to fix, and reporting it from a worker gives the report to code that cannot act
on it.

### The three-way distinctions this must not collapse

A single nullable pointer runs together outcomes that are different grants, so
each is given its own representation:

| Caller passed | Meaning |
|---|---|
| a null `lpSecurityAttributes` | default security, non-inheritable handle |
| attributes whose descriptor is null | default security, but the caller's inheritance choice |
| attributes with a descriptor | the caller's security |

and inside a descriptor, a **DACL** has three states that a `bool` or an
`Option` would flatten: **absent** (the object takes its default), **NULL**
(everyone gets everything), and **empty** (nobody gets anything). NULL and empty
are opposites, so collapsing them is not a loss of detail -- it is an inversion.
`AclState` keeps all four cases (with a populated list reporting its entry
count), and applies to the SACL as well.

### Alignment is a property of the buffer, not the first field

A self-relative descriptor must live in DWORD-aligned storage. A `Box<[u8]>`
guarantees an alignment of 1, so this is a requirement the obvious
representation silently fails. It is the second such requirement in the crate
-- the directory-information classes need 8-byte alignment for the same
underlying reason -- so it is met once, by an aligned-buffer primitive whose
alignment is stated at construction, rather than being solved twice by two
different local tricks.

That primitive was this crate's own `AlignedBuffer`, and is now `AlignedBuf` from the [io-buffers](../io-buffers/README.md) crate, which the
crate re-exports because the type appears in its public signatures (the query
result and the captured security descriptor's storage). The move is a breaking
change: the old type's constructors panicked on a bad alignment and aborted on
allocation failure, and `AlignedBuf` returns an error for both. The first
version of the move took that error with `expect` at each call site, which left
a panic where there had been an abort; the engineer rejected both (2026-10-13),
and an allocation that fails is now an error the caller receives, at every site:

- `QueryFileInformation::perform` returns `ERROR_NOT_ENOUGH_MEMORY` when the
  buffer for the caller's capacity cannot be allocated. It is the one code in
  this crate that no Win32 call produced, and the call is not made.
- `SecurityDescriptor::capture` returns
  `SecurityCaptureFailure::Allocate`, whose error carries the same code, from
  both of its allocations, which share one function.
- `SecurityDescriptor` and `SecurityAttributes` are no longer `Clone`: copying
  one allocates, and an infallible `clone` could only panic or abort. Each has a
  `try_clone`, which reports `Allocate`; a request's `try_clone` returns that
  error as it is ([D-32](#d-32)).

Every call site passes a constant alignment, so the alignment's own refusal
cannot occur; the constants are asserted to be powers of two at build time, and
every refusal of an allocation is then reported as an allocation failure. What
no test reaches is `SecurityDescriptor::try_clone` failing, because it allocates
exactly the size of a buffer that already exists; [sabotage.json](sabotage.json)
declares it. Why the buffer is fallible and clone-less is
[IB-D-6](../io-buffers/DESIGN-NOTES.md#ib-d-6).

## <a id="d-9"></a>D-9: Path preparation is duplicated from the enumeration crate, deliberately and temporarily

**Superseded in part by [D-19](#d-19).** The mechanism below -- resolving with `GetFullPathNameW` when the request is built, refusing a path past `MAX_PATH`, validating a verbatim path -- is replaced by a graft onto a captured root. The need to fix a relative path's meaning at construction, and the duplication with the enumeration crate and its scheduled merge-or-delete decision, stand; once `NR-3.3` lands the two copies differ, so that decision is no longer a plain de-duplication.

Path preparation resolves a caller's path **on the calling thread, when the
request is built**, because the process current directory is shared mutable
state that any thread can change between submission and execution. That is not
a new contract: `windows-file-enumeration-sys` already ships exactly this
preparation, and writing a second one from scratch is what this repository's
mono-repo policy refuses.

So it was **copied**, byte for byte, and adapted only where this crate's error
taxonomy differs. The alternatives were considered and rejected:

- **Depend on the enumeration crate.** It is released; this crate is not. A
  released crate cannot depend on an unpublished `0.0.0` sibling, and the whole
  point of the branch this landed on is a minimal-impact, fast route to
  publication. Pointing the released crate at this one would have made that
  strictly worse, and would also have inverted the layering: the enumeration
  crate sits *above* the namespace catalogue.
- **Extract to a third shared crate.** A new workspace member and a new
  published crate, purchased before the consumer that would justify it exists.
- **Write a second implementation.** The option the mono-repo policy names as
  the wrong one.

The duplication is therefore the mechanism that keeps a working, released crate
stable while this one is proven -- not debt incurred by accident. `path.rs`
carries a provenance comment naming its source and the commit it was taken at,
and the **merge-or-delete decision is scheduled**, not left to be rediscovered:
see `M26+.3` in
[CHECKLIST-thread-ambient.md](../../CHECKLIST-thread-ambient.md). It is gated on
this branch merging with `main`, which is when the de-duplication happens. Until
then, a fix to either copy must be applied to both.

## <a id="d-10"></a>D-10: The faithful-execution contract is a primitive entries bind to, not a rule they restate

Every entry reports what Windows reported: the raw code, unaltered, never
normalised or reclassified. `ERROR_FILE_NOT_FOUND` means a missing directory
from an open, an **empty** directory from a first query, and a genuine failure
from a later one -- only a consumer holding that context can tell them apart,
so any interpretation here destroys information no layer above can rebuild.

The harder half is *when* the code is read. `GetLastError` is volatile thread
state: almost any subsequent Win32 call overwrites it, including cleanup nobody
thinks of as a call -- a `CloseHandle` in a `Drop`, a buffer release, a
restoration guard unwinding. Reading it a few statements after the failure is a
race against the entry's own tidying up.

**The contract forbids reclassifying a code; it does not require inventing one.**
Two entries -- `QueryFinalPath` and `ResolveFullPath` -- retry a growing buffer,
and "the required size kept changing" is a failure Windows never reported at all,
so there is no raw code to pass through faithfully. Each reports it as its own
`Unstable` variant alongside a `Win32` variant that still carries the raw code
unaltered. That is additive rather than a reclassification: no Win32 code is
altered, and a failure Win32 did not produce is not dressed up as one.

Stating this explicitly is not pedantry. `ResolveFullPath` originally read the
rule as "an entry must return a Win32 code" and therefore synthesized
`ERROR_INSUFFICIENT_BUFFER` for the unstable case -- a code Windows also returns
on its own, leaving a caller unable to tell the crate's retry giving up from a
genuine Windows refusal. Inventing a code is the same information loss the
paragraph above forbids, just arriving from the other direction. See
[the seam's error-type rationale](src/request.rs).

So it is **not left to each entry's discipline.** `outcome::perform` takes the
call as a closure and snapshots the code in the statement after it returns,
with the convention-specific forms (`perform_bool`, `perform_handle`,
`perform_nonnull_handle`, `perform_nonzero`) layered on it. An entry binds to
that function; it does not re-implement the rule. This is the
derived-rather-than-restated posture the repository's contract-integrity rule
asks for, applied to the one guarantee every entry in the catalogue shares.

Two consequences worth stating because they are easy to get backwards:

- **Success never consults `GetLastError`.** Many Win32 calls leave a non-zero
  last error behind on success, so an entry that checked the error slot rather
  than the return value would invent failures. The return value decides.
- **The two handle conventions are both real and are not interchangeable.**
  `CreateFileW` fails with `INVALID_HANDLE_VALUE`; other calls fail with null.
  Both forms exist by name because using one where the other belongs turns a
  failure into a plausible-looking handle.

Capture failures are *not* governed by this. `handle`, `security`, and `path`
report a named stage plus a code, because there the useful question is which
part of building the request went wrong, and that is answered on the calling
thread before any entry runs.

## <a id="d-11"></a>D-11: A handle carries its close routine

Closing is a catalogue entry, which surprises people. `CloseHandle` looks like
bookkeeping, but it is a blocking namespace call: it waits for outstanding I/O
and can block hard on a dead network path or an ejected device. A consumer that
carefully moved its opens onto a worker and then closed on its own thread would
have moved the wrong half.

The audit then showed a close entry **cannot assume its routine**. A
`FindFirstChangeNotificationW` handle is closed with
`FindCloseChangeNotification`, and `CloseHandle` is wrong for it *silently* --
no error, just a leak. So the routine travels with the handle rather than being
chosen at the call site. This is the same shape
[windows-threadpool-sys](../windows-threadpool-sys/README.md) already needed for
wait targets, and adopting it here is deliberate reuse of a solved problem
rather than a parallel invention.

Two structural consequences:

- **`ChangeNotification` is a type, not an `OwnedHandle`.** `OwnedHandle`'s drop
  is `CloseHandle`, which is exactly the wrong routine, so returning one from
  the watch entry would hand every caller a silent leak. The type is what
  remembers.
- **Closing happens exactly once, enforced by construction.** `CloseRequest::perform`
  consumes the request through a `ManuallyDrop` so the destructor cannot close
  again; both constructors suppress the source value's own drop so it cannot
  close first; and an unperformed request still closes on drop, because a
  request that quietly did nothing would leak.

## <a id="d-12"></a>D-12: An entry carries flags verbatim and defaults nothing

Two of the audited consumers open without `FILE_FLAG_OVERLAPPED` and one opens
with it, because the watcher's handle is destined for a completion port and the
other two are not. The entry carries that as a **field**. Deciding it would be
the delivery-model choice [D-2](#d-2) refuses to make.

The same reasoning forbids helpful defaults. `FILE_FLAG_BACKUP_SEMANTICS` is
mandatory to open a directory at all and every audited consumer passes it -- and
it is still not implied, because an entry that quietly added a flag would be
deciding what the caller meant, and the same field is what a caller opening a
plain file must be able to leave out. Every parameter starts at "the caller said
nothing"; a plausible-looking default is exactly what a caller cannot see they
were given.

This extends to values the crate does not recognise. `NotifyFilter` is a newtype
over a bitmask rather than an enum, so a bit Windows defines and this crate has
never heard of still reaches it unaltered. Its named constants are bound to the
platform's own `FILE_NOTIFY_CHANGE_*` values rather than restated as literals,
so the two cannot drift.

## <a id="d-13"></a>D-13: A tagged union becomes an enum; a dependency's type does not become our surface

`OpenFileById` takes a `FILE_ID_DESCRIPTOR`: a tag plus a union that a caller
must keep in step by hand. `FileIdentifier` is an enum instead, so the tag is
implied by the variant and the two cannot disagree. All three identifier kinds
are supported although only `FileId` appears in the audited consumers, for the
[D-5](#d-5) reason.

Its object-id variant carries a `u128`, not windows-sys's `GUID`. That type
implements neither equality nor `Debug`, and a public surface should not be
shaped by whichever binding crate we happen to build against -- that is
behavior inherited from a dependency rather than owned. The conversion happens
at the FFI boundary, where it belongs.

## <a id="d-14"></a>D-14: The catalogue offers a seam, in two traits because the distinction is real

Every entry is a value whose `perform` is the single point where Win32 is
touched. `Request` and `ConsumingRequest` are the traits over that, so a
*consumer's* code can be written against "a request that produces `T`" and
exercised in that consumer's own tests without a filesystem, a network path, or
a device that may not be present. Offering the seam is a levelness obligation:
a platform whose consumers cannot test against it is not level, whether or not
a consumer has asked yet.

**Two traits, not one.** An open is a parameter set -- it may be performed
repeatedly, producing an independent handle each time, so it takes `&self`. A
close is one-shot; performing it consumes the request, which is what makes
closing twice impossible ([D-11](#d-11)). One trait would have to pick a side
and both choices lie: a `&self` trait makes a close look repeatable, and a
`self` trait makes every open look single-use.

**A seam, not an abstraction layer.** The traits exist for substitution by a
consumer. Nothing in this crate dispatches through them and the entries keep
their inherent `perform` methods, which is what an ordinary caller uses. A test
asserts the two paths agree, so the trait cannot become a second, divergent
implementation.

The traits are object-safe, because a consumer holding heterogeneous requests
needs that, and a test pins it -- object safety is easy to lose by accident and
nothing else would notice.

## <a id="d-15"></a>D-15: Examples are compiled, and cover what a caller gets wrong

Every public type carries a runnable example, and so does every method whose
correct use is not obvious from its signature. They are doctests, so a contract
change breaks the build rather than leaving the documentation quietly teaching
the old answer -- the repository's contract-integrity rule applied to this
crate's prose.

Coverage is chosen by **where a caller goes wrong**, not by what is easy to
demonstrate:

- the three-way security distinction and the absent/NULL/empty DACL
  distinction, where collapsing a pair inverts a grant ([D-8](#d-8));
- the two handle-failure conventions, where using one for the other turns a
  failure into a plausible-looking handle ([D-10](#d-10));
- that a request shares the caller's handle rather than copying it, which a
  caller reasoning in value semantics gets backwards ([D-32](#d-32));
- rearming a notification, without which a waiting loop spins;
- that a close is one-shot while an open is repeatable ([D-14](#d-14)).

An example that merely shows a constructor being called would satisfy a count
and teach nothing; these are written to be read by someone about to make the
mistake.

## <a id="d-16"></a>D-16: Acceptance checks coverage twice, because the audit had two purposes

The audit that produced the entry list had two purposes, and the first draft of
this plan discharged only one -- establishing the *operation set* -- while
silently leaving the question of whether the *scenario* is served. That is how
the coverage gap got missed once already, so acceptance is split in two and the
split is structural rather than a heading:

- **Operation coverage** re-expresses each audited call site against the
  catalogue, one test per call site, named for the consumer and the call. A gap
  shows up as a missing test rather than as a paragraph nobody re-reads.
- **Scenario coverage** confirms the catalogue serves each consumer's actual
  *shape*: a request built on one thread and executed on another under a
  captured context, many requests across concurrent workers from one shared
  capture, and a handle opened by one request carried into a later one -- the
  combination where a handle a request shares ([D-32](#d-32)) meets
  [D-10](#d-10)'s shared enumeration cursor, and the one no single-entry test
  reaches.

**An entry may be wider than its call site, and that is the correct direction.**
The watcher passes NULL for two of `GetVolumeInformationByHandleW`'s
out-parameters; this crate's entry fills them anyway. Acceptance records that as
correct rather than as a mismatch, because the failure this milestone guards
against is an entry that is *narrower* than what a consumer needs.

**`windows-thread-ambient-sys` is a dev-dependency, and only that.** A request
carries no ambient context and a context is useful to work that never opens a
file, so the two are paired at a submission site by whoever owns both
([D-1](#d-1)). The scenario tests *are* that submission site -- which is the
only honest way to demonstrate the relationship without creating the coupling
the design refuses. Acceptance also asserts the negative: every entry works with
no captured context at all, because access was checked at the open. A catalogue
that silently required a context would not be a sibling of the ambient crate; it
would be a layer above it.

## <a id="d-17"></a>D-17: A Win32 out-buffer is allocated in content units and sized in terminator-inclusive units, and the two differ by one on purpose

Raised as a review finding on PR #46 and rejected as a defect; recorded here so it
is not re-raised, and so nobody "corrects" it in the unsafe direction.

Two conventions meet at every string-out call in this crate, and they are not the
same:

- **`Wtf16String::with_capacity(n)`** takes *content* units and reserves `n + 1`,
  because the terminator always occupies one slot. Total storage is `n + 1`.
- **Win32's length parameters** (`nBufferLength` for `GetFullPathNameW`,
  `cchFilePath` for `GetFinalPathNameByHandleW`, `nVolumeNameSize` for
  `GetVolumeInformationByHandleW`) count units *including* the terminator. A call
  told `k` writes at most `k` units total, so at most `k - 1` content units.

Every site here passes the same number to both. The consequence is that the
allocation is **one unit larger** than the size Win32 is told:

| site | `with_capacity(n)` | total storage `n+1` | told Win32 `k` | max content Win32 writes (`k-1`) | spare (`n+1-k`) |
|---|---|---|---|---|---|
| [path.rs](src/path.rs) `resolve` | 260 | 261 | 260 | 259 | 1 |
| [volume.rs](src/volume.rs) label | 261 | 262 | 261 | 260 | 1 |
| [volume.rs](src/volume.rs) filesystem name | 261 | 262 | 261 | 260 | 1 |
| [full_path.rs](src/full_path.rs) first attempt | 260 | 261 | 260 | 259 | 1 |

The slack is in the **safe** direction: the buffer is over-sized relative to what
Win32 is permitted to fill, never under-sized. The review that raised this
described it as under-sizing the buffer and as being able to truncate at the
documented limit; both are the wrong way round.

### The documented limit is honoured exactly, not approximately

`path.rs` is the case where a real off-by-one would show, because it enforces a
limit rather than growing. `MAX_PATH` is 260 *counting the terminator* and
`MAX_PATH_CONTENT` is 259. Told `nBufferLength = 260`, `GetFullPathNameW` can
write at most 259 content units -- exactly `MAX_PATH_CONTENT`, which is exactly
what the success path permits before returning `PathTooLong`. So the buffer size,
the limit constant, and the success test agree; there is no length that is
rejected when it should fit, and none accepted that should not.

The growing callers cannot truncate at all: a path that does not fit produces the
"required size including the terminator" form, which is strictly greater than the
size passed, so it is always recognised as a retry rather than mistaken for a
success. `written == capacity` is unreachable -- that would mean `capacity - 1`
content units, which would have fit.

### Why the spare unit is not removed

Tightening each site to `with_capacity(k - 1)` would make the allocation exactly
`k` and is equally correct. It is not done because the gain is one `u16` per call
and the cost is editing four `unsafe` FFI call sites whose current sizing is
proven correct -- a bad trade on a crate whose buffers are handed to the kernel.
If it is ever done, it must be done together with the size passed to Win32, never
to one side alone.

## <a id="d-18"></a>D-18: `GetFullPathNameW` is not lexical, and the genuinely lexical alternative is the wrong call

**Its decision, to keep `GetFullPathNameW` at construction, is superseded by [D-19](#d-19); its measurements stand.** D-18 weighed only the canonicalisers, which leave a relative path relative; the lexical joins `PathAllocCombine` and `PathCchCombineEx` were not weighed, and are measured in D-19.

**The correction.** This crate described `GetFullPathNameW` as **lexical** in
**nine places across five files**, in a sentence that then went on to say it
resolves against the process current directory. Those two claims disagree, and
the call is better described as doing two separable things. It collapses
`.`/`..` and normalizes separators, which *is* lexical -- `C:\a\..\b` becomes
`C:\b` whatever the current directory is, and whether or not `C:\a` exists. It
*also* **roots** most paths that are not fully qualified, and that reads mutable
process state. Three forms use it three different ways -- though not from three
different sources, since the first two both derive from the process current
directory and so does the current-drive case of the third: a relative path
takes the current directory; a root-relative path like `\foo` takes only that
directory's *root*, which is `\\server\share\` when the current directory is a
UNC path and so is not a drive at all; and a drive-relative path such as
`C:foo` takes the entry recorded for that drive -- usually that drive's own
current directory, though the entry is used verbatim and an accepted one may
name a directory on another drive entirely. That rule has two arms:
for a drive other than the current one Windows reads the hidden `=C:` entry
recorded for it, which moves independently of the process current directory;
for the *current* drive the entry makes no difference to the result and the
process current directory wins. Measured -- setting `=Q:` while the process is
on `Q:` changes nothing. ("Makes no difference" rather than "is ignored" for the
same reason the probe no longer says "without consulting a device": an entry
that is read and then discarded is indistinguishable from one never read.) So the call is not lexical *as a whole*, and the claim that
holds unqualified is that it **does not verify what it produces** -- the
documented guarantee, and narrower than the "touches no filesystem" an earlier
draft claimed. A black-box success cannot establish that broader claim, and it
did not need to: the drive-relative form *disproves* it outright, as the
measurement below records.

**"Most" rather than "every", because a legacy device name short-circuits the
rooting.** `CON` resolves to `\\.\CON` and is not rooted, so it is an
unqualified input that is nonetheless invariant -- which is why the rooting
clause cannot be stated unconditionally. The form is looser than exact match:
`CON:`, `CON.`, `con` and `CONIN$` all map, while `CON.txt`, `a\CON` and
`CON:x` root normally. This matters to a crate that prepares paths on a
caller's behalf: `prepare("CON")` returns a device.

**`NUL` is the one member that clause does not describe**, and the clause was
written from `CON` and stated of the whole set. Measured across all eight
accepted names: seven root normally once anything precedes them, and `NUL`
short-circuits as the final component of any path -- `\NUL`, `.\NUL`, `a\NUL`
and `C:\NUL` all reach `\\.\NUL`, where the `CON` spellings root. Only a suffix
(`NUL.txt`, `NUL:x`) takes it out. So a **fully qualified** path can still
resolve to a device, which is the part a caller needs: a rooted result is not by
itself evidence that a name refers to a file on that volume. Pinned by
`nul_is_the_one_device_word_a_path_around_it_does_not_save`.

Keeping the two halves apart matters, because the decision below turns on the
rooting half alone. Saying the call resolves `.`/`..` "against the current
directory" -- as a first draft of this correction did -- attributes process-state
dependence to the one operation that has none, which is the same imprecision
running the other way.

The wrong word had spread well beyond the one file that was reported. The sweep
that found the rest, and its arithmetic, are Tier 2:
[DESIGN-RATIONALE.md](DESIGN-RATIONALE.md) -> `D-18`.

**The decision: keep `GetFullPathNameW`.** Two canonicalizers that do not root
exist -- `PathCchCanonicalizeEx` and `PathAllocCanonicalize`. They are the wrong
call here for a semantic reason: resolving against the current directory *at
submission* is what this crate is buying. A canonicalizer that does not root
would leave a relative path relative, so its meaning would be settled on the
worker at execution time, against a current directory any thread may have
changed in between -- which is exactly the race preparation exists to close. What
they omit is the part that is wanted.

**No cost comparison is claimed, and that is deliberate.** An earlier draft of
this decision called the alternatives "cheaper". Nothing here benchmarks them,
Microsoft documents behaviour rather than relative cost, and
`PathAllocCanonicalize` allocates its own result -- so the word was a guess
wearing the clothes of a measurement, in a decision whose whole subject is not
doing that. It is also unnecessary: the rooting semantics decide this alone.
Neither is reliably free of process state either, since
`PATHCCH_ALLOW_LONG_PATHS` makes `PathCchCanonicalizeEx` consult the process
long-path setting unless the FORCE variant is used.

Recorded with the alternatives named so the next reader does not re-derive it.
If the reasoning is ever wrong -- a consumer wanting a pure string operation,
having resolved relativity another way -- they are named here.

**It does touch the filesystem, on one form -- measured, after four drafts said
otherwise.** Resolving a drive-relative path for a drive that is *not* the
current one checks that drive's `=X:` entry against the filesystem *and* against
a required shape. An accepted entry is used **verbatim** and need not be on that
drive -- with `=X:` set to `C:\Windows`, `X:foo` is `C:\Windows\foo`. Anything
rejected is replaced by the drive root, and the entry is **written** back there
-- created when absent, so this happens on a pristine host too.

Both halves of the gate were measured, and one of them refuted a draft of this
very decision. Existence matters: a missing directory and an existing *file* are
each rejected. Shape matters too, and independently -- `C:/Windows/System32`,
`C:\Windows\System32\.`, `C:\Windows\System32\..\System32` and
`\\?\C:\Windows\System32` were all rejected while naming the same existing
directory that `C:\Windows\System32` was accepted for. So the draft calling this
"a filesystem query rather than a syntax test" named a mechanism the evidence
contradicts: it is both, and the list is observation rather than specification. The rewrite mutates the process environment
block as a side effect of what reads like a pure query. For the current drive
the entry makes no difference to the result and is not rewritten -- stated as
those two measured effects rather than as "not consulted", because installing an
entry and observing the outcome cannot separate "not read" from "read and
ignored", and a draft of this very paragraph said "not consulted" anyway.

Earlier drafts concluded the opposite by reasoning that the current directory
lives in the PEB and the `=X:` variables in the environment block, so both are
ordinary process memory. The reasoning was sound and the conclusion wrong. That
is the failure this decision exists to name: a mechanism argued from where the
data lives rather than measured. It also means the guarantee this crate relies
on has to be the narrow one -- the call does not *verify* what it produces --
because the broad one is not merely unproven but false.

**The constraint this decision carries, and not just its conclusion:** state
only what the evidence reaches. Nine drafts of this entry each named a
mechanism it did not -- the call's nature, what a number measured, what the
alternatives cost, whether any filesystem was touched. The wordings differ; the
error does not. A reader taking only "it is not lexical" away from D-18 has the
answer without the thing that kept producing wrong ones.

The constraint reaches further than prose, and the sharpest case was not about
this call at all: a test helper collapsed an *empty* `=X:` entry into an absent
one on a comment that called the equivalence measured, when the measurement had
never cleared the last error that distinguishes them. So **"measured" is itself
a claim, and a procedure can be wrong in ways its result never shows.** The
practical consequence was that restoring a borrowed empty entry deleted it. See
[DESIGN-RATIONALE.md](DESIGN-RATIONALE.md) -> "The measurement that was itself
unmeasured".

The drafts themselves, and why each failed, are Tier 2:
[DESIGN-RATIONALE.md](DESIGN-RATIONALE.md) -> `D-18`.

## Open, and inherited rather than introduced

- **Path resolution under a captured identity.** A path must be resolved on the
  calling thread, because the process current directory is mutable by any
  thread -- but `GetFullPathNameW` never expands a drive letter,
  and drive-letter resolution follows the *impersonated* token's logon session.
  So a root resolved on a submitter and opened on a worker under a captured
  token can name a different device. The workspace has this as an open decision;
  until it is settled, this crate documents the hazard rather than implying its
  resolution is complete.
- **Reusing the shipped path preparation.** **Resolved 2026-10-18, in the other
  direction:** the enumeration crate is to consume this crate's `AnchoredPath`
  ([D-30](#d-30)) and delete its own preparation (its `D-22`).

## <a id="d-19"></a>D-19: A relative path is grafted onto a root captured at construction, by concatenation, and only when it is clearly safe

**Status: decided by the engineer on 2026-10-15; implemented** by `NR-3.3` (`src/anchored.rs`) and
wired into the entries by `NR-3.4` to `NR-3.6`; what those settled is [D-27](#d-27). It supersedes the mechanism of [D-9](#d-9) and the decision of
[D-18](#d-18); D-18's measurements stand.

**Amended by [D-28](#d-28): the reason given for rule 6 below is corrected there. The rule stays as
wide as it is.**

**What is decided.** A path reaches Win32 as the caller gave it: a `Wtf16String`, with no length limit
and no check of this crate's own beyond the one that cannot be avoided, an interior NUL, which a
`PCWSTR` cannot carry. `OpenFile::new` takes the `Wtf16String` directly. What is kept from D-9 is the
need to fix the meaning of a relative path when the request is built, because the process current
directory is shared and mutable and a request is performed later, on any thread. That is done by
capturing the current directory when the request is built (`GetCurrentDirectoryW`, which does no
I/O) and resolving a relative path against it **textually**, so that it still names that directory
when the request is performed, after the current directory has changed.

**The graft is concatenation:** the captured directory, a backslash unless it already ends in one (a
drive root does), then the child. There is no combine call, no flag, and no canonicalisation:
`CreateFileW` normalises `.`, `..` and trailing dots and spaces on the final path itself, and Win32
decides whether a long result is acceptable to the host. Nothing here prefixes `\\?\`, ever.

**Only a clearly safe child is grafted.** All of these must hold:

1. it is not empty;
2. it does not begin with `\` or `/` (that rules out root-relative, UNC, `\\?\` and `\\.\`);
3. it contains no `:` (that rules out drive-relative `C:a` and `C:`, and a stream `name:stream`);
4. if the captured directory is a UNC path, the child has no `..` component;
5. no component of the child is a reserved device name -- `CON`, `PRN`, `AUX`, `NUL`, `COM` and `LPT`
   with a digit, `CONIN$`, `CONOUT$` -- compared without regard to case, after removing everything from
   the first `.` and any trailing spaces. The list is the one in Microsoft's "Naming Files, Paths, and
   Namespaces", and the rule errs toward not grafting;
6. the captured directory does not begin with `\\?\` or `\\.\`. Win32 does not normalise a `\\?\` path,
   so a grafted `.\a` would be read literally and name a file that is not there, where the same `.\a`
   handed to Win32 resolves against that directory. A `\\.\` path it does normalise ([D-28](#d-28)), so
   for that prefix the rule is wider than its reason. It was found by review, which
   reported that a process whose current directory is verbatim opens `.\kernel32.dll` and could not open
   the grafted form; the child-process test in the open tests shows it against Win32.

A child that is not grafted is passed through untouched, and Win32 resolves it when the call is
made, against whatever the current directory is then. For those forms the race D-9 closed stays
open, and is documented as open. A root-relative child (`\a\b`) is among them: grafting it needs the
root of the captured directory, a parse this version does not do, and no work is scheduled for it.

**Why not the alternatives**, measured on 2026-10-15 on Windows 11 with the process current directory
set to `C:\Windows\System32` (the full table is in [DESIGN-RATIONALE.md](DESIGN-RATIONALE.md) ->
`D-19`):

- **`GetFullPathNameW` at construction (D-9, D-18).** It is exact for every form, but for a
  drive-relative path on another drive it reads, validates against the filesystem, and may write the
  hidden `=X:` entry (D-18), on the submitting thread; and its output buffer is what imposed the
  `MAX_PATH` limit.
- **`PathAllocCombine` / `PathCchCombineEx` (a lexical join).** Exact for relative and root-relative
  paths, and also not safe to hand every path: it returns a drive-relative path unresolved and turns
  `C:` into `C:\`; it returns an empty path as the base; it drops the `\\?\` prefix and collapses `.`
  and `..` inside a verbatim path; and it drops the `\\.\` prefix, so `\\.\PhysicalDrive0` becomes
  `\\PhysicalDrive0`. Its long-path flags are Win32's, not this crate's, and
  `PATHCCH_ALLOW_LONG_PATHS` alone **rewrites a result longer than `MAX_PATH` into a `\\?\` path**,
  which is the prefixing this decision refuses. So the call needs a classification of the path first
  anyway, and once the child is known to be a plain relative one, concatenation does the same job
  with none of that.

**Device names are why rule 5 exists.** `NUL` is a device whatever precedes it, so a graft changes
nothing for it. `CON`, `con`, `CONIN$` and `CONOUT$` open as devices when bare and as a lookup of a
file of that name when grafted, so a graft would change what the call means.

**Not verified**, and stated as such:

- Concatenation is not measured against Win32's own resolution of the same relative path; it follows
  from the join's measured equality with `GetFullPathNameW` for the relative forms.
- `GetCurrentDirectoryW` with a current directory longer than `MAX_PATH` was not measured.
- `PRN`, `AUX`, `COM1`, `COM2` and `LPT1` open as nothing in any spelling on the machine measured,
  so whether they behave as `CON` does when a device is present is unknown; they are on the list
  because the documentation names them.
- Behaviour in a process that is long-path aware was not measured; the test binary is not.

**Devices, measured on 2026-10-16**, Windows 11, a Hyper-V guest, not elevated, each probe run
with a time limit:

- `NUL` opened as `\\.\NUL`, with and without the overlapped flag: a 4096-byte read returned no
  bytes, a write was accepted, and the probe took 51 ms. It cannot hang, and what is written to it is
  discarded while the call reports success.
- `CON`, `CONIN$` and `CONOUT$` opened. `PRN`, `AUX`, `COM1`, `LPT1` and `CLOCK$` were not found.
- **`COM3` opened, and a 16-byte read on it was still blocked after 8 seconds**, with no timeouts
  set. Whether `CancelIoEx` or `CancelSynchronousIo` interrupts it was not tried.
- The device namespace was not enumerated: a `QueryDosDeviceW` probe returned no names and was not
  pursued. What else is reachable is therefore not measured.

**A device is reachable without being grafted.** Rule 5 decides only whether a relative child is
grafted. A path that begins with `\\.\`, and a `COM` name above 9 (which needs that prefix), is
passed to `CreateFileW` as it is and reaches whatever device Windows names, so the rule is not a
filter of what can be opened, and this crate does not filter (the policy is its consumers', as in
[DI-D-63](../durable-ioring/DESIGN-NOTES.md#di-d-63)). What to do about it is `DI-14.6` in
[durable-ioring's CHECKLIST.md](../durable-ioring/CHECKLIST.md); this decision schedules no work
in this crate for it.

**As implemented by `NR-3.3`:**

- **The graft is computed when the request is built**, not when it is performed, and the result is
  stored beside the path as given. The path handed to Win32 is the same either way and is fixed to
  the directory of the moment of construction; `perform` allocates nothing, and the root is not
  kept after it has been used.
- **The current directory is read only for a child that rules 1, 2, 3 and 5 allow**, since only rules 4
  and 6 need it. A path that is absolute, a device, a stream or empty makes no call.
- **`NUL` is not grafted.** It is among rule 5's names, and the rule errs toward not grafting; the
  checklist item had listed `NUL` among the names that graft, which disagreed with the rule as
  numbered. The rule governs. `NUL` opens the same device grafted or not, so nothing observable turns
  on it.
- **Rule 5 also names `COM` and `LPT` followed by the superscripts `U+00B9`, `U+00B2` and `U+00B3`**,
  which the documentation lists after the ASCII digits. The comparison folds ASCII case only.
- **A directory is UNC for rule 4 when it begins with two separators of either kind.** That is wider
  than a UNC path and errs toward not grafting. A verbatim or device directory is decided earlier, by
  rule 6, and is not grafted at all.
- **A failure to read the directory is its own error**, with the retry shape and vocabulary of
  [`ResolveFullPath`](src/full_path.rs): Windows' code unaltered, or that the size kept changing.
- **An interior NUL is refused when the request is built**, which is [D-27](#d-27).

**Scheduled work:** none here. `DI-13.2`'s rework in `win-durable-ioring` was waiting on the entries
taking a `Wtf16String`.

## <a id="d-20"></a>D-20: Every device control goes through one overlapped-safe helper

**Superseded by [WV-D-7](../win-volume-sys/DESIGN-NOTES.md#wv-d-7).** The helper and the volume NUMA
entry that was its first user ([WV-D-8](../win-volume-sys/DESIGN-NOTES.md#wv-d-8)) moved to
`win-volume-sys` by `NR-2.8`, because a device control against a volume is not a namespace
operation. This crate has no `DeviceIoControl` call. The decision this row recorded is no longer
this crate's, and its reasoning is in [DESIGN-RATIONALE.md](DESIGN-RATIONALE.md).

## <a id="d-21"></a>D-21: The disk-extents entry reads the reply and refuses one that is not what it counts

**Superseded by [WV-D-3](../win-volume-sys/DESIGN-NOTES.md#wv-d-3).** Moved to `win-volume-sys` by
`NR-2.8`, where the two implementations of this call were reconciled into one.

## <a id="d-22"></a>D-22: Storage properties are one call with a typed entry each, and a reply is reported as bytes the device gave

**Superseded by [WV-D-9](../win-volume-sys/DESIGN-NOTES.md#wv-d-9).** Moved to `win-volume-sys` by
`NR-2.8`.

## <a id="d-23"></a>D-23: A disk's NUMA node is its own entry, carried unaltered, and the size protocol is a property's

**Superseded by [WV-D-9](../win-volume-sys/DESIGN-NOTES.md#wv-d-9).** Moved to `win-volume-sys` by
`NR-2.8`, with the storage-property decision it built on.

## <a id="d-24"></a>D-24: The dependency query is its own entry, the version is an input, and names are copied out of the reply

**Superseded by [WV-D-10](../win-volume-sys/DESIGN-NOTES.md#wv-d-10).** Moved to `win-volume-sys` by
`NR-2.8`, with its declared blind spot: no real entry has been observed, because attaching a
virtual disk needs an elevated token.

## <a id="d-25"></a>D-25: The SDDL conversion is public in both directions, and the length Windows reports for the text is not trusted

**Status: decided and implemented** by `NR-3.1` (`src/security/sddl.rs`).

**What is decided.**

- **Both directions are public methods of `SecurityDescriptor`**: `from_sddl`, which takes SDDL text
  and returns the descriptor, and `to_sddl`, which writes the parts a `SecurityInformation` mask
  selects as text. They are the documented `ConvertStringSecurityDescriptorToSecurityDescriptorW` and
  `ConvertSecurityDescriptorToStringSecurityDescriptorW`. Hiding one form so that no conversion is
  exposed was considered and set aside: the binary form is already public (`capture`, `as_ptr`,
  `as_bytes`) in a published crate, so hiding it would be a breaking removal and would still convert
  inside. The engineer, 2026-10-15: "go forward with the conversion functions as public".
- **The input is a `Wtf16String`**, which is what the crate's other entries take for text, and a
  string with an interior NUL is refused **before the call** as `InteriorNul`, with
  `ERROR_INVALID_PARAMETER`: Windows reads up to the NUL and would accept what is before it, which is
  a different descriptor than the string says.
- **Nothing is parsed or defaulted here.** Windows decides what the text means. There is no revision
  parameter, since `SDDL_REVISION_1` is the only revision defined, and the mask is a newtype over the
  raw bits ([D-12](#d-12)), with the five documented parts named and `from_bits` for the rest.
- **A string Windows refuses is `InvalidSddl` with Windows' own code**, read in the statement after
  the call; a refusal to write is `ToSddl`. Both are new variants of the existing
  `#[non_exhaustive]` `SecurityCaptureFailure`, so nothing breaks.
- **What Windows returns is copied and freed.** Both calls return `LocalAlloc` memory. The descriptor
  is copied into the crate's own self-relative blob by the existing `capture`, and the text into a
  `Wtf16String`; the original is freed by a guard on every path, so no raw pointer reaches the caller
  ([D-8](#d-8)).
- **The length Windows reports for the text is not used.** The text is read up to the terminator the
  documentation promises.

**The text form is not canonical.** Two strings can name one descriptor (a rights mask and its alias),
and what `to_sddl` returns is Windows' rendering, not necessarily the string a descriptor was built
from. Descriptors compare by their bytes.

**Measured on 2026-10-15**, Windows 11:

- The length reported with the written text is the string's length plus one for some strings (the
  documented figure, the terminator counted), plus two for others, and plus none for others
  (`D:P(A;;GA;;;WD)` is 15 characters and reports 16; `D:(A;;GA;;;WD)(A;;GR;;;BA)` is 26 and reports
  28; `O:BA` is 4 and reports 4), so a copy that trusted it would end in a NUL or lose the last
  character.
- Refusals: text that is not SDDL, and lower-case parts and aliases (`d:p(a;;ga;;;wd)`), are
  `ERROR_INVALID_PARAMETER`; an unknown ACE type is `ERROR_INVALID_DATATYPE`; an unknown SID alias is
  `ERROR_INVALID_SID`; a DACL of 6000 entries is `ERROR_INVALID_ACL`, where 1500 is accepted; an
  unpaired surrogate is `ERROR_INVALID_PARAMETER` and does not panic.
- An empty string is accepted and is a descriptor with no parts. `D:` is an empty DACL,
  `D:NO_ACCESS_CONTROL` a NULL one, and `O:BA` has no DACL, so the three grants `AclState` keeps apart
  are all reachable from text.
- A mask selects what is written: owner, group and DACL separately and together; a part the
  descriptor lacks, and an empty mask, write nothing.
- A descriptor written as text and read back is equal to the original for every canonical string
  tried, an owner, a group, a DACL, a SACL and a NULL DACL among them. A SACL is written with no
  privilege, since the descriptor is in memory.
- A file created with a descriptor from text is given its DACL, read back through `GetSecurityInfo`.
  A generic right is stored as the file rights it maps to (`GA` reads back as `FA`), which is the
  file system's, not this conversion's.

**Not established.** Whether any descriptor and mask make Windows refuse to write text was not found,
so `ToSddl`'s kind is reached by no test ([sabotage.json](sabotage.json) says so). Whether every
binary descriptor survives the round trip was not tried beyond the strings above: ACEs of types SDDL
has no name for were not. That the memory Windows allocated is freed is read from the code, not
observed.

**Scheduled work:** none in this crate. [DI-13.5](../durable-ioring/CHECKLIST.md) takes the release
that carries this.

## <a id="d-26"></a>D-26: `ReOpenFile` is one entry that owns a duplicate of the original, and nothing in it is defaulted

**Amended by [D-32](#d-32)** (`NR-6.2`): the request holds a share of the original, not a duplicate.

**Status: decided and implemented** by `NR-3.2` (`src/reopen.rs`).

**What is decided.**

- **One entry for one call** ([D-3](#d-3)): `ReOpenFile`, a new handle to what an open handle already
  names, with the access, the share mode and the flags stated anew. It names nothing by path or
  identifier, so it is a third open primitive beside `OpenFile` and `OpenFileByIdentifier`.
- **The request owns a duplicate of the original**, as `OpenFileByIdentifier` does for its volume
  hint ([D-8](#d-8)): the request is built on one thread and performed on another, and cannot be
  left naming a handle its originator has closed. `try_clone` duplicates it and is fallible, so the
  request is not `Clone`.
- **All three parameters start at zero and are carried verbatim** ([D-12](#d-12)). There is no
  default access, share mode or flag; which flags `ReOpenFile` accepts is the call's to say.
- **Every refusal is the call's own code**, unaltered. No sharing violation or access denial is
  inferred from, mapped, or retried here.
- **`perform` returns an `OwnedHandle`**, a new handle on every call however often the request is
  performed.

**Measured on 2026-10-15**, Windows 11, a Hyper-V guest, not elevated:

- An access the original's sharing does not allow (a read-only original sharing read, reopened for
  writing), a share mode that does not allow the original's access (an original open for writing,
  reopened sharing read only), and a share mode of zero against an original open for reading are
  each `ERROR_SHARING_VIOLATION`. The same requests with a compatible share mode succeed.
- Access is not limited to the original's: a read-only original is reopened for writing when the
  sharing allows it, and the write is seen through the original.
- Access zero with a share mode of all three and no flags succeeds; `DELETE` access succeeds;
  `FILE_FLAG_OVERLAPPED` is accepted; the `NUL` device reopens.
- `FILE_FLAG_NO_BUFFERING` added to a handle opened without it takes effect: an unaligned read that
  succeeds through the original is `ERROR_INVALID_PARAMETER` through the reopened handle, and an
  aligned one succeeds.
- **A directory handle was refused with `ERROR_ACCESS_DENIED` for every combination tried**,
  whatever the access, share mode and flags, for the directory handles tried (opened for
  `FILE_LIST_DIRECTORY` with `FILE_FLAG_BACKUP_SEMANTICS`, among them). No working directory reopen
  was found, and why is not established.
  The entry does not special-case it.

**Scheduled work:** none in this crate. [DI-13.6](../durable-ioring/CHECKLIST.md) takes the release
that carries this.

## <a id="d-27"></a>D-27: The path entries take a `Wtf16String`, are built fallibly, and refuse an interior NUL where the request is built

**Status: implemented** by `NR-3.4`, `NR-3.5` and `NR-3.6` (`src/open.rs`, `src/watch.rs`,
`src/full_path.rs`, `src/anchored.rs`). The fallible construction is the assistant's recommendation,
written into `NR-3.5` as such, and was implemented on the engineer's instruction to implement
`NR-3.4`; it has not been ruled on separately.

**What is decided.**

- **`OpenFile::new`, `WatchDirectory::new` and `ResolveFullPath::new` take a `Wtf16String` and return
  a `Result<Self, PathCaptureError>`.** `prepare`, `PreparedPath`, `PathError`, `PathFailure` and the
  `path` module are removed. A request holds no preparation of its own; the contract of the paths is
  [D-19](#d-19).
- **`PathCaptureError` has three variants**: `InteriorNul`, `CurrentDirectory(Win32Error)` and
  `CurrentDirectoryUnstable { attempts }`. The second and third are the failure of reading the
  current directory, with Windows' code unaltered and the retry shape of
  [`ResolveFullPath`](src/full_path.rs); only `OpenFile` and `WatchDirectory` can produce them.
- **An interior NUL is refused where the request is built**, in all three entries. `Wtf16String` does
  not exclude one, and Win32 reads a path up to its first NUL, so a call would act on a shorter path
  than the one named and report success for it. The check is the one the `SAFETY` comments in the
  entries rest on.
- **`path()` is the path as given and `win32_path()` is the path handed to the call**, on `OpenFile`
  and `WatchDirectory`. They are equal unless the path was grafted ([D-19](#d-19)), so a caller or
  a test can see what the graft did.
- **`ResolveFullPath` keeps the path exactly as given and grafts nothing.** It is the call; a caller
  who wants a path fixed to the current directory when the request is built uses `OpenFile` or
  `WatchDirectory`.
- **Nothing else about a path is checked.** An empty path, a path longer than `MAX_PATH`, and a
  verbatim path that is not fully qualified all reach the call and are its to answer. Those refusals
  belonged to `prepare` and went with it.

**Measured on 2026-10-16**, Windows 11:

- A relative name given to a request, with the process current directory then moved elsewhere,
  is opened (`OpenFile`) and watched (`WatchDirectory`) at the directory it was built in. The same
  name given to a request built after the move is `ERROR_FILE_NOT_FOUND` for the open and is
  refused for the watch. Both are tested in a child process, so the current directory of the test
  process is not changed.

**Scheduled work:** none in this crate. [NR-3.7](CHECKLIST.md) and `M6.1` in
[windows-platform-probes' CHECKLIST.md](../windows-platform-probes/CHECKLIST.md) rewrite the probe
that measured `prepare`. `win-durable-ioring`'s open was adapted only far enough to build, and its
rework is [DI-13.7](../durable-ioring/CHECKLIST.md).

## <a id="d-28"></a>D-28: The forms of a path are the readings `CreateFileW` gives its prefix, measured, and `GetFullPathNameW` is not the oracle for them

**Status: measured on 2026-10-16 by `NR-5.1`.** The classifier of path forms that was planned to carry
it, `NR-5.2` to `NR-5.4`, was **withdrawn the same day**, because the refusal it was for was dropped
(below); the measurements stand as a record. They correct the reason given for rule 6 of
[D-19](#d-19).

**What is decided.** How `CreateFileW` reads the start of a string is the table below, and not what
`GetFullPathNameW` makes of it, because the two disagree. A consumer that ever needs to classify a path
classifies by this table. The example that produced the rows reproduces them. Nothing in this crate
classifies a path by it, and no public type does.

**How it was measured.** `cargo run --example path_form_measurements`
([examples/path_form_measurements.rs](examples/path_form_measurements.rs)) puts each spelling to both
calls through this crate's own entries. The open is of a file that exists, with a `.` component in the
middle of its path, so that it succeeds only if the path was normalised, or of a share on `localhost`
that does not exist, so that a path read as UNC fails with a network error (67) and one that is not
fails with a local one. Run on Windows 11 build 26300, not elevated, with the process current drive
`Q:`.

| Start of the string | `GetFullPathNameW` | `CreateFileW` reads it as |
|---|---|---|
| `C:\a`, with `/` or `\` | slashes made backslashes, `.` removed | an ordinary path; opened |
| exactly `\\?\` | prefix kept, **`.` removed** | **verbatim, not normalised**: the `.` stayed and the open failed with 3 |
| `//?/`, `\/?\`, `/\?\`, `\\?/`, `//?\` | prefix made `\\?\`, `.` removed | normalised first, then verbatim: opened |
| `\\.\`, `//./`, `\/.\`, `/\.\`, `\\./`, `//.\` | prefix made `\\.\`, `.` removed | **normalised**: opened |
| exactly `\??\` | a rooted path on the current drive (`Q:\??\...`) | **an NT path, not normalised**: a drive path without a `.` opened, one with a `.` failed with 3 |
| `/??/`, `\??/`, `/??\` | a rooted path on the current drive | a rooted ordinary path: failed with 123 |
| two leading separators, any mix of `\` and `/`, then a name: `\\`, `//`, `\/`, `/\` | `\\server\share`, slashes made backslashes | UNC: error 67 |
| `\\?\` then `UNC\`, `UNC` in any ASCII case (`UNC`, `unc`, `Unc` tried) | kept | UNC: error 67 |
| `//?/UNC/` and `//?/UNC\`, a forward slash in the prefix | prefix made `\\?\` | UNC: error 67 |
| `\\.\` then `UNC\` or `UNC/`, any case, any slash mix in the prefix | prefix made `\\.\` | UNC: error 67 |
| `\??\` then `UNC\`, any case | a rooted path on the current drive | UNC: error 67 |
| `\\?\UNC/` or `\??\UNC/`, a forward slash after `UNC` | slashes made backslashes | **not UNC**: failed with 2, the slash kept as a name character |
| `\\?\UNC` then a name with no separator | kept | not UNC: failed with 3 |
| `\\??\` then a drive path | unchanged | UNC, with `??` as the server: failed with 53 |

Three things the table shows that a reading of `GetFullPathNameW` alone would miss.
**`GetFullPathNameW` normalises an exactly-backslash `\\?\` path**, removing the `.`, where
`CreateFileW` does not. **`\??\` is a form `CreateFileW` reads and `GetFullPathNameW` does not**: the
second calls it a rooted path on the current drive, and the first treats it as an NT path and reaches
the network through `\??\UNC\`. **And `GetFullPathNameW` changes a `/` that `CreateFileW` keeps**,
after `UNC` under an exactly-backslash `\\?\` or `\??\`. A classifier built from `GetFullPathNameW`'s
output would be wrong about all three.

From the same run: `GetFullPathNameW` of the empty string fails with 123, of `\` is the root of the
current drive, of `\\?` and `\\.` (with no separator after them) gives the prefix with a separator
added, and of `C:` is the current directory of that drive.

**The forms, as the readings above give them.**

- **Ordinary**, which Win32 normalises (`.`, `..`, slashes of either kind): relative, drive-relative
  (`C:a`), drive-absolute (`C:\a`), and rooted (`\a`, and the forward-slash spellings of `\??\`).
- **UNC**, which Win32 normalises and which reaches the network: two leading separators of either kind
  that are not a verbatim or device prefix.
- **Verbatim**, spelled with backslashes only (`\\?\`): not normalised, a `/` is a name character.
  Spelled with a forward slash anywhere in its four characters it is normalised first and is then
  handed on as `\\?\`.
- **Device** (`\\.\`, any slash mix): normalised.
- **NT** (`\??\`, backslashes only): not normalised.
- **A UNC marker** inside the last three: the component `UNC`, in any ASCII case, followed by a
  separator, which is a backslash for the two that are not normalised and either kind for the rest.
  It makes the path UNC, which reaches the network.

**What a string alone can show to be remote** is UNC, and a verbatim, device or NT path with a UNC
marker; nothing else. A refusal on that was planned before an open (`durable-ioring`'s `DI-13.1`
(b)) and dropped by the engineer on 2026-10-16: there
is no check before the open, and the check on the opened handle is the only one. Discovering a remote
device late costs a file left behind by an unbuffered create, which is documented, and a network call
made before the refusal.

**Not measured, stated as such.**

- UNC matched against non-ASCII lookalikes of `UNC`, and any case folding beyond ASCII.
- Trailing dots and spaces, and short (8.3) names, in or after a prefix.
- A UNC marker followed by the server on a path whose share does exist; a share that does not exist
  was used so that nothing remote is contacted.
- Whether `\??\` is documented by Microsoft. It was found by measurement. A consumer binds to
  documented behaviour ([copilot-instructions.md](../../.github/copilot-instructions.md), platform
  integrity), so a consumer that used this row would be binding to behaviour that was found and not
  read, and should say so.
- The behaviour on other Windows builds.

**Consequence for [D-19](#d-19) rule 6.** Rule 6 gave as its reason that Win32 does not normalise a
path with either the `\\?\` or the `\\.\` prefix. This measurement shows that holds for `\\?\` and not
for `\\.\`, which is normalised, so a child grafted onto a `\\.\` directory would be read as it should
be. The rule stays as it was implemented, wider than its reason, and errs toward not grafting; narrowing
it to the exactly-backslash verbatim form would change behaviour for a process whose current directory
has the device prefix, and no work is scheduled for that. The same run confirms that only a backslash-only `\\?\` suppresses
normalisation, the claim rule 6 made with no measurement recorded.

**Scheduled work:** none. `NR-5.2` to `NR-5.4` are withdrawn, as
[COMPLETED-CHECKLIST.md](COMPLETED-CHECKLIST.md#nr-52-withdrawn) says, with what would reopen them.

## The `security` module moved to win-authorization-sys

The engineer, 2026-10-18. `SecurityDescriptor`, `SecurityAttributes`, `SecurityInformation`, `AclState` and the
capture errors, which this crate held only because a request takes a descriptor, are in
[win-authorization-sys](../win-authorization-sys/DESIGN-NOTES.md), where Microsoft Learn files them.
This crate re-exports that crate as `security` and at its root, so no published path changed. Text above that
names `src/security.rs` or `src/security/sddl.rs` (including `NR-3.1`'s) describes where the code was when the
decision was made; the same files are now `crates/win-authorization-sys/src/security.rs` and
`security/sddl.rs`, moved with `git mv`.

## <a id="d-29"></a>D-29: `GetFileType` is its own entry, an unknown type is a failure only by the call's own report, and the value is carried as it came

**Decided by the engineer, 2026-10-18** ("build both of these now"), so that a caller can ask what kind of
handle an opened file is of an observer, and the opener need not return it. Sources read that day: the
`GetFileType` page (`ms.date` 12/05/2018).

**One entry, `QueryFileType`, in the shape of its neighbours:** a share of the handle ([D-32](#d-32)), a
`perform` that returns the call's outcome with the code snapshotted before anything else runs, and
`Clone`. Learn files the call under File Management Functions, with `CreateFile` and
`GetFinalPathNameByHandle`, so it is here.

**The value is carried as it came.** `FileType` holds what the call returned and names the five values the
page lists; a value outside them is held and printed as it is. The entry says nothing of what a type means
beyond the page's own description of each.

**An unknown type is a failure only when the call says so.** The page makes `FILE_TYPE_UNKNOWN` both a result
and a failure and tells them apart by `GetLastError`: `NO_ERROR` after a call that worked, the code after one
that failed. So the last error is cleared before the call, and an unknown type read with `NO_ERROR` is
`FileType::UNKNOWN`, while one read with any other code is that code, unaltered. The mapping is a function of
those two values, and is tested as one.

**Observed on 2026-10-18, Windows 11 build 26300, local NTFS, unelevated.** A disk file and a directory
returned `FILE_TYPE_DISK`. A volume opened with no access returned `FILE_TYPE_DISK`. `NUL` returned
`FILE_TYPE_CHAR`. The read end of an anonymous pipe returned `FILE_TYPE_PIPE`. An event, which is not a file,
made the call fail with `ERROR_INVALID_HANDLE`. A handle whose call worked and returned an unknown type was not
found, so the clearing of the last error is not exercised by any handle here; its sabotage is declared as
surviving, with that reason, and the clearing rests on the page.

**Scheduled work:** none in this crate. The consumer it was built for was `durable-ioring`'s `DI-14.12`, with
`WV-3.1` in [win-volume-sys](../win-volume-sys/CHECKLIST.md), which was withdrawn
([DI-D-73](../durable-ioring/DESIGN-NOTES.md#di-d-73)); the entry stays, since it reports `GetFileType`'s documented values.

## <a id="d-30"></a>D-30: `AnchoredPath` is public, so a crate that builds a request of its own takes the path contract from here

**What is decided.** [D-19](#d-19)'s graft lived in a crate-private type that `OpenFile` and `WatchDirectory` hold.
`AnchoredPath` is now public, with `new`, `given` and `win32`, so a crate that builds a request of its own and performs
it later on another thread fixes the meaning of a relative path by this contract and keeps no second copy of it. It is
the merge-or-delete decision of [D-9](#d-9), taken by the engineer on 2026-10-15 as "the enumeration crate consumes
the request crate's path contract and deletes its older copy" (`NR-3.8`, `PATH-1` in
[windows-file-enumeration-sys](../windows-file-enumeration-sys/CHECKLIST.md)); the type had to be reachable for that to
be done, and an `OpenFile` held in its place would not do, since it is not `Clone` or `Eq`, which the enumeration
crate's request is.

**What stays private:** `anchored_at`, which takes the directory as an argument so the rules can be tested without
process state, and `win32_terminated`, which returns a raw pointer for this crate's own calls. A consumer copies
`win32()` into a terminated string for its call.

**The contract is unchanged.** The rules, the one refusal (an interior NUL) and the two failures of reading the
current directory are [D-19](#d-19)'s and [D-27](#d-27)'s.

**The two path entries are built from one too** (`NR-6.3`). `OpenFile::from_anchored` and
`WatchDirectory::from_anchored` take an `AnchoredPath` the caller holds and are infallible, because the path's one
refusal and its one read of the current directory already happened when it was anchored. Neither anchors it again: a
caller that anchored its path when its own request was built has fixed what the
path names, and a second anchoring would read the current directory as it is when the open is built, which may be
later and elsewhere. `new` is `from_anchored` over `AnchoredPath::new`, so the two cannot start a request
differently. Both entries get it, so they stay one shape.

## <a id="d-31"></a>D-31: `FileIdInfo` has a typed entry, `QueryFileIdInfo`, which makes the one call and carries the two fields as the call wrote them

**Queued 2026-10-18 as `NR-3.10`** from the review of durable-ioring's `DI-M15`, which the engineer asked to have
made: [DI-15.1](../durable-ioring/CHECKLIST.md) reads a file's identity, durable-ioring forbids `unsafe`, and the
untyped [`QueryFileInformation`](src/query.rs) gives this class only as bytes. Learn files the call under File
Management, so the typed form is this crate's. Sources read that day: the `FILE_ID_INFO` page (`ms.date`
12/05/2018) and the `FILE_ID_DESCRIPTOR` page.

**It is still one entry per call.** [D-3](#d-3) forbids an entry that makes two calls; this one makes one,
`GetFileInformationByHandleEx` with `FileIdInfo`, into a `FILE_ID_INFO` on the stack, and reads the structure
that class writes. The untyped entry remains the catalogue's form of the call for every class. The class value
is taken from `FileInformationClass::ID`, so it has one definition.

**The values are carried as the call wrote them.** `FileIdInfo` holds `VolumeSerialNumber` as a `u64` and
`FileId` as its 16 bytes, not folded into an integer, since the page states no byte order. The page says the two
together "uniquely identify a file on a single computer", and that two handles are compared by combining them;
the entry says nothing more about what either means. `FILE_ID_INFO` is not part of the surface ([D-13](#d-13)).
A refusal is the call's code, unaltered.

**Observed on 2026-10-18, Windows 11 build 26300, local NTFS, unelevated.** Two opens of one file read the same
value; two files, a file and its copy, and a directory and its file read different identifiers with the same
serial; a hard link reads the value of the file it names; the identifier equals the one the
`FileIdExtdDirectoryInfo` listing gives for the file, and the one `windows-file-enumeration-sys` reports with a
required identity. A file opened with no access reads the same value as one opened to read. The 16 bytes opened
the file through `OpenFileById` with `ExtendedFileIdType`, where the `FILE_ID_DESCRIPTOR` page says of that
member "This is used on ReFS file systems". Refused: an event with `ERROR_INVALID_HANDLE`, `NUL` and an
anonymous pipe with `ERROR_INVALID_PARAMETER`, and a volume opened with no access with `ERROR_INVALID_FUNCTION`.
None of those codes is on the page, so the tests compare each with what the untyped entry reports for the same
handle, except the event's, which is named as [D-29](#d-29) names it. The `FILE_ID_INFO` page's requirements
table gives no supported client; the call worked on this one.

**Scheduled work:** none in this crate. [DI-15.1](../durable-ioring/CHECKLIST.md) is the consumer.
`windows-file-enumeration-sys` kept its private read of the serial, on the handle it owns, because this entry
would have duplicated that handle per enumeration ([D-4](#d-4)). [D-32](#d-32) removes the duplication, so the
enumeration crate can move its read onto this entry, with a test there that the two agree.

## <a id="d-32"></a>D-32: A request shares the caller's handle as a `SharedHandle`, and nothing in this crate duplicates one

**Decided by the engineer, 2026-10-18; lands with `NR-6`, which graduates `NR-inf.1`.** Asked whether
`CapturedHandle` was something wanted or something the crate happened to have, and whether a duplicating route
would be added if the crate were started now: "I suspected that this was just legacy. While we in theory have a
duty to possible clients, in practice, I want us to move forward as much as possible towards better interfaces
and this counts. We just didn't know we needed to do this ahead of time."

**What is decided.** Every entry that takes a handle holds a
[`SharedHandle`](../win-shared-os-owned-handle/README.md), and a clone of a request shares it. `CapturedHandle`,
its capture functions and its error types are deleted, and so is any route in this crate that duplicates a
handle. A caller holding only a borrow makes an owned handle itself, visibly: std's
`BorrowedHandle::try_clone_to_owned` (same access, not inheritable) or `win-handles-sys`'s
`duplicate_handle_with_access` (narrower access), then `SharedHandle::from`.

**Why nothing of `CapturedHandle` is kept.** Each thing it did came from duplicating:

- *A lifetime of the request's own*, so a request is never left naming a handle its originator closed.
  `SharedHandle`'s count gives that with no new handle.
- *[D-7](#d-7)'s refusals*, made before `DuplicateHandle`. `INVALID_HANDLE_VALUE` was refused because
  duplicating it yields a process handle; with no duplication there is nothing to yield, and std's types catch a
  failed open before an `OwnedHandle` exists. Null and `-1` are values std documents an `OwnedHandle` may hold;
  given one, an entry makes its call and reports the call's code ([D-10](#d-10)), and a refusal ahead of it would
  rest on nothing the call's page says. A pseudo-handle reaches an `OwnedHandle` only through an `unsafe`
  constructor whose contract it breaks, and no entry here takes a thread or token handle, so none could succeed
  wrongly with one.
- *A fallible `try_clone`*, because duplicating can fail. Sharing cannot.
- *A non-inheritable handle*, so capturing never widened what a child process reaches. Sharing makes no handle,
  so it widens nothing.

**What is unchanged.** [D-4](#d-4)'s measured state-sharing: shared or duplicated, a request names the caller's
kernel object, and a request is not an independent enumeration.

**What a caller sees differently.** The handle stays open until the last holder drops, and an `OwnedHandle` can
be taken back (`SharedHandle::try_into_owned`) only when no request still holds a clone.

**Two entries are not `Clone`, and the reason is not the handle.** `OpenFile` and `OpenFileByIdentifier` carry
`SecurityAttributes`, whose copy allocates and can fail, so they keep `try_clone` for that; it shares the handle
and copies only the attributes, and its error is the attributes' own (`SecurityCaptureError`). Every other
handle-taking entry derives `Clone`.

**`new` keeps `impl Into<SharedHandle>`** (settled by `NR-6.2`). A `SharedHandle` converts to itself, so a caller
holding one passes a clone, and an `OwnedHandle` or a `File` is given as it is, with no `SharedHandle::from` at
every call site; taking `SharedHandle` would add that ceremony and exclude nothing. `OpenFile::with_template`
takes an `Option<SharedHandle>`, since `None` would leave a generic parameter with nothing to infer from.

**Scheduled work:** none left in this crate for the decision itself: `NR-6.1` and `NR-6.2` are complete
([COMPLETED-CHECKLIST.md](COMPLETED-CHECKLIST.md)).

## <a id="d-33"></a>D-33: `FileBasicInfo` has a typed entry, `QueryFileBasicInfo`, shaped as D-31's, which carries the five fields as the call wrote them

**Built as `NR-6.4`** ([COMPLETED-CHECKLIST.md](COMPLETED-CHECKLIST.md#nr-64-basic)) for `windows-file-enumeration-sys`,
whose check that an opened root is a directory reads the attributes under `forbid(unsafe_code)`, and so cannot read
them out of the untyped entry's bytes. Source read 2026-10-19: the
`FILE_BASIC_INFO` page (`ms.date` 2024-08-02).

**The shape is [D-31](#d-31)'s, and so is the reason it is still one entry per call.** `QueryFileBasicInfo` makes
one `GetFileInformationByHandleEx` call with `FileBasicInfo`, the class taken from `FileInformationClass::BASIC`,
into a `FILE_BASIC_INFO` on the stack. It holds a `SharedHandle` and is `Clone` ([D-32](#d-32)). The untyped entry
remains the catalogue's form of the call for every class.

**The values are carried as the call wrote them.** `FileBasicInfo` holds `CreationTime`, `LastAccessTime`,
`LastWriteTime` and `ChangeTime` as `i64` and `FileAttributes` as its raw `u32`. The page gives the times "in FILETIME
format", 100-nanosecond intervals since 1601-01-01 UTC, and the attributes as the file attribute constants; nothing
here converts a time to another type or names an attribute bit, and `FILE_BASIC_INFO` is not part of the surface
([D-13](#d-13)). A refusal is the call's code, unaltered.

**Observed 2026-10-19, Windows 11 build 26300, local NTFS, unelevated.** Three times set through std's `set_times`
read back exactly in their fields, with the change time after them. A directory reported the directory attribute and
a file did not; a read-only file reported the read-only attribute. A file opened with no access answered (attributes
`0x20`), and so did an anonymous pipe (`0x80`), which `FileIdInfo` refuses. Refused: an event with
`ERROR_INVALID_HANDLE`, `NUL` with `ERROR_INVALID_PARAMETER`, and a volume opened with no access with
`ERROR_INVALID_FUNCTION`. None of these outcomes is on the page, so the tests compare each handle's outcome with the
untyped entry's for the same handle, except the event's code, which is named as [D-29](#d-29) names it. One
observation about NTFS rather than this entry, recorded because a test depends on it: a time set explicitly through a
handle did not move when the file was later written through that same handle, so the test of the write time writes
through a handle of its own.

**Scheduled work:** none in this crate. The consumer it was built for is `windows-file-enumeration-sys`.

## <a id="d-34"></a>D-34: `QueryFileInformation` also performs into a buffer the caller owns, and refuses only the alignment a structure's page requires of it

**Amended 2026-10-20: a buffer whose address is off the boundary the class's structure page requires is refused
before the call.** It had refused nothing, on a reading of the call's page alone; see "Alignment" below.

**Built as `NR-6.5`** ([COMPLETED-CHECKLIST.md](COMPLETED-CHECKLIST.md#nr-65-into)) for `windows-file-enumeration-sys`,
whose refill reads a directory batch after batch
and, through [`perform`](src/query.rs), would allocate once per call. Source read 2026-10-19: the
`GetFileInformationByHandleEx` page (`ms.date` 2018-12-05).

**What is decided.** `QueryFileInformation::perform_into(&self, &mut AlignedBuf) -> Result<(), PerformIntoError>`
makes the same one call into the caller's buffer and allocates nothing; `perform` is now an allocation followed by
the same call, so the call has one site. The buffer's length is the size given, and the request's `capacity` is not used by this
form. The buffer is not cleared first: bytes the call does not write keep what the caller left, which differs from
`perform`'s buffer, which starts zeroed. After a failure the contents are whatever the call left.

**What is refused rests on the pages, and is the alignment three of them require.** The call's page gives
`lpFileInformation` as "a pointer to the buffer that receives the requested file information" and `dwBufferSize` as
"the size of the *lpFileInformation* buffer, in bytes", a `DWORD`, and states no alignment itself. It also gives, for
each class, "the corresponding data structure types for use with this function" as the `lpFileInformation` type,
and those structures' pages are the contract for what the buffer holds. Read 2026-10-20, all `ms.date` 2018-12-05:

- **Alignment.** `FILE_STREAM_INFO`, `FILE_ID_BOTH_DIR_INFO` and `FILE_FULL_DIR_INFO` each "must be aligned on a"
  "(8-byte) boundary"; no other structure the call's page lists says so. So for `FileStreamInfo`,
  `FileIdBothDirectoryInfo`, `FileIdBothDirectoryRestartInfo`, `FileFullDirectoryInfo` and
  `FileFullDirectoryRestartInfo`, a buffer whose **address** is not on an 8-byte boundary is
  `PerformIntoError::Misaligned`, with no OS code, and the call is not made. The address is checked and not the
  stated alignment (`AlignedBuf::align`), because the address is what the pages constrain: a check of the stated
  alignment would refuse a buffer stated at less that lies on the boundary, which the pages allow. The table is one
  constant, `REQUIRED_ALIGNMENT`, checked by a pure function and tested in memory in both directions; `perform`
  allocates at 8, which every entry divides (asserted at build), so it shares the unchecked call.
  The engineer, 2026-10-20, on review of the release branch, which found the structure pages' requirement: "this
  is a clearly documented alignment requirement. Refuse and ensure we also document." **Observed, and not a
  check:** the two `FileIdExtdDirectory` classes, whose `FILE_ID_EXTD_DIR_INFO` page states no alignment, fail with
  `ERROR_NOACCESS` on a misaligned buffer; that stays a measurement, recorded where `perform`'s 8-byte alignment is
  explained.
- **A length above `u32::MAX`** is given as `u32::MAX`, its first that many bytes, as `perform` already gives an
  over-large capacity. That is not a refusal: the buffer is writable for at least the size stated.
- **An empty buffer** is passed as one, and the call answers it; the tests compare its code with `perform`'s for a
  capacity of zero.

**Scheduled work:** none in this crate. The consumer it was built for is `windows-file-enumeration-sys`.
