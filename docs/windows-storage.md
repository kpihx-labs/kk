# Windows Protected Storage Boundary

This is a design/blocker record, not Windows support or execution evidence.
Full read-only research is retained in peer `ses_e7f3b4f9affeqBl18cmaXOurUG`.
No dependency was approved or installed by that research. Product/native status
remains in [platforms](platforms.md).

## Required Invariants

| Boundary | Required behavior |
| --- | --- |
| Private creation | Account SID plus OS-trusted SYSTEM/Administrators; creation-time protection or proven private inheritance |
| Inspection | Actual opened handle, owner, absent/NULL DACL distinction, supported ACEs and inheritance; no string-equality ACL proof |
| Namespace | Retained ancestor handles, reparse refusal and demonstrated replacement exclusion, not a final-component-only check |
| Publication | Authorized source object, pinned destination, atomic no-replacement; no existence-check/rename race |
| Durability | Data flush and namespace publication have separate evidence; visibility alone is not a durable commit |

The existing safe-Rust constraint remains. An owned unsafe side crate would not
resolve the boundary and is not an authorized workaround.

## Inspected Safe APIs

| Exact crate | Useful scope | Unresolved issue |
| --- | --- | --- |
| windows-namespace-request-sys 0.5.1 + win-authorization-sys 0.1.0 | Owned descriptor at CreateFileW CREATE_NEW; handle-bound descriptor capture; absent/NULL ACL distinction | No directory creation or source-handle rename; typed SID/ACE parser absent; recent publication, not an established audit |
| cap-primitives/cap-fs-ext 4.0.3 | Scoped directory operations/nofollow | Does not establish the complete ACL and source-handle nonclobber contract |
| winx 0.36.4 | Safe reopen_file with access/share/flags | No complete creation/ACL/rename chain |
| windows-permissions 0.2.4 | Handle-based GetSecurityInfo | Old publication; NULL-DACL inspection panic and truncated unknown bits found in published source |
| windows-acl 0.3.0 | Handle-based ACE enumeration | Old publication; incomplete exposed owner/control/null-DACL inspection |
| winsafe 0.0.29 | Safe-shaped directory/file/token APIs | Complete owned descriptor chain unproven; token-buffer ownership concern needs maintainer resolution |
| fs_at 0.2.1 | Relative opens/creation | Windows backend forces delete sharing and strips OBJ_DONT_REPARSE |

Source-bound publication needs a maintained safe interface equivalent to
`SetFileInformationByHandle` with `FileRenameInfo`, `ReplaceIfExists = FALSE`,
the authorized source handle and pinned destination directory. The inspected
`tempfile::persist_noclobber` uses path-based MoveFileExW on Windows; it does not
prove that stronger source binding. Neither does ordinary `std::fs::rename`.

## Acceptance Before Enabling Production

- Separate-account creation/read refusal, including before the first write.
- Foreign owner, broad/NULL DACL, unsafe inherit-only and unknown ACE refusal.
- File and nested-directory inheritance checked through resulting handles.
- Another process cannot replace any retained ancestor; metadata-only handles
  are a negative control, not an assumed namespace lock.
- Reparse/junction and stage-path replacement races cannot escape authorization.
- Conflicting destination files/directories/links remain unchanged.
- Process-kill recovery, injected flush failures and VM power-cut behavior are
  distinct gates; unsupported volumes fail closed without a weaker fallback.

The targeted advisory inspection used RustSec revision
`99670cfc0bc574433034b75876a4b791ccaa2efb`; it found no named advisory directory for
the candidates, not proof of soundness or a full resolved-transitive audit.

## Modular Native Boundary

