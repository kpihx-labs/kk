# Platform Scope

| Environment | Executed evidence | Not implied |
| --- | --- | --- |
| Linux x86_64 | Host/isolated Fedora and Mint; CLI, storage, rooms, copies and native virtual-display tests | Physical radio/devices or an independently audited deployment |
| Linux aarch64 | Native hosted CI runs the product checks and builds archives | A local ARM VM or physical capture test |
| Windows native | `cfa8145` foundation and actual PowerShell refusal pass after earlier `a60aab6`/`114705b` passes; protected storage remains refused | No full-feature Windows archive or product qualification |
| macOS native | `cfa8145` selected gates pass both architectures, including five probes of all 17 consent cases. Earlier `114705b` ARM and `1f846ef` Intel failures remain recorded, not reclassified as passes | No full-product support or physical capture claim from compilation or a partial run |
| WSL2 | Not an executed qualification | Not native Windows support |
| Mobile browser | Narrow-screen light/dark layout fixture | No physical phone, keyboard or mobile-backend claim |

This is a Linux software delivery. The exact final artifact and publication state
remain in [TODO.md](../TODO.md) and the [verification record](progress.md).
Unsupported protection models fail closed instead of weakening storage or TLS.

## Native Foundation CI

[`native.yml`](../.github/workflows/native.yml) is a nonpublishing lane for `windows-2022` / `x86_64-pc-windows-msvc`, `macos-15-intel` / `x86_64-apple-darwin`, and `macos-15` / `aarch64-apple-darwin`.
It uses the existing checkout action pin and reads the Rust channel from [`rust-toolchain.toml`](../rust-toolchain.toml), rather than maintaining another version. The [hosted runner labels](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) identify native hosts, not Linux cross-compilation. macOS fixtures use a resolved physical temporary root; production `NOFOLLOW` and ancestor checks remain unchanged.

**Execution status is source-bound.** Earlier Windows foundation passes do not qualify another changed tree; earlier macOS runs stopped at compile/stdio blockers.
The `a60aab6` selected pass does not explain every historical failure. Linux cannot establish Windows/macOS behavior. Compiler, lint, runtime or linker failures are blockers, not permission to skip a step or claim product support.

| Gate | What it establishes if it passes | What it does not establish |
| --- | --- | --- |
| Locked fetch, offline locked all-target build | Native dependency resolution and compilation of binary, library, tests and examples | Execution of compiled test/example targets |
| Offline locked all-target Clippy, warnings denied | Native target lint acceptance, without suppression | Runtime safety or platform qualification |
| Explicit binary smoke | Actual native executable runs `--version`, `--help`, and refuses an unknown argument with exit 2 | Initialized profiles, networking, UI or media |
| Offline locked foundation tests | Public digest parsing; Unix protected identity and inspection, or exact non-Unix refusals; Linux/macOS publication or unsupported-target refusal | Full integration/unit suites, cross-device interoperability or physical devices |
| Windows-only PowerShell refusal | The actual adapter exits 3 and explains that Windows releases are not shipped; valid synthetic selectors exercise the unsupported contract | A working Windows installer or successful installation |
| macOS-only selected integrations | File/envelope transfers and nonclobber; `ipc_long_paths` real contact/room/HTTP paths; `history_keys` protected binding/recovery regressions | Cross-device exchange, crypto redesign or complete product qualification |
| macOS-only selected library tests | `files::`, shared IPC ownership/recovery, room reservations, publication consent/revocation/cancellation and grant-lock regressions; explicit SQLite ancestor-symlink refusal with committed state preserved; full store and [actor-lease ownership](database-ownership.md) regressions | Unselected library suites, complete foreground lifecycle, native UI or capture qualification |
There are no uploads, installers, release calls, signing credentials or write
permissions in this lane. It does not change the existing release job's gates.
The always-run job summary reports scope even if an earlier gate fails; it is
not a success signal. Expected `UnsupportedPlatform` results are assertions of
the present contract, never successful product operations.

### Existing Test Gating Inventory

