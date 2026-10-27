# Platform Scope

| Environment | Executed evidence | Not implied |
| --- | --- | --- |
| Linux x86_64 | Host/isolated Fedora and Mint; CLI, storage, rooms, copies and native virtual-display tests | Physical radio/devices or an independently audited deployment |
| Linux aarch64 | Native hosted CI runs the product checks and builds archives | A local ARM VM or physical capture test |
| Windows native | Required protected storage/IPC/lifecycle not implemented | No full-feature Windows archive |
| macOS native | Atomic publication and native lifecycle/capture not qualified | Compilation alone is not support |
| WSL2 | Not an executed qualification | Not native Windows support |
| Mobile browser | Narrow-screen light/dark layout fixture | No physical phone, keyboard or mobile-backend claim |

This is a Linux software delivery. The exact final artifact and publication state
remain in [TODO.md](../TODO.md) and the [verification record](progress.md).
Unsupported protection models fail closed instead of weakening storage or TLS.

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
