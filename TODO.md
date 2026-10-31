# Delivery State

Single source for current delivery status. Version: `Cargo.toml`.
Evidence: [verification record](docs/progress.md).

## Execution Order

This is the single persistent product plan. The older dated checkpoint below is
historical evidence, not another active plan. Owners and durable peer IDs live in
[workstreams](docs/workstreams.md); preserve them across compaction and layers.

| Layer | Work | Exit condition |
| --- | --- | --- |
| L0 contracts | Coordinator fixes ownership/common interfaces; six persistent experts map their modules once | Every required capability has an owner, truthful implemented/missing boundary and integration interface |
| L1 breadth | UI, core/history, files/replicas, media, native platforms and distribution implement concurrently | Every required function has a real modular implementation or explicit safe boundary with a precise remaining dependency; skeleton is not claimed functional |
| L2 deepen together | Experts close actual error/cancel/retry/restart/revocation/device gaps in parallel | Focused regressions and real user scenarios establish behavior, not just build success |
| L3 integration | Coordinator resolves shared interfaces; experts perform graphical/native/end-to-end scenarios | Two-profile session includes simultaneous sharing, chat, files/consent, history, roles and recovery; no unowned actionable gap |
| L4 finalization | One frozen integrated candidate: full checks, final CI, signed exact-byte install/update/rollback and publication | Actual required platforms/capabilities pass; preserve data and distinct evidence boundaries |

### L4 Finalization Breakdown

| L4 gate | State | Completed | Remaining exit work |
| --- | --- | --- | --- |
| L4.1 integrated behavior | **DONE** | Common L3 candidate passes 358 library, 15 CLI, seven `room_cli`, four native-profile and nine platform-foundation tests. Recovery/replay, durable transfer IDs and the real process forwarding scenario pass | Re-run only if the frozen final candidate changes these invariants |
| L4.2 core and encrypted history | **DONE** | 24 focused qualification cases pass, including deterministic committed replay after key-package expiry, private replay, snapshot recovery and preserved local history | Bind the unchanged result to the final frozen candidate |
| L4.3 files and replicas | **DONE** | 28 focused cases pass. Consent, retained copies, forwarding, cancellation, resume, deletion and durable terminal transfer inspection are covered; the real restarted-process forwarding scenario passes | Bind the unchanged result to the final frozen candidate |
| L4.4 media | **DONE** | 64 library/CLI/JS/native synthetic cases pass. Fragmentation, cancellation, resume, device selection and simultaneous synthetic media/chat/file workflows are covered | Physical camera/microphone/Wayland portal and sustained network load remain separate explicit evidence boundaries |
| L4.5 graphical product | **DONE** | Independent profiles pass pairing, chat/reply, file consent/forward/cancel/resume, history/restart, roles/revocation, simultaneous synthetic screens and chat/file coexistence. Compact rail, `68→244 px` hover, stable main content and mobile layout are verified on the rebuilt candidate | Preserve this UI unchanged in the frozen candidate; no additional cosmetic pass |
| L4.6 provenance and offline distribution | **DONE** | Vendored OpenMLS/Windows provenance guards pass. Authenticated `verify-only` and interrupted-publication resume each pass on the fresh distribution artifact `35ddd608...e15cfe`; no prefix activation or silent mutation | Produce and bind the final release artifacts after candidate freeze |
| L4.7 Linux native | **DONE** | Linux storage, identity, IPC, lifecycle and integrated product gates pass | Re-run against the exact frozen release bytes |
| L4.8 Windows native | **DONE** | Windows 11 Pro VM at 4 GiB/2 vCPU passes native platform-foundation and Windows gates: protected storage, no-replace publication, exclusive identity, authenticated same-owner named pipe with timeout recovery, and bounded stdio worker. Exact transferred snapshot `a506937965585651b21b44db8c3e44a2bd221e5c1d2d49b8c04f15a2d68467b3`; final gate exits 0 | Bind the unchanged Windows result to the frozen candidate and keep the VM shut down outside native qualification |
| L4.9 macOS native | **DONE** | Hosted Intel and ARM macOS jobs pass build, warning-free lint, smoke, platform contracts and every bounded storage/IPC/history/publication gate in tagged Native run `41214581884`; Windows is green in the same run | No local non-Apple VM claim; preserve the final hosted evidence |
| L4.10 candidate freeze and final gates | **DONE** | Local `make check` and `make workflow-check` pass. Tagged Verify run `41214581888` passes SDK/Hax, audit, x86_64/ARM full checks, clean archives and signed provenance. Final commit is `352e1007bb0db5c5c229f476a2f788760105944d` | None for this candidate |
| L4.11 signed delivery | **DONE** | GitHub prerelease `v0.9.0-dev.2` publishes 16 unchanged assets. Exact x86_64 TAR `9f1fdf3b...dae43`, ZIP `67634413...e0f9` and helper `adf82179...348` pass digest, manifest and tag/commit/workflow Sigstore verification. Fresh install, `0.8.2` update and rollback pass in isolated managed prefixes with installed bytes matching the attested helper | GitLab remains deferred but deployment-ready; the stable 0.8.2 operator link remains intentionally unchanged |

Use targeted checks inside layers; no per-edit commits/pushes/full-CI loops.
Relaunch a persistent module expert for its next layer instead of rediscovering
the module. Keep the five-minute reminder armed and read peers if notices fail.

## Now