The native lane selects the tests above, not unfiltered `cargo test`. Windows
runs `platform_foundation` plus the PowerShell refusal step; build/lint compile applicable
code. Guards below do not imply that every Unix suite is qualified on macOS:

| Existing Rust integration suites | Source guard | Native foundation execution |
| --- | --- | --- |
| `envelope`, `files` | Whole file `cfg(any(target_os = "linux", target_os = "macos"))` | Selected on both macOS architectures; `a60aab6` passes; excluded on Windows |
| `cli_profiles`, `enrollment`, `enrollment_forwarding`, `node`, `pairing`, `pairing_cli`, `room_pairing`, `sensitive`, `voters` | Whole file `cfg(target_os = "linux")` | Not run; empty targets on Windows/macOS |
| `enrollment_recovery`, `identity`, `network`, `room_cli`, `room_crash`, `rooms` | Whole file `cfg(unix)` | Not run; excluded on Windows, compiled but not executed on macOS |
| `ipc_long_paths`, `history_keys` | IPC whole-file `cfg(unix)`; key tests use Linux/Apple module guards | Selected on both macOS architectures; `a60aab6` passes, not a future-source guarantee |
| `stdio_pipe`, `stdio_socket`, `stdio_worker`, `stdio_terminal` | Platform-specific source guards | Selected applicable tests on macOS; native PTY/worker outcomes are separate from compilation |
| `cli` | Mixed: Unix-only inspection, identity, two-process message exchange and SIGINT cases; remaining cases unguarded | Not run; minimum smoke lives in the dedicated foundation target |
| `packaging` | No platform guard | Compiled, not run; package inventory is not native product qualification |
| `native_profile` | Linux/macOS only; explicit paths plus isolated HOME/XDG and private shared fixtures | Prepared native CLI acceptance for noncreating status, identity-preserving settings and corrupt/missing-state refusal; not selected by this workflow or yet executed on macOS/WSL2 |

Library tests also mix unguarded, Unix-only and Linux-only modules. Only the explicit macOS filters above are executed; other applicable modules are compiled but not executed by this lane.
Python packaging, documentation and lab suites, browser JavaScript suites, SDK interoperability, VMs and hardware tests are not run here either. The existing Linux workflow retains its separate product gates; this lane neither replaces nor extends their evidence to other operating systems.

### Product Blockers Behind The Foundation

| Platform | Explicit current contract and work still needed |
| --- | --- |
| Windows | `profile::private_directory`, identity create/load, file-source inspection and bounded foreground output return `UnsupportedPlatform`. Protected storage/ACLs, IPC, foreground lifecycle and UI need native implementations and integration qualification; Unix-only node/UI modules are absent. Exact inspected safe-API gaps and native acceptance gates: [Windows storage](windows-storage.md). |
| macOS | Unix identity, protected storage and shared IPC have code paths. Current selected tests must pass on both architectures; prior incomplete runs do not qualify them. Unselected enrollment, complete lifecycle, capture, UI and end-to-end scenarios still need OS evidence. |
| Both | No native installer, exact-artifact replay, two-device exchange, physical media or full-product support is established by this workflow. Native media process code outside Linux lacks the Linux `prlimit`/`setpriv` parent-death wrapper; compilation is not equivalent lifecycle evidence. |

## Atomic Publication Filesystem And Durability Scope

Linux and macOS share the descriptor-relative `files::Store`: a user-owned 0700 directory, no-follow opens, owned 0600 single-link regular files, exclusive staging locks and content/peer-bound resume metadata. The open directory descriptor stays the authority if its pathname moves.
Stream publication uses the same owner/consent and transfer-cancellation gates on both systems, never an ungated macOS commit. Other targets still refuse protected file reception/publication.

The locked rustix 1.1.5 source exposes `renameat_with` for Apple targets and maps
`RenameFlags::NOREPLACE` to `RENAME_EXCL`, using `renameatx_np`. Linux uses
`RENAME_NOREPLACE`. No check-then-rename, copying fallback or handwritten unsafe
syscall is used. If the Apple API is absent, rustix returns `NOSYS` for these
flags/descriptors; K&K reports publication failure instead of overwriting.

