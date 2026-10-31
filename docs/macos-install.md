# macOS Installer Implementation Boundary

The source implements macOS native installation and rollback under the existing
`InstallRequest` / `Outcome` API. This is not evidence of a published macOS release
or successful execution on an Apple host. Windows remains fail-closed.

## Format And ABI Gates

- Thin little-endian Mach-O 64-bit only: baseline x86_64 or arm64, matching the
  requested Darwin target. Universal images, foreign platforms and CPU subtypes
  are rejected rather than choosing a different execution slice.
- The `object` crate reads headers/load commands. Input bytes, command count,
  command-table size, section counts, segment/file bounds and strings are bounded.
- PIE, a system dyld loader, a mapped executable entrypoint and an explicit macOS
  deployment requirement are mandatory. Writable executable segments, rpaths,
  dyld environment injection, relative/non-system libraries and unknown load
  commands are refused.
- `binary_macos_required` must exactly match executable metadata; a Linux GLIBC
  requirement in a macOS manifest is rejected. `/usr/bin/sw_vers` must report a
  sufficiently new host. `/usr/bin/codesign --verify --strict` checks an exact
  private candidate copy without executing it. Ad-hoc signatures are permitted;
  origin authentication remains the pinned GitHub Actions attestation, not an
  invented Developer ID or notarization claim.
- System dylibs may exist only in dyld's shared cache. The implementation constrains
  their install names to Apple system locations; it does not claim Linux-style
  per-symbol version inspection of that cache or prove every runtime dependency.

## Verification Input And Activation

Linux retains kernel-sealed memfd/procfs inputs. macOS instead uses private owned
0700 temporary directories with protected no-follow ancestors, exclusive files, closed writable handles,
0400 verifier inputs and descriptor/path/content rechecks before returning the
authenticated bytes. Decoder suffixes are preserved. This is **not kernel seal
equivalence**: malicious same-UID code and root/account compromise are excluded.
Retained input owners accompany the subprocess cleanup owner until process-group
termination and leader reaping finish, including caller cancellation.
This requires exclusive reaping ownership. ECHILD disarms all numeric signalling;
Tokio kill-on-drop is disabled, rather than risking a reused PID/PGID.

The descriptor-relative managed transaction is shared: owner/mode checks, root/
elevation refusal, private immutable version trees, lock serialization, retained
original archives and receipts, same-version mismatch refusal and atomic managed
activation. rustix maps no-replace publication to Darwin `RENAME_EXCL`; it does not
fall back to a check-then-overwrite rename. Profiles and unrelated binaries are
never opened or replaced. System `/var` temporary aliases are resolved only when
selecting the private verifier parent; managed prefix traversal still rejects links.
Descendants freeze before publication, while the private stage root stays 0700
through exclusive rename. The published root is frozen/synced before activation.
Post-rename failure can leave a visible unactivated version; it is not "unchanged"
and does not trigger automatic rollback. No F_FULLFSYNC/power-loss guarantee is made.

## Packaging And Bootstrap

`release.py` supports Darwin targets and `macos-ARCH` TAR/ZIP/bootstrap assets.
Native Apple `otool` supplies deployment metadata; `codesign` must validate the
candidate before packaging. Source/notice/checksum gates remain unchanged. The
shell authenticates explicit macOS/Mach-O companion bindings; bootstrap needs
`timeout`/`gtimeout`, jq and sha256sum/shasum, not a compiler. The trusted-helper
path does not need those shell-bootstrap tools.

## Remaining Host Evidence

Local Linux parser/backing-file tests are not macOS installation acceptance.
An Apple host must still compile the final dependency/wiring, exercise its real
process-group cleanup and filesystem durability semantics, and run fresh install,
update and rollback against exact authenticated artifacts. Native runtime/library
availability and physical crash durability are separate from format inspection.
The current Linux workstation has no Apple SDK/runtime or installed Darwin Rust
target; no Apple-host execution is inferred from mocked shell/packaging cases.
