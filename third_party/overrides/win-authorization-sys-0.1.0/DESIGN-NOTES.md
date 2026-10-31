# Design notes: win-authorization-sys

## WA-D-1: the crate is the Learn section, and its first code was moved in, not written

The engineer, 2026-10-18: the crate is named `win-authorization-sys`, and the function and the types that
belong move with it. It is the crate for the Authorization functions, placed by the Microsoft Learn section that documents
them: `GetSecurityInfo`, `ConvertStringSecurityDescriptorToSecurityDescriptor` and the descriptor calls are all
listed in the Win32 reference under *Security and Identity > Authorization*.

`SecurityDescriptor`, `SecurityAttributes`, `SecurityInformation`, `AclState` and the capture errors were the
`security` module of [windows-namespace-request-sys](../windows-namespace-request-sys/DESIGN-NOTES.md), which
kept the code only because a request takes a descriptor, and the files were moved with `git mv` so their history
follows. From the release of that crate that depends on this one, it re-exports this crate at `security` and at the
root, so no path it published changes. A release of it from before then carries its own copy, whose types are
distinct from these.

Two things changed in the move and nothing else. `SecurityCaptureError::into_source` was `pub(crate)` and is
`pub`, since the request crate is now another crate and reports a failed copy under its own failure. And the
tests that open a file through a request, which are the request's behaviour and not the descriptor's, stayed
there (`security_open_tests.rs`).

## <a id="wa-d-2"></a>WA-D-2: `get_security_info`, and what was measured

**Amended by [WA-D-3](#wa-d-3): the safe entry passes only the kinds measured to read any handle.**

`GetSecurityInfo` is `get_security_info(handle, ObjectType, SecurityInformation)`, returning an owned
`SecurityDescriptor` (`DI-14.8.2`). Windows allocates the descriptor with `LocalAlloc`; it is captured into the
owned self-relative form and the allocation freed on every path, by the same guard `from_sddl` uses. The call
returns its error as its return value and not through `GetLastError`, so the code is carried as it came,
including a code of 2^31 or more (`SecurityCaptureFailure::ReadSecurity`).

Measured on Windows 11 build 26300, local NTFS, and none of it a ruling:

- **The kind does not change what a handle reads.** An event read as `KERNEL_OBJECT`, as `FILE` and as
  `WINDOW_OBJECT` gives equal descriptors. `UNKNOWN` is refused with `ERROR_INVALID_PARAMETER`. The kinds are
  all named, with a raw constructor, and none is removed; the safe entry passes only some of them
  ([WA-D-3](#wa-d-3)).
- **An object with no descriptor reads as a descriptor with no parts**, not as an error and not as a NULL DACL.
  An event made with default security and no name reads that way (the result is the 20-byte header), which is
  why the test for a kernel object uses a named event given a descriptor. The same result is a selection of
  nothing (`SecurityInformation::NONE`), which succeeds.
- **A part the handle cannot read is Windows' refusal.** A file opened with the right to read attributes only is
  refused the DACL with `ERROR_ACCESS_DENIED`, and a SACL on a handle without `ACCESS_SYSTEM_SECURITY` is
  refused the same way, before the privilege is looked at. Nothing retries with less.
- **A file's stored DACL has its generic rights mapped to specific ones**, so a descriptor given with `GA` reads
  back as `FA` for a file and `0x1f0003` for an event; a protected DACL reads back protected. A DACL given as
  auto-inherited (`D:PAI`) keeps its entries and loses the flag: it reads back as `D:P`.

A null descriptor on success is guarded and not reachable by any test, since Windows documents a descriptor on
success; it is the one error edge here with no test, and is said so where it is defined.

## <a id="wa-d-3"></a>WA-D-3: the safe `get_security_info` passes only the kinds measured to read any handle; `get_security_info_unchecked` takes every kind

A `BorrowedHandle` establishes that a handle is open, not what it names, and the `GetSecurityInfo` page (read
2026-10-19) reads `ObjectType` as the kind of the object the handle names without saying what a mismatch does.
Measured on Windows 11 build 26300: `LMSHARE` with a file handle writes through a null pointer and the process
faults, so a safe function that passed every kind could crash from safe code. The evidence is a test that makes
the call in a child process and expects the fault.

**What is decided.** `get_security_info` keeps its signature and passes the kinds `ObjectType::reads_any_handle`
names -- `FILE`, `KERNEL_OBJECT` and `WINDOW_OBJECT`, under which Windows was measured reading any handle's
security ([WA-D-2](#wa-d-2)), and `UNKNOWN`, which Windows refuses with its own code. Any other kind is refused
before the call as this crate's own narrowing, `SecurityCaptureFailure::UnsupportedObjectType`, with
`ERROR_INVALID_PARAMETER` supplied by the crate, so it cannot be read as Windows' refusal.
`get_security_info_unchecked` is `unsafe`, takes every kind, and requires the caller to name the kind of the
object the handle names. No kind is removed from `ObjectType`, so nothing is narrowed for a caller who knows what
the handle is.

**Why not make the one function `unsafe`.** Its callers read a file's security from crates that forbid `unsafe`
(`win-durable-ioring`), and every kind they need is one that reads any handle.

## <a id="wa-d-4"></a>WA-D-4: a self-relative descriptor is rebuilt, not copied by its reported length

`SecurityDescriptor::capture` took a self-relative descriptor by copying `GetSecurityDescriptorLength` bytes. That
length counts the parts, and the self-relative format locates each part by an offset, so parts may share bytes or
have space between them: a descriptor whose owner and group share one SID reports more bytes than it occupies, and
one with a gap before its DACL reports fewer than the DACL ends at. Both validate. The copy then read past the
source, or named a part outside itself.

**What is decided.** A self-relative descriptor is taken apart with `MakeAbsoluteSD`, which reads each part where
its offset says, into buffers this crate owns, and made self-relative again with `MakeSelfRelativeSD`, the path an
absolute descriptor already took. The result is the compact form Windows writes, so it may be shorter than the
source. Two failures are new, `SizeAbsolute` and `MakeAbsolute`, for the two calls. The tests lay both layouts out
against a guard page, so a read past the source faults rather than going unseen.