Private identity creation/load and private profile storage remain unavailable on
Windows. Public certificate import is separate: `Peer::load` uses the safe Rust
standard library's `OpenOptionsExt`, requests `FILE_FLAG_OPEN_REPARSE_POINT` with
read-only sharing, inspects the opened handle's regular-file/reparse attributes,
and enforces the certificate byte limit before DER parsing. It neither reads nor
creates a private key, validates an ACL, authenticates a peer's origin, nor proves
an ancestor namespace immutable. Fingerprint verification remains mandatory.
Its Windows branch requires native execution; Linux tests are not that evidence.

The Windows-only `platform_foundation` sharing test requires `ERROR_SHARING_VIOLATION` (32) while an exclusive reader or a shared writable handle is retained, then unchanged public import after closure. It needs no symlink privilege, does not test DACL protection and has not been executed on Windows for this source.

The next binding must expose the following complete safe public interfaces,
not just similarly named Win32 operations:

| Required binding | Why the inspected alternative is insufficient |
| --- | --- |
| Parent-handle-relative directory/file creation with owned creation-time security descriptor and no reparse traversal | `winsafe::HFILE::CreateFile` supplies a safe owned guard, but `CreateDirectory` remains path-only. Its attributes require `&'a mut winsafe::SECURITY_DESCRIPTOR`; the owned descriptor/raw pointer from `win-authorization-sys::from_sddl` has no proven safe interoperability chain. `fs_at` removes `OBJ_DONT_REPARSE` and permits delete sharing |
| Handle-bound owner/DACL capture with typed SID/ACE/control parsing, absent versus NULL ACL distinction and unknown-ACE refusal | `get_security_info` supplies one owned descriptor snapshot but not the complete typed owner/ACE policy path; `windows_acl` handle enumeration is a separate snapshot, not an established single-snapshot validation chain |
| Source-handle `FileRenameInfo`, pinned target directory and no replacement | No inspected safe public dependency establishes this publication contract; path-based rename cannot substitute |
| Named-pipe client security-token identity bound to the connected client, without PID-reuse ambiguity | `get_named_pipe_client_process_id` supplies a PID, not an authenticated SID; token/impersonation binding is unresolved |

Retained real-access ancestor handles without DELETE sharing can exclude filesystem
rename/delete, but do not yet prove DOS-device/volume-name mapping is pinned.
`DefineDosDevice` shadowing remains unresolved: a canonical path is not a binding
to the namespace used by a later path-only creation call.

Dependencies may contain reviewed internal unsafe code behind a sound safe API.
A new local unsafe shim, an unsafe side crate owned by K&K, a permissive DACL or
path-check fallback is not an authorized substitute. No dependency is requested
merely to turn an unavailable capability into a successful empty operation.
The acceptance cases above remain mandatory once a complete binding is available;
the current non-Unix refusal tests assert no state is created, not native support.

## Primary Sources

- [Microsoft file security and inheritance](https://learn.microsoft.com/en-us/windows/win32/fileio/file-security-and-access-rights)
- [Windows rename structure](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info)
- [MoveFileExW semantics](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)
- [Owned file creation implementation](https://docs.rs/windows-namespace-request-sys/0.5.1/src/windows_namespace_request_sys/open.rs.html)
- [Handle descriptor capture](https://docs.rs/win-authorization-sys/0.1.0/src/win_authorization_sys/security/query.rs.html)
- [tempfile Windows implementation](https://docs.rs/crate/tempfile/3.27.0/source/src/file/imp/windows.rs)
- [winsafe CreateDirectory](https://docs.rs/winsafe/latest/winsafe/fn.CreateDirectory.html)
- [Owned authorization descriptors](https://docs.rs/win-authorization-sys/latest/win_authorization_sys/)
- [fs_at backend source](https://docs.rs/fs_at/latest/src/fs_at/lib.rs.html)
- [Named-pipe client PID API](https://docs.rs/win-pipes-sys/latest/win_pipes_sys/fn.get_named_pipe_client_process_id.html)
- [Rust Windows OpenOptionsExt](https://doc.rust-lang.org/std/os/windows/fs/trait.OpenOptionsExt.html)
- [Microsoft CreateFile flags and sharing](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)
