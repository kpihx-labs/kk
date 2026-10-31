# windows-namespace-request-sys

Owned, marshalable parameter sets for synchronous Win32 namespace calls.

**Windows only.** Every public item is behind `cfg(windows)`; the crate builds to
an empty shell on other platforms.

## Why

Win32's namespace and metadata surface -- opening, querying, closing -- is
synchronous-only. There is no overlapped `CreateFileW`, no overlapped
`GetFileInformationByHandleEx`. A call that blocks on a dead network path blocks
the thread that made it, and on a shared thread that is somebody else's problem
too.

This crate makes such a call **capturable as a value**: an owned parameter set
built on one thread and executed faithfully on another. It schedules nothing --
it is the catalogue-plus-faithful-execution layer, testable with no ring, no
pool, and no async anywhere near it.

## What a request is, and is not

A request carries call **parameters**, not the thread state the call runs under.
Impersonation and the rest belong to
[windows-thread-ambient-sys](../windows-thread-ambient-sys/README.md), which this
crate does not depend on. The two are siblings rather than a stack: a request can
be executed with no captured context at all, and a context is useful to work that
never opens a file. Whoever owns both pairs them at the submission site.

A request also chooses **no delivery model**. An opened handle comes back plain
and unassociated, because associating it with a completion port irreversibly
forecloses `IoRing` use of it, and that choice belongs to a layer that knows the
handle's destination.

## Why the `-sys` suffix

Not in the usual sense. A `-sys` crate elsewhere in the ecosystem is normally
raw FFI declarations with no abstraction over them, and by that reading this
crate is misnamed: it declares almost no FFI of its own and takes its
declarations from [`windows-sys`](https://crates.io/crates/windows-sys).

In this workspace the suffix marks a **layer**, not a linking strategy: a
`windows-*-sys` crate makes an existing Win32 API memory-safe **without adding
policy**, and a crate that decides something Win32 has no equivalent of drops
the suffix. That is why `windows-waitable-queues` carries no `-sys` -- it picks
a slot protocol and an overflow policy -- while this crate does, despite
wrapping a great deal of `unsafe`. Everything above is the suffix being earned:
it schedules nothing, chooses no delivery model, and reports raw Win32 outcomes
without normalising them.

The convention is stated for the whole workspace in the repository's
[README](../../README.md#crate-naming).

## A path is copied; a handle is shared

Several entries take a handle rather than a path, and a request holds a
[`SharedHandle`](../win-shared-os-owned-handle/README.md): a share of the
caller's handle, not a new one, and a clone of the request shares it again.
Nothing in this crate duplicates a handle. The distinction is easy to get
backwards: a path is a value and is copied, while a handle is a reference to a
kernel object.

A request is therefore self-contained with respect to **lifetime** -- the handle
stays open while any holder keeps its share, so a request cannot be left naming a
handle its originator closed -- and **not** isolated with respect to **state**. A
request names the caller's kernel object, so an enumeration through it continues
the caller's rather than starting its own, and single-shot metadata queries
disturb nothing. An independent traversal needs a fresh open. A caller holding
only a borrow makes an owned handle itself, visibly, before handing it over.

## Example

Build on the submitting thread, where a failure is still the caller's to see,
then use the result on a worker that saw none of the inputs:

```rust
use std::fs;
use std::thread;

use windows_namespace_request_sys::{FileType, OpenFile, QueryFileType};
use wtf_string::Wtf16String;

let path = std::env::temp_dir().join(format!("wnrs-readme-{}.tmp", std::process::id()));
fs::write(&path, b"example")?;

// Built here rather than on the worker: a relative path is fixed to the process
// current directory now, which is shared mutable state that any thread can
// change in between. This one is absolute, so it is handed over as given.
let text = path.to_str().expect("a temporary path is valid UTF-8");
let open = OpenFile::new(Wtf16String::from(text))?;
assert_eq!(open.win32_path().to_string_lossy(), text);

// A handle is shared, not duplicated: the request holds the caller's handle, so
// it stays open for the worker however the caller lets go.
let query = QueryFileType::new(fs::File::open(&path)?);

let kind = thread::spawn(move || query.perform())
    .join()
    .expect("the worker did not panic")?;

assert_eq!(kind, FileType::DISK);
# fs::remove_file(&path)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Round-one entries

One entry per Win32 call. The list is audited from three real consumers -- this
repository's file watcher and enumeration crates, and `MikeGrier/Globazog-rs` --
rather than chosen by taste:

| Win32 call | Type |
|---|---|
| `CreateFileW` | `OpenFile` |
| `OpenFileById` | `OpenFileByIdentifier` |
| `FindFirstChangeNotificationW` | `WatchDirectory` |
| `CloseHandle` and variant close routines | `CloseRequest` |
| `GetFileInformationByHandleEx` | `QueryFileInformation`; `QueryFileIdInfo` and `QueryFileBasicInfo` read its `FileIdInfo` and `FileBasicInfo` classes as values |
| `GetFileInformationByHandle` | `QueryFileInformationByHandle` |
| `GetFileType` | `QueryFileType` |
| `GetFinalPathNameByHandleW` | `QueryFinalPath` |
| `GetVolumeInformationByHandleW` | `QueryVolumeInformation` |
| `GetFullPathNameW` | `ResolveFullPath` |

Deletion, rename, directory creation, attribute setting, link creation, and the
`FindFirstFileExW` family are **deliberately** out of round one: no audited
consumer calls them. [DESIGN-NOTES.md](DESIGN-NOTES.md) records the full list so
a considered omission is distinguishable from an unexamined one.

## Volume and disk operations are not here

Device controls against volumes and disks, and the virtdisk dependency query, are not namespace
operations. They are in [win-volume-sys](../win-volume-sys/README.md), which is where this crate's
storage-locality work was first built and from where it was moved.

## Testing your own code against these entries

Every entry implements `Request` (or `ConsumingRequest`, for the one-shot
close), so a consumer's code can be written against "a request that produces
`T`" and exercised in that consumer's own tests with a fake -- no filesystem, no
network path, no device that may or may not be present.

Two traits rather than one because the distinction is real: an open is a
parameter set that may be performed repeatedly and takes `&self`, while a close
is consumed by performing it, which is what makes closing twice impossible.

## Status

The round-one catalogue is complete and its acceptance pass covers both
operation coverage (every audited call site re-expressed) and scenario coverage
(a request built on one thread and executed on another under a captured
context, many requests across concurrent workers from one shared capture, and a
handle opened by one request carried into a later one).

Design decisions are recorded in [DESIGN-NOTES.md](DESIGN-NOTES.md). Published
on crates.io as
[`windows-namespace-request-sys`](https://crates.io/crates/windows-namespace-request-sys).
