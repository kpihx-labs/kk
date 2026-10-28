# Supervised stdio

## CLI

K&K ships one executable. On Darwin its reserved worker invocation is dispatched
before Tokio, argument parsing, profiles or networking. Normal startup retains
the current-thread Tokio runtime with all previously enabled drivers.

Darwin pipe input/output uses one supervised subprocess per operation. Opening
`/dev/stdin` or `/dev/stdout` there duplicates the underlying descriptor and
ignores `O_NONBLOCK`; readiness polling alone cannot prevent blocking IO races.
Workers never change borrowed file-status flags or install signal handlers.
Linux's existing input/output paths remain intact. Darwin socket output is
supervised too, so the caller's socket options remain unchanged.
Terminal prompts and final newlines use bounded cancellable diagnostic output,
never a synchronous stderr write while the echo-restoration guard is held.
Prompt cancellation leaves attributes untouched; input cancellation restores them.

## SDK participation

Public `kk::input`/`kk::output` APIs never assume SDK hosts understand worker requests.

Before using Darwin pipe input, non-regular-file output (including sockets),
terminal prompts or hidden input's cancellable stderr notice, explicitly
register a trusted, compatible K&K executable using an absolute path:

```rust,no_run
# fn main() -> kk::Result<()> {
kk::stdio_worker::configure(std::path::PathBuf::from("/trusted/bin/kk"))?;
# Ok(())
# }
```

Registration is process-wide and cannot be replaced with a different path.
The caller controls the executable and its containing directories; compatibility
checking is not authentication of an untrusted executable. There is no PATH
search, environment override or implicit `current_exe()` lookup in library code.
Without registration, Darwin pipe operations return a descriptive
`Error::Configuration` pointing here, rather than spawning the SDK host.
Inline input, nonsensitive regular-file input, nonsensitive socket input and
regular-file output need no registration.

Alternatively an SDK executable can implement the same early dispatch:

```rust,no_run
fn main() -> std::process::ExitCode {
    if let Some(status) = kk::stdio_worker::dispatch() {
        return status;
    }
    // Explicit host opt-in, before creating the application's Tokio runtime.
    if kk::stdio_worker::configure(std::env::current_exe().unwrap()).is_err() {
        return std::process::ExitCode::FAILURE;
    }
    // Construct and run the application's runtime here.
    std::process::ExitCode::SUCCESS
}
```

Both examples require Darwin; guard them with `cfg(target_vendor = "apple")` in
portable applications. `Worker::new(path)` provides per-value raw IO there.
On other Unix hosts, `configure`, `configured` and `Worker::new` return a clear
Darwin-only configuration error before spawning or opening worker descriptors.
Linux's ordinary input/output APIs do not require this worker API. The hidden
production dispatcher does nothing on Linux; no environment bypass enables it.
Raw writes are already framed bytes, not lines; raw reads do not validate UTF-8.
`Worker::write_stderr` provides the same bounded, cancellable diagnostic output.
The public text/line adapters retain their existing validation and framing.

## Protocol and lifetime

- Each child accepts one versioned, length-bounded read or write request.
  Limits are shared with the input/output adapters; oversized requests are
  rejected before allocation or data IO. At most four library workers run at once.
- An owned socket carries control/results through child stdin/stdout. Only the
  chosen data descriptor is mapped to child fd 2. Payloads never enter argv or
  environment, and worker errors never print into that data descriptor.
- Darwin uses safe `posix_spawn` with `POSIX_SPAWN_CLOEXEC_DEFAULT`: unknown SDK
  descriptors above 2 are excluded even without CLOEXEC. Owned sources above 2
  are duplicated to 0/1/2, then only those owned originals are closed. The helper
  receives an empty environment and must run without ambient loader overrides.
- A process-contained supervisor watches control EOF or unexpected trailing
  traffic. It terminates the entire worker even while data IO is blocked.
  Result IO uses owned files, not global stdio locks that could obstruct exit.
- Successful calls reap their worker. Dropping an in-flight operation kills
  and synchronously reaps it, independently of runtime shutdown. There are no
  detached parent threads, retry loops or recursively spawned helpers.
- Abrupt parent death closes control and terminates the child. Orphan reaping
  belongs to the operating system's init/subreaper. This assumes the host does
  not deliberately duplicate or leak private control descriptors into other
  processes and keeps its stdio descriptors stable during each operation.
  Exclusive wait/reap ownership is a host precondition, as with `std::process::Child`:
  no competing waitpid or automatic child reaping. ECHILD clears ownership before
  any signal. Violating exclusivity can reuse a numeric PID before cancellation;
  there is no universal no-other-child-signalled guarantee under that violation.
  Cancellation assumes normal OS kill/wait behavior for pipe waits.
- Cancellation cannot undo bytes already read or written. The process boundary
  prevents blocked work from surviving cleanup, not partial external effects.

## Verification scope

`stdio_pipe`, `stdio_socket`, `stdio_terminal` and `stdio_worker` exercise real
child processes. Parent deadlines kill/reap stalled fixtures. Darwin tests assert
actual SDK descriptor exclusion, worker reaping, parent death, full-stderr notice
cancellation, unchanged socket options and explicit/unconfigured SDK behavior.
Parent-death coverage includes production `Worker` spawning, not only direct exec.
Linux asserts public API refusal before spawning and keeps protocol/supervision
tests in the library's test executable only. These controlled fixtures cannot be
enabled in production. They do not qualify Darwin spawning or SDK isolation.

The direct target-specific path dependency pins corrected `nix` 0.31.3, with only
the `process` feature. Its MIT license and upstream unsafe blocks are preserved.
Safe return checks now handle positive POSIX errors, including init failures.
Independent SDK graphs inherit this path dependency without a root Cargo patch.
The release's offline source archive includes tracked override sources and
vendored registry dependencies. The unpublished Cargo registry `.crate` is not
the offline source distribution; Cargo excludes nested packages from that file.

The owned-PTY regression uses offline `uv` Python stdlib allocation, safe rustix
attribute/control APIs and ordinary runtime shutdown, without Cargo feature edits.
Run all four targets on macOS too; Linux cannot qualify native Darwin spawning.