Staging and final output live in the same directory/filesystem. Qualification is limited to the local filesystem actually used by the hosted job; it does not establish equivalent locking, permission, atomicity or crash behavior on network, FUSE, cloud-synced or removable filesystems.
The port preserves file `sync_all`, directory `fsync` after rename, metadata unlink only after that sync, and a final directory `fsync`. Errors propagate without warning suppression or fallback.
An error after rename can leave a complete output visible even though the operation reports failure; it is not a rollback guarantee. A cancelled waiter prevents a not-yet-authorized rename, not an already-completed publication.

These sync calls do not claim macOS `F_FULLFSYNC`, hardware cache flushing or power-loss
durability on arbitrary devices. Hosted tests are not power-cut tests. No format,
encryption design or data migration changes are included. Hosted macOS jobs must pass
before recording runtime evidence.

## Identity Backend Boundaries

Unix and macOS private identity IO uses retained directory descriptors and descriptor-relative exclusive creation/no-follow file opens. Opened private keys must be owned 0600 single-link regular files; identity directories must be owned 0700 without special permission bits. The certificate inside an identity is also owned, single-link and not group/world writable.
FIFO/device/directory objects are rejected before reading; nonblocking opens prevent a FIFO substitution from waiting for a writer. Key/certificate mismatch and missing/partial state fail without regeneration.

Ancestor traversal refuses user-owned aliases and writable nonsticky directories; root-owned system aliases and root-owned sticky OS temporary directories remain trusted boundaries (including macOS `/var` and `/tmp`). The resolved path is then walked through no-follow directory descriptors. After the identity descriptor is opened, replacing its pathname does not redirect reads or writes.
This does not isolate a malicious process running as the same account or prevent a privileged administrator changing the namespace. Data and directory sync errors propagate; this is not a hardware power-loss guarantee or macOS `F_FULLFSYNC` claim.

Profile JSON metadata reads, updates and publication leases use the same descriptor-bound directory resolver. It permits root-owned OS ancestor aliases on macOS while rejecting user aliases and unsafe physical ancestors. Reads remain noncreating, but no longer bypass the publication path's ancestor policy.
A missing metadata directory/file returns defaults; malformed or unsafe existing state still fails without reset. This alias integration is implemented, not new native-host execution evidence.
Shared Unix IPC client diagnostics distinguish absent listeners, unavailable socket paths and access-policy refusals using the stable `NODE` code. Client probes do not create profiles/sessions, remove stale sockets or loosen permissions; missing profiles retain `IO`/`NotFound` rather than being initialized.

Public certificate import does not require protected private storage. Its Windows implementation checks the opened final object's reparse attributes using safe standard-library handle APIs, without claiming private storage or authenticated IPC support. Exact missing Windows interfaces are in [Windows storage](windows-storage.md).
Changes to this backend need new native macOS/Windows execution; Linux regression tests or cross-compilation do not extend the earlier hosted qualification.
`platform_foundation` also prepares a Windows sharing-contract test: public import must refuse exclusive readers or already-open writers, then succeed unchanged after handle closure. Adding this test is not native execution evidence.

WSL2 executes the Linux backend, not Windows ACL/named-pipe implementations. Keep private profiles on the Linux filesystem with working Unix owner/mode/no-follow semantics.
Windows-mounted, network and cloud-backed filesystems are not qualified by the Linux implementation or by a file reporting mode 0700; WSL2 interoperability and filesystem-specific protections still require an authorized native host run.

## Runtime Dependencies

| Use | Prerequisites |
| --- | --- |
| CLI / bundled browser UI | Rust executable; no Node or Python application runtime |
| Native media | GStreamer tools/plugins and Linux `prlimit`/`setpriv` |
| Browser media | Required capture/WebCodecs APIs and local browser/OS permission |
| Build / offline rebuild | Pinned Rust toolchain and required C/build tools |
| Lab | Separate libvirt/QEMU/UV/tooling setup; not an application dependency |

`kk media doctor` checks plugins without opening devices. Explicit X11 capture does not bypass Wayland screen-selection permission.
The build record reports actual GLIBC symbols, not the build interpreter's libc. See [media](media.md) and [distribution](distribution.md).
