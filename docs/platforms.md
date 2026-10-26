# Platform Qualification

Platform claims follow executed functional gates, not whether Rust accepts a
target triple. Unsupported protection or capture models fail closed.

| Environment | Current Evidence | Distribution Status |
| --- | --- | --- |
| Linux x86_64 | Ubuntu host, Fedora 44, Mint 22.3 and independent Fedora peer; public room/copy gate and private-display native contention | Primary qualified software target; final frozen-artifact/CI result must pass |
| Linux aarch64 | Native GitHub runner matrix executes the same local Rust/JS/packaging checks | Release candidate only until its actual required CI job succeeds |
| Windows native | Owner-only ACL, secure local IPC and bounded native-process model are not implemented | No full-feature Windows archive is generated; no Windows functional claim |
| macOS native | Some Unix primitives compile, but atomic file publication and bounded native capture/playback are not qualified | No full-feature macOS archive is generated; compilation alone is not acceptance |
| Windows WSL2 | Linux operation is plausible on a protected Linux filesystem; operator Windows was not modified | Not a substitute for native Windows or an executed WSL qualification |
| Mobile browser | Actual bundled layout exercised at 390 x 844, two themes and retained operation errors | Responsive layout evidence only, not touch keyboard, native mobile backend or physical portal evidence |

The 0.8 software delivery is therefore a Linux delivery, not a claim that every
future platform gate in the project roadmap is complete. The native Windows and
macOS backlog remains explicit instead of replacing private storage guarantees
with permissive fallbacks. A physical camera/microphone and OS screen-selection
dialog also require separately consented device validation.

## Linux Runtime

The archive embeds the browser assets in the Rust executable. No Node or Python
runtime is needed to use the installed application. Native CLI media additionally
uses GStreamer tools and the required base/good Opus/VP8/X11/Pulse/V4L2 plugins,
plus util-linux `prlimit`/`setpriv` for bounded Linux child ownership. `kk media
doctor` reports plugin presence without opening capture devices.

Browser capture still uses the browser/OS permission model and WebCodecs support;
it is not granted by joining a room. X11 CLI capture is explicit and is not a
Wayland portal bypass. The build manifest records the actual binary's GLIBC symbol
requirement, build toolchain and separately labelled build-system/Python libc,
rather than inferring a compatibility baseline from the packaging interpreter.
