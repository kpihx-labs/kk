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

## Primary Sources

- [Microsoft file security and inheritance](https://learn.microsoft.com/en-us/windows/win32/fileio/file-security-and-access-rights)
- [Windows rename structure](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_rename_info)
- [MoveFileExW semantics](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)
- [Owned file creation implementation](https://docs.rs/windows-namespace-request-sys/0.5.1/src/windows_namespace_request_sys/open.rs.html)
- [Handle descriptor capture](https://docs.rs/win-authorization-sys/0.1.0/src/win_authorization_sys/security/query.rs.html)
- [tempfile Windows implementation](https://docs.rs/crate/tempfile/3.27.0/source/src/file/imp/windows.rs)
