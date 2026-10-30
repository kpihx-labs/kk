# Delivery State

Single source for current delivery status. Version: `Cargo.toml`.
Evidence: [verification record](docs/progress.md).

## Now

| Item | Actual state |
| --- | --- |
| Release | `0.8.2`, published on [GitHub](https://github.com/kpihx-labs/kk/releases/tag/v0.8.2) and [GitLab](https://gitlab.com/kpihx-labs/kk/-/releases/v0.8.2) |
| Previous draft | Signed `v0.8.1` at `fa6a29f`; retained, **not publicly released** |
| Released software | The published Linux baseline passed product, docs, audit, SDK and both hosted CI platforms |
| Final executable | Exact signed x86_64 bytes pass 25 public, 66 baseline and native contention scenarios |
| Installation | Authenticated `0.8.2` executable installed; obsolete managed `0.8.0` trashed on request, profiles preserved |
| Branch name | `main` locally and by default on GitHub/GitLab; commits/tags retained |

## Product Expansion In Progress

The working tree is newer than the installed release. Coordination, file owners
and acceptance cases live in [product workstreams](docs/workstreams.md).

| Scope | Current verification boundary |
| --- | --- |
| Message notifications | Backend/CLI/UI default-off preference, bounded generic alerts and privacy/revision regressions implemented; real browser refusal/non-opt-in and bidirectional messages pass. Native OS grant/display remains open; [evidence](docs/notifications.md). This is not full-product completion |
| Moderation controls | Backend-derived advisory capabilities and delegation-bounded UI choices implemented; full local check and selected actual Edge moderator/participant flow pass. Reader/inviter rendering, actual retirement/removal/revocation and fresh hosted delivery remain open; [evidence](docs/moderation.md) |
| Replies and reactions | Backend-owned bounded typed MLS operations, audience negotiation, transactional encrypted relations, CLI/UI implemented. Full local check and real two-profile Edge reply, reaction add/remove, sensitive defaults and post-restart scroll replay pass; [evidence/limits](docs/conversation.md). Delivery and complete mixed-version/error qualification remain open |
| Live incoming files | Publication/cancellation/shutdown and lifetime-budget regressions pass; actual two-UI opt-in, denied retry and binary/empty transfer digests verified |
| Profiles and useful status | Locked updates, local member labels, CLI profile management and public-only offline/live status pass combined checks |
| Human-facing UI | Live profile-name collision/routing, local aliases and Settings-conflict controls verified; 39 static layout cases pass; broader interactive qualification remains open |
| Portable archives | TAR/ZIP transaction regressions pass; real signed 0.8.2 TAR online install, same-version reinstall/rollback verified in isolated prefix; new/raw/offline artifact gates remain open |
| Encrypted local history | Runtime/CLI/UI controls, stable references and literal search implemented with regression evidence; fresh post-reboot two-tab flow and exact-artifact qualification remain open |
| Long-path recovery | IPC correction and runtime-only creation retry implemented; independent review findings require corrective acceptance and a stable integrated gate before delivery |
| Meetings and native targets | Two simultaneous synthetic UI screen publications verified; Windows foundation passes, Apple stdio runtime correction in progress; full native product not claimed |
| Encrypted room history | Transactional capture, key enrollment, authorized CLI/IPC and UI controls implemented; real two-profile HTTP tests pass; latest Edge pairing/history/restart qualification remains open |
| Bootstrap recovery | Public bundle retry tests cover nine publication boundaries without replacing existing bytes; authenticated helper/notice binding tested with mocked network/build inputs, not final release assets |
| Long-profile recovery | Initial SUN_LEN correction passes real IPC/HTTP tests; independent IPC findings require corrective acceptance. Runtime-only create retry now has a passing regression; persistent or ambiguous room state remains refused |
| Persisted QA permissions | Explicitly approved nonrecursive group-write removal applied to workspace/repository/target ancestors: 0775 to 0755. Strict IPC policy retained; QA can resume at unchanged profile paths |

### Current checkpoint: 2026-10-29

This checkpoint supersedes the pending wording in the earlier expansion snapshot;
it does not change the installed release or qualify new distributed bytes.

| Gate | Current state |
| --- | --- |
| Source | Application/test checkpoint `cfa8145`, package `0.9.0-dev.2`, delivered to both remotes: duplex sender `1f846ef`, shared revoked-wire-peer correction, and `97eb1dd` private native workspace. Later harness work adds TCP-zero allocation and measured guest digests, not a new release |
| Hosted verification | `cfa8145` Verify `40950864252` and Native `40950864320` succeed. Both macOS jobs pass five fail-fast probes of all 17 consent cases, zero ignored/failed. Earlier `1f846ef` Intel and `114705b` ARM failures are retained in the [duplex evidence](docs/file-duplex-qualification.md), not hidden by retries or deadline changes |
| Native foundation | Selected Windows and both macOS foundation/storage/IPC/history/consent gates pass on `cfa8145`, including actual PowerShell refusal on Windows. This is the corrected fixture snapshot, not a proof of every historical Quinn wakeup cause, complete native product support or physical capture |
| GitLab deferred | Operator direction 2026-10-30: GitHub is active delivery; GitLab push/CI is nonblocking and deferred. Preserve remote, CI and explicit later synchronization. Earlier quota failures remain recorded; do not stop other product lots, buy allocation or alter global runners |
| Historical tag | `v0.9.0-dev.1` remains immutable at `7f93adf`; its failed verification is not replaced or promoted |
| History lifecycle | Actual two-tab disable/re-enable, local clear, unchanged-profile cold restart and bidirectional messages pass; [evidence](docs/reboot-history-qualification.md) |
| File/media UI | Prior binary/empty/withdrawal checks plus a new 4-MiB file sent with both synthetic publication/subscription pairs active; exact recipient digest, advancing bracketed frame counters and chat pass. [Evidence](docs/reboot-data-media-qualification.md); no per-packet timing, sustained-load or physical-capture claim |
| Distribution | Real-gh offline stable 0.8.2 fresh/reinstall/same-version rollback passes as nobody. Unsigned corrected `cfa8145` TAR/ZIP integrity and measured three-VM/native contention replay pass; [native evidence](docs/native-candidate-qualification.md). Published signed new bootstrap/archive installation and inter-version rollback remain open; [bindings](docs/install-qualification.md) |
| Lab preflight | Definitions/disks retained. Corrected `cfa8145` bytes pass 25 public CLI scenarios, native contention and the corrected 66-case broader baseline, with independent guest executable digests. Earlier status-parser failure remains recorded. [Bindings](docs/install-qualification.md) and [measured replay](docs/native-candidate-qualification.md#broader-baseline-compatibility); not signed release/installer proof |

## Not Qualified

| Scope | Work still needed |
| --- | --- |
| Windows/macOS native | Platform storage, IPC, lifecycle and real OS qualification |
| Physical media/Wayland portal | Explicitly consented hardware and portal lifecycle tests |
| Physical networks | Wi-Fi/AP isolation, sustained load and traversal evidence |

These remain outside this Linux delivery. They are not silently closed by a
compilation, virtual display or mobile layout fixture. Exact digests, CI and lab
report references are kept in the verification record, not duplicated here.
