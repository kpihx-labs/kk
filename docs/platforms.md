# Platform Scope

Use [current evidence](progress.md) for source/artifact bindings and
[TODO.md](../TODO.md) for delivery status. A native build or selected gate does
not establish full workflow, installer or physical-device support.

The table describes the qualified prerelease baseline. The current integration
batch still needs new hosted and exact-artifact gates; old passes do not qualify it.

| Environment | Recorded evidence | Not implied |
| --- | --- | --- |
| Linux x86_64 | Host/isolated guests, integrated CLI/UI/storage/IPC, owned-display codecs and exact signed prerelease install/update/rollback | Physical devices, arbitrary filesystems or independently audited deployment |
| Linux aarch64 | Native hosted product checks and archives in tagged Verify | Local ARM-device or physical-capture acceptance |
| Windows native | Windows 11 Pro VM protected storage, identity exclusion, no-replace publication, authenticated same-owner named pipe/timeout recovery and bounded stdio; tagged Native gates pass | A Windows installer/archive or every Linux workflow on Windows |
| macOS Intel/ARM | Tagged native build, warning-free lint, smoke and selected platform/storage/IPC/history/publication gates pass | Full browser/device coverage or exact signed macOS installation/update/rollback |
| WSL2 | No separate executed qualification recorded | Native Windows ACL/pipe support or safe use of Windows-mounted private storage |
| Mobile browser | Responsive static and rendered layout checks | Physical phone, keyboard, permission chooser or mobile backend support |

## Contributor Boundaries

[`native.yml`](../.github/workflows/native.yml) owns the selected native matrix;
[`check.yml`](../.github/workflows/check.yml) owns product/release gates. Read
actual job steps/results rather than keeping a duplicated test/cfg inventory.
Compilation, expected unsupported refusals and execution are distinct outcomes.
Unsupported protection models fail closed without weaker storage or TLS.

Private identity/profile IO must bind operations to validated opened objects,
refuse unsafe owners, modes/ACLs, links/reparse points and partial/mismatched keys,
and never regenerate existing missing or damaged state. Public certificate import
does not establish protected private storage or authenticate certificate origin.

On Unix, retained no-follow directory descriptors anchor private files. Trusted
root-owned OS aliases and sticky temporary directories are distinct from
user-controlled aliases or writable nonsticky ancestors. IPC details:
[contact inbox](node.md#shared-local-ipc-filesystem-contract).
On Windows, native protection and same-owner pipe identity need their own tests;
Unix permissions and a client PID are not substitutes for ACL/token checks.

## Filesystem And Durability Scope

Linux/macOS publication stages and publishes on the same filesystem, with
descriptor-relative no-replace operations, file sync and directory sync. Linux
uses `RENAME_NOREPLACE`; rustix maps Darwin to `RENAME_EXCL`. No check-then-rename
fallback is permitted. Failure after rename can leave complete visible output
while reporting failure; cancellation cannot recall an authorized publication.

Tests qualify their actual local filesystem, not network/FUSE/cloud/removable
storage. Sync calls do not establish physical power-loss protection or macOS
`F_FULLFSYNC`. Cooperating writers, stable owned private paths and exclusive
resource ownership remain assumptions; malicious same-account code is excluded.
Database lease versus SQLite locking: [database ownership](database-ownership.md).

WSL uses the Linux backend. Keep private state on a Linux filesystem with actual
owner/mode/no-follow semantics; a displayed 0700 mode on a Windows mount is not
equivalent evidence.

## Runtime Prerequisites

| Use | Prerequisites |
| --- | --- |
| CLI / bundled UI | Native executable; no Node/Python application runtime |
| Native media | GStreamer plugins and applicable OS lifecycle/output tools; Linux uses `prlimit`/`setpriv` |
| Browser media | Capture/WebCodecs APIs and explicit browser/OS permission |
| Build / offline rebuild | Pinned Rust toolchain and required C/build tools |
| Lab | Separate libvirt/QEMU/UV/tooling; not an application dependency |

`kk media doctor` probes plugins without opening devices. X11 does not bypass
Wayland permission. See [media](media.md), [installers](installers.md),
[macOS installer internals](macos-install.md) and [distribution](distribution.md).