| Item | Actual state |
| --- | --- |
| Release | Stable `0.8.2` remains on GitHub/GitLab; prerelease [`v0.9.0-dev.2`](https://github.com/kpihx-labs/kk/releases/tag/v0.9.0-dev.2) is published on active GitHub delivery only |
| Previous draft | Signed `v0.8.1` at `fa6a29f`; retained, **not publicly released** |
| Released software | The published Linux baseline passed product, docs, audit, SDK and both hosted CI platforms |
| Final executable | Attested x86_64 helper SHA-256 `adf82179df86550c9e57103254f142e461674806ace70deadf086ba704eb0348`; tagged Windows/macOS/Linux gates pass |
| Installation | Exact prerelease fresh install, update and rollback pass in isolated managed prefixes; stable authenticated `0.8.2` operator link and profiles remain unchanged |
| Branch name | `main` locally and by default on GitHub/GitLab; commits/tags retained |

## Product Expansion In Progress

The working tree is newer than the installed release. Coordination, file owners
and acceptance cases live in [product workstreams](docs/workstreams.md).

| Scope | Current verification boundary |
| --- | --- |
| Message notifications | Backend/CLI/UI default-off preference, bounded generic alerts and privacy/revision regressions implemented; real browser refusal/non-opt-in and bidirectional messages pass. Native OS grant/display remains open; [evidence](docs/notifications.md). This is not full-product completion |
| Moderation controls | Advisory capabilities delivered at `9e6960b`; hosted checks pass through `ea701a4`. Working-tree real two-profile HTTP regression now approves and retires a participant's vote, removes membership, verifies publishing denial/inactive capabilities and preserves received files. This exposed and corrects falsely recorded outgoing messages after refusal. Focused HTTP/JS pass; full gate and Edge destructive replay remain open. [Evidence](docs/moderation.md) |
| Replies and references | Useful reply/reference/history functionality retained. Operator withdrew emoji reactions as superfluous: active UI/CLI/IPC publication is being removed by owners, with legacy data decoding preserved. Prior reaction demonstrations are historical, not current acceptance goals; [compatibility evidence](docs/conversation.md) |
| Live incoming files | Publication/cancellation/shutdown and lifetime-budget regressions pass; actual two-UI opt-in, denied retry and binary/empty transfer digests verified |
| Profiles and useful status | Locked updates, local member labels, CLI profile management and public-only offline/live status pass combined checks |
| Human-facing UI | Live profile-name collision/routing, local aliases and Settings-conflict controls verified; 39 static layout cases pass; broader interactive qualification remains open |
| Portable archives | TAR/ZIP transaction regressions and signed 0.8.2 installation pass. Local `0.9.0-dev.2` archives/bootstrap/notices bind `9e6960b`, not current source; local integrity verification does not establish publication, signing, provenance or installation. Exact-artifact and offline gates remain open |
| Encrypted local history | Runtime/CLI/UI controls, stable references and literal search implemented with regression evidence; fresh post-reboot two-tab flow and exact-artifact qualification remain open |
| Long-path recovery | IPC correction and runtime-only creation retry implemented; independent review findings require corrective acceptance and a stable integrated gate before delivery |
| Meetings and native targets | Two simultaneous synthetic UI screen publications verified; Windows foundation passes, Apple stdio runtime correction in progress; full native product not claimed |
| Encrypted room history | Transactional capture, key enrollment, authorized CLI/IPC and UI controls implemented; real two-profile HTTP tests pass; latest Edge pairing/history/restart qualification remains open |
| Bootstrap recovery | Public bundle retry tests cover nine publication boundaries without replacing existing bytes; authenticated helper/notice binding tested with mocked network/build inputs, not final release assets |
| Long-profile recovery | Initial SUN_LEN correction passes real IPC/HTTP tests; independent IPC findings require corrective acceptance. Runtime-only create retry now has a passing regression; persistent or ambiguous room state remains refused |
| Persisted QA permissions | Explicitly approved nonrecursive group-write removal applied to workspace/repository/target ancestors: 0775 to 0755. Strict IPC policy retained; QA can resume at unchanged profile paths |
| Critical control replay blocker | Third reader admission committed on A but C cannot start. Read-only comparison: identical ten public log entries, same origin/room/membership and applied index nine; A epoch two/three members/two Welcomes, C epoch zero/one member/no Welcomes. Current-time OpenMLS key-package validation during committed replay is the identified time-dependent hazard; expiry-specific causal regression/correction and explicit recovery remain required. No TTL/clock/permission/check weakening or state reset. Fresh three-process/controller-reconfiguration case passes, so it is not accepted as a fix. All state retained; [evidence and correction boundary](docs/key-package-replay-qualification.md). L4 remains blocked |
| 2026-10-30 integration slice | Initial 121 library/three CLI fixture failures and physical-lock release cases corrected without weakening production checks: library 343 passed/14 explicit ignores, binary eight, every integration target and doctest passed; mandatory HTTP and six synthetic native media cases separately pass. Notices: 492 dependencies/848 files/none missing. All-target Clippy passed after module-order/borrow fixes; latest CLI job slice passes four parser cases, real admission/status/cancel/owner-retirement case and targeted Clippy. Actual GUI image `f8171b5b...abb79c13`: bilateral pairing, chat/reply, denied/live file consent, verified file/forward, cancellation before/after handoff, chat continuity and 16-MiB resume from 131,072 bytes passed. Earlier blockers/failed cleanup attempts retained; minimal keyboard repair now permits scoped paste. Initial task download/history cleanup verified; later expired test download needs final cleanup. Browser media, history/roles/restart and final exact-byte/platform delivery remain open. [GUI proof](docs/ui-candidate-f817-qualification.md), [source record](docs/current-graphical-qualification.md) |

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
| Distribution | Real-gh offline stable 0.8.2 fresh/reinstall/same-version rollback passes as nobody. Local dev.2 bundles bind `9e6960b`; they are not published signed assets or proof of current-source qualification. Published bootstrap/archive installation and inter-version rollback remain open; [bindings](docs/install-qualification.md) |
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
