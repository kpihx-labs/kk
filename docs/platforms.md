# Platform Scope

| Environment | Executed evidence | Not implied |
| --- | --- | --- |
| Linux x86_64 | Host/isolated Fedora and Mint; CLI, storage, rooms, copies and native virtual-display tests | Physical radio/devices or an independently audited deployment |
| Linux aarch64 | Native hosted CI runs the product checks and builds archives | A local ARM VM or physical capture test |
| Windows native | New hosted foundation workflow added, not yet executed; protected storage explicitly refused by code | No full-feature Windows archive or product qualification |
| macOS native | New hosted foundation workflow added, not yet executed; atomic file publication explicitly refused by code | Compilation or Unix permission support alone is not product support |
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
| Offline locked foundation tests | Public digest parsing; Unix protected identity and inspection, or exact non-Unix refusals; Linux publication or non-Linux refusal | Full integration/unit suites, cross-device interoperability or physical devices |

There are no uploads, installers, release calls, signing credentials or write
permissions in this lane. It does not change the existing release job's gates.
The always-run job summary reports scope even if an earlier gate fails; it is
not a success signal. Expected `UnsupportedPlatform` results are assertions of
the present contract, never successful product operations.

### Existing Test Gating Inventory

The native lane deliberately runs only `platform_foundation`, not `cargo test`
without a target selection. All-target build/lint still compile applicable code.
The following is an inventory of existing integration test guards, not a claim
that every Unix-gated suite is runnable on macOS:

| Existing Rust integration suites | Source guard | Native foundation execution |
| --- | --- | --- |
| `cli_profiles`, `enrollment`, `enrollment_forwarding`, `envelope`, `files`, `node`, `pairing`, `pairing_cli`, `room_pairing`, `sensitive`, `voters` | Whole file `cfg(target_os = "linux")` | Not run; empty targets on Windows/macOS |
| `enrollment_recovery`, `identity`, `network`, `room_cli`, `room_crash`, `rooms`, `stdio_socket` | Whole file `cfg(unix)` | Not run; excluded on Windows, compiled but not executed on macOS |
| `cli` | Mixed: Unix-only inspection, identity, two-process message exchange and SIGINT cases; remaining cases unguarded | Not run; minimum smoke lives in the dedicated foundation target |
| `packaging` | No platform guard | Compiled, not run; package inventory is not native product qualification |

Library tests also mix unguarded, Unix-only and Linux-only modules. They are
compiled where applicable but **not executed** by this lane. Python packaging,
documentation and lab suites, browser JavaScript suites, SDK interoperability,
VMs and hardware tests are not run here either. The existing Linux workflow
retains its separate product gates; this lane neither replaces nor extends their
evidence to other operating systems.

### Product Blockers Behind The Foundation

| Platform | Explicit current contract and work still needed |
| --- | --- |
| Windows | `profile::private_directory`, identity create/load, file-source inspection and bounded foreground output return `UnsupportedPlatform`. Protected storage/ACLs, IPC, foreground lifecycle and UI need native implementations and integration qualification; Unix-only node/UI modules are absent. |
| macOS | Unix identity permissions and file inspection have code paths, but the non-Linux `files::Store` refuses atomic publication with `UnsupportedPlatform`. Room/file/enrollment scenarios that need that store are not qualified by their Unix guards. Native IPC, lifecycle, capture and end-to-end behavior still need OS testing. |
| Both | No native installer, exact-artifact replay, two-device exchange, physical media or full-product support is established by this workflow. Native media process code outside Linux lacks the Linux `prlimit`/`setpriv` parent-death wrapper; compilation is not equivalent lifecycle evidence. |

## Runtime Dependencies

| Use | Prerequisites |
| --- | --- |
| CLI / bundled browser UI | Rust executable; no Node or Python application runtime |
| Native media | GStreamer tools/plugins and Linux `prlimit`/`setpriv` |
| Browser media | Required capture/WebCodecs APIs and local browser/OS permission |
| Build / offline rebuild | Pinned Rust toolchain and required C/build tools |
| Lab | Separate libvirt/QEMU/UV/tooling setup; not an application dependency |

`kk media doctor` checks plugins without opening devices. X11 capture is explicit;
it does not bypass Wayland screen-selection permission. The binary's build record
reports its actual GLIBC symbol requirement rather than guessing from the build
interpreter's libc. See [media](media.md) and [distribution](distribution.md).
