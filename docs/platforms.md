# Platform Scope

| Environment | Executed evidence | Not implied |
| --- | --- | --- |
| Linux x86_64 | Host/isolated Fedora and Mint; CLI, storage, rooms, copies and native virtual-display tests | Physical radio/devices or an independently audited deployment |
| Linux aarch64 | Native hosted CI runs the product checks and builds archives | A local ARM VM or physical capture test |
| Windows native | New hosted foundation workflow added, not yet executed; protected storage explicitly refused by code | No full-feature Windows archive or product qualification |
| macOS native | Hosted foundation and bounded storage tests added, not yet executed; protected atomic publication now has a code path | No macOS runtime pass or full-product support until actual hosted evidence; no physical capture claim |
| WSL2 | Not an executed qualification | Not native Windows support |
| Mobile browser | Narrow-screen light/dark layout fixture | No physical phone, keyboard or mobile-backend claim |

This is a Linux software delivery. The exact final artifact and publication state
remain in [TODO.md](../TODO.md) and the [verification record](progress.md).
Unsupported protection models fail closed instead of weakening storage or TLS.

## Native Foundation CI

[`native.yml`](../.github/workflows/native.yml) is a nonpublishing lane for
`windows-2022` / `x86_64-pc-windows-msvc`, `macos-15-intel` /
`x86_64-apple-darwin`, and `macos-15` / `aarch64-apple-darwin`.
It uses the existing checkout action pin and reads the Rust channel from
[`rust-toolchain.toml`](../rust-toolchain.toml), rather than maintaining another
version. The [hosted runner labels](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
identify native hosts, not Linux cross-compilation.

**Execution status: this new workflow has not run.** Local Linux checks of
[`platform_foundation.rs`](../tests/platform_foundation.rs) cannot establish
Windows or macOS results. Record actual hosted job results before changing this
statement. A hosted compiler, dependency, lint or linker failure is a foundation
blocker, not permission to skip the failed step or claim product support.

| Gate | What it establishes if it passes | What it does not establish |
| --- | --- | --- |
| Locked fetch, offline locked all-target build | Native dependency resolution and compilation of binary, library, tests and examples | Execution of compiled test/example targets |
| Offline locked all-target Clippy, warnings denied | Native target lint acceptance, without suppression | Runtime safety or platform qualification |
| Explicit binary smoke | Actual native executable runs `--version`, `--help`, and refuses an unknown argument with exit 2 | Initialized profiles, networking, UI or media |
| Offline locked foundation tests | Public digest parsing; Unix protected identity and inspection, or exact non-Unix refusals; Linux/macOS publication or unsupported-target refusal | Full integration/unit suites, cross-device interoperability or physical devices |
| macOS-only file and envelope integration tests | Native binary-payload/empty/chunk-boundary transfers, verified-prefix resume, nonclobber, unsafe path/link/permission refusal and existing envelope regressions | Cross-device exchange, crypto redesign or data migration |
| macOS-only selected library tests | `files::` staging/wire checks; `rooms::control::raft::service::tests::consent` and `rooms::control::raft::network::file::tests` publication, revocation, cancellation and grant-lock regressions | Unselected library suites, native UI/IPC, lifecycle or capture qualification |

There are no uploads, installers, release calls, signing credentials or write
permissions in this lane. It does not change the existing release job's gates.
The always-run job summary reports scope even if an earlier gate fails; it is
not a success signal. Expected `UnsupportedPlatform` results are assertions of
the present contract, never successful product operations.

### Existing Test Gating Inventory

The native lane deliberately selects the tests above, not `cargo test` without
a target selection. Windows runs only `platform_foundation`. All-target
build/lint still compile applicable code.
The following is an inventory of existing integration test guards, not a claim
that every Unix-gated suite is runnable on macOS:

| Existing Rust integration suites | Source guard | Native foundation execution |
| --- | --- | --- |
| `envelope`, `files` | Whole file `cfg(any(target_os = "linux", target_os = "macos"))` | Selected on both macOS architectures, results pending; excluded on Windows |
| `cli_profiles`, `enrollment`, `enrollment_forwarding`, `node`, `pairing`, `pairing_cli`, `room_pairing`, `sensitive`, `voters` | Whole file `cfg(target_os = "linux")` | Not run; empty targets on Windows/macOS |
| `enrollment_recovery`, `identity`, `network`, `room_cli`, `room_crash`, `rooms`, `stdio_socket` | Whole file `cfg(unix)` | Not run; excluded on Windows, compiled but not executed on macOS |
| `cli` | Mixed: Unix-only inspection, identity, two-process message exchange and SIGINT cases; remaining cases unguarded | Not run; minimum smoke lives in the dedicated foundation target |
| `packaging` | No platform guard | Compiled, not run; package inventory is not native product qualification |

Library tests also mix unguarded, Unix-only and Linux-only modules. Only the
explicit macOS filters above are executed; other applicable modules are compiled
but not executed by this lane. Python packaging,
documentation and lab suites, browser JavaScript suites, SDK interoperability,
VMs and hardware tests are not run here either. The existing Linux workflow
retains its separate product gates; this lane neither replaces nor extends their
evidence to other operating systems.

### Product Blockers Behind The Foundation

| Platform | Explicit current contract and work still needed |
| --- | --- |
| Windows | `profile::private_directory`, identity create/load, file-source inspection and bounded foreground output return `UnsupportedPlatform`. Protected storage/ACLs, IPC, foreground lifecycle and UI need native implementations and integration qualification; Unix-only node/UI modules are absent. Exact inspected safe-API gaps and native acceptance gates: [Windows storage](windows-storage.md). |
| macOS | Unix identity permissions, file inspection and protected atomic publication have code paths. Selected native storage and publication-gate tests are required on both hosted architectures and have not yet run. Room/file/enrollment scenarios are not qualified merely by their Unix guards. Native IPC, lifecycle, capture and end-to-end behavior still need OS testing. |
| Both | No native installer, exact-artifact replay, two-device exchange, physical media or full-product support is established by this workflow. Native media process code outside Linux lacks the Linux `prlimit`/`setpriv` parent-death wrapper; compilation is not equivalent lifecycle evidence. |

## Atomic Publication Filesystem And Durability Scope

Linux and macOS share the descriptor-relative `files::Store`: a user-owned 0700
directory, no-follow opens, owned 0600 single-link regular files, exclusive staging
locks and content/peer-bound resume metadata. The open directory descriptor stays
the authority if its pathname moves. Stream publication uses the same owner/consent
and transfer-cancellation gates on both systems, never an ungated macOS commit.
Other targets still refuse protected file reception/publication.

The locked rustix 1.1.5 source exposes `renameat_with` for Apple targets and maps
`RenameFlags::NOREPLACE` to `RENAME_EXCL`, using `renameatx_np`. Linux uses
`RENAME_NOREPLACE`. No check-then-rename, copying fallback or handwritten unsafe
syscall is used. If the Apple API is absent, rustix returns `NOSYS` for these
flags/descriptors; K&K reports publication failure instead of overwriting.

Staging and final output live in the same directory/filesystem. Qualification
is limited to the local filesystem actually used by the hosted job; it does not
establish equivalent locking, permission, atomicity or crash behavior on network,
FUSE, cloud-synced or removable filesystems. The port preserves file `sync_all`,
directory `fsync` after rename, metadata unlink only after that sync, and a final
directory `fsync`. Errors propagate without warning suppression or fallback.
An error after rename can leave a complete output visible even though the
operation reports failure; it is not a rollback guarantee. A cancelled waiter
prevents a not-yet-authorized rename, not an already-completed publication.

These sync calls do not claim macOS `F_FULLFSYNC`, hardware cache flushing or power-loss
durability on arbitrary devices. Hosted tests are not power-cut tests. No format,
encryption design or data migration changes are included. Hosted macOS jobs must pass
before recording runtime evidence.

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
