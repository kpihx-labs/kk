# Verification Record

Executed evidence for released `0.8.2`, 2026-10-27. This page records proof;
[TODO.md](../TODO.md) alone owns current delivery work and publication status.

## Software And Hosted Checks

| Gate | Recorded result |
| --- | --- |
| Product check | 155 Rust, six JavaScript, 11 packaging and five documentation tests pass; format/Clippy/docs clean |
| Explicit media tests | Nine pass, including the four tests ignored by the default suite |
| Dependency audit | Root graph passes strict known-advisory and maintenance checks without ignores |
| Notices | 485 dependencies covered; local overrides included; no missing notice |
| Expansion source | Three exact source hashes; six upstream XMD/XOF vector groups pass |
| Independent OPAQUE | Official/restricted peers verified as separate graphs; six positive/negative cross-role cases pass |
| Conditional Hax | Actual macro branch compiles with upstream nightly; not a formal extraction claim |
| Existing private state | Agent-owned multi-member 0.8.0 room reopens and stops with the dependency-updated runtime |
| Offline source | Extracted candidate builds all product targets with fresh Cargo home and `--offline --locked` |

Hosted checks for `v0.8.2`, source `6a0a085`:
[GitHub](https://github.com/kpihx-labs/kk/actions/runs/40526267726) and
[GitLab](https://gitlab.com/kpihx-labs/kk/-/pipelines/2995038142).
Both pipelines pass. Three archive attestations match the exact tag, source
commit, workflow and hosted-runner policy. The same draft assets were promoted.

## Exact Binary Evidence

| Binary SHA-256 | Scope |
| --- | --- |
| `3aad41a3b5cf532ce927a01309d7a5d2730f0765899d316865ebc16f36db1cb7` | Final signed release and installed x86_64 executable: 25 public scenarios, 66 baseline scenarios and native contention pass |

Retained local reports, relative to the repository:

```text
lab/tmp/public-room-cli-e5492670d1df.json
lab/tmp/kk-results-2a4acbde7a26-1793114775594979706.json
lab/tmp/native-media-b72154dee78f.json
```

The final native run decoded 1,455 packets and fresh private-display pixels
while a verified file was active. Nine text samples had a maximum 1.555-second
acknowledgement at 125 KiB/s. Cleanup reports are empty; guests/network stop and
the previous QoS policy is restored. These are run observations, not a latency SLA.

## Do Not Overclaim

Synthetic media is not physical capture. A virtual display is not the operator's
desktop. Native hosted ARM tests are not a local ARM-device test. Quorum checks
are not Byzantine consensus. An authenticated candidate is not a published release.
Earlier attempts and checkpoints remain in Git and retained lab/tool artifacts.

## Working-Tree Product Expansion

These are intermediate source checks after the release, not installed or shipped
artifact qualification. Current ownership and remaining gates: [workstreams](workstreams.md).

| Gate | Observed result and boundary |
| --- | --- |
| Profile concurrency | Initial seven profile tests pass; locked partial-appearance and corrupt-certificate tests added subsequently |
| CLI profiles/status | Initial three new profile tests and ten existing CLI tests pass; public-only room inventory and control admission added afterwards |
| ZIP packaging | Initial 19 release tests pass; immutable-snapshot pathname/same-inode replacement tests raise coverage to 22, all pass |
| Packaging/docs | One Rust packaging and five documentation tests pass; source-tree checks, not native ZIP execution |
| Consent | Byte/attempt lifetime unit test and saturated-work-lane unit test pass; executor reports five real-service receive/fetch/budget regressions passing three reruns |
| Intermediate full check | `make check` passes fmt, Clippy, all default Rust targets, rustdoc, six then-discovered JS tests, 19 packaging and five docs tests; four native media tests remain explicitly ignored |
| UI follow-up | Normal UI gate discovers 11 JS tests including all quantized appearance contrasts; five docs and 22 release tests pass |
| Native foundations | Seven Linux foundation tests pass; hosted Windows and both macOS builds failed at 8d3f384; working-tree cfg/storage/stdio corrections still require a fresh native run |
| Combined source gate | `make check` passes: 209 Rust, 21 JS, 22 release and five docs tests; fmt/Clippy/rustdoc clean |
| Explicit native source gate | All four ignored native codec, private virtual-screen and child/owner lifecycle tests pass when invoked explicitly |
| Independent security follow-up | Static checker finds no remaining concrete defect in publication/revocation/cancellation, owner/borrower lifetime, reserved IPC or immutable archive snapshot paths |
| Expanded integrated source gate | `make check` passes 255 Rust tests, 38 JS, 22 packaging and five docs tests; fmt/Clippy/rustdoc clean; four native media gates separately invoked and passed |
| Name consent/revision | Per-poll disclosure gates and 45 profile tests returned; UI API rejects stale ABA revisions without changing bytes; pending/failed conflict refresh now uses a direct successful state read; 38 JS tests pass after that correction |
| Responsive rendering | Two real Edge profiles: compact rail persists; chat grows from 192 to 337px at 881x786, composer visible and no horizontal overflow; narrow/short viewports not yet qualified |
| Remaining review findings | Stdio descriptor inheritance, blocked hidden-input notice and borrowed Apple socket-option mutation assigned for correction; metadata filesystem latency/path-authority bounds also under review; these prevent a final acceptance claim |
| After-reboot packaging retry | Main independently ran 33 release tests, including public bundle retries at all nine publication boundaries; mocked build inputs, not real artifact provenance |
| After-reboot adapters | Main ran 46 JS, 27 installer tests plus one explicit PowerShell skip, five docs tests and pinned workflow lint successfully |
| After-reboot native media | Main explicitly ran all four ignored native media tests successfully; synthetic codecs, owned virtual display and child lifetime only, no physical capture |
| IPC review rejection | Initial long-path functional tests pass, but independent review found pathname races, interrupted binding initialization and lock-entry replacement gaps; corrective source work is active |
| Concurrent-source check limitation | Main invoked the complete product check while IPC corrective edits were ongoing: default tests passed, but later doc compilation saw intermediate dead-code warnings. This is not accepted as a stable final snapshot gate; rerun after ownership handoff |
| Corrected IPC and create retry | Main reviewed the corrective handoff, bounded interrupted-stage enumeration and passed 11 IPC tests plus the isolated umask child, three real IPC/HTTP integrations, five reservation tests and all-target Clippy with warnings denied |
| Persisted QA permission blocker | Actual workspace, repository and target ancestors are 0775. Strict IPC refuses group-writable non-sticky ancestors; no permissions were changed or policy relaxed. Explicit approval is requested for a nonrecursive removal of group-write on those three directories before GUI continuation |
| SDK replay | Main reran six upstream XMD/XOF vectors and six independent cross-role OPAQUE positive/negative cases successfully |
| Permission blocker resolved | Operator approved and Main applied only nonrecursive group-write removal on the workspace/repository/target ancestors: each is now 0755. No profile relocation or relaxed IPC policy |
| Logical ownership and publication | IPC and reservation guards explicitly unlock despite retained descriptions; binding publication uses no-replace rename. Main passed 14 IPC cases plus isolated umask child and five reservation cases; native replay remains separate |
| UI lifecycle correction | A queued old dialog close erased replacement controls in real Edge. Modal revisions now fence stale close/results and retire private controls/handlers; 48 JS and 17 fixture guard tests pass |
| Fresh Edge pairing/history | Unchanged long-profile paths create and pair after matching SAS. Both show two members/epoch one. Equal Reboot QA bodies yield two distinct references and two stored/rendered matches; sensitive text is hidden and excluded. See after-reboot evidence for remaining gates |
| Profile fixture timing | One complete run exceeded its unchanged 40-second metadata fixture bound. Phase diagnostics were added; focused and later complete runs passed. No root cause or timing guarantee is claimed from reruns |
| Checkpoint 9e16252 hosted failures | GitLab Rust and browser jobs pass; release-integrity and both GitHub Linux packaging-lint steps fail on import order and late-bound fault-injection closures. All three native all-target builds pass, but platform-specific dead-code/import lint blocks subsequent execution. No skipped native step is counted as a pass |
| CI correction verification | Imports and loop captures are corrected; Linux-only/test-only traits and the Unix-only database file handle are cfg-scoped, not suppressed. Python lint is now part of the local product gate, with CI selecting its pinned provider through RUFF. Local Python lint, release/fixture tests, workflow lint and all-target Clippy pass; hosted replay remains required |
| Current hosted snapshot `8761b9f` | Native selected workflow `40823238875` succeeds. Verify `40823238849`, job `123280290696`, fails at `src/rooms/runtime/tests.rs:444` while encrypting a bounded actor event: `Error::Timeout`, 267 passed and one failed in the library batch. This is separate from the previously corrected consent fixture; no rerun is counted as a fix |
| Dedicated guest inventory | Read-only `virsh -c qemu:///system` checks on 2026-10-29 confirm `fedora-44`, `mint-22.3`, `kk-peer-3` and their dedicated qcow2 paths; all are shut off and the isolated network is inactive. Incorrect former guest-name guesses are not missing machines; startup and exact-artifact replay remain unexecuted |
| Native archive redirect correction | Main passed all 52 installer target tests and all-target Clippy after replacing automatic HTTPS redirects with exact-authority checks before each hop. Mocked transport tests prove allowed-chain byte preservation and refusal before foreign-host contact/provenance/writes; no new real-download qualification is inferred |
| Quorum scheduler fixture | Executor demonstrates that blocking the current-thread private caller also freezes every simulated voter/driver; two workers permit a real remote quorum read while the caller blocks, with private actor LocalSet retained. Focused and unfiltered library checks pass, but the original hosted loop timeout was not reproduced locally. A two-CPU/22-thread stress experiment failed five other timing tests and is not passing evidence; hosted replay remains required |
| Combined corrective product gate | Main ran complete `make check`: concurrent library batch 269 passed/10 explicitly ignored/one workflow filtered, mandatory two-device phase passed, every default integration target passed, rustdoc/fmt/Clippy passed, 48 JavaScript, 20 fixture, 33 release, 27 installer-script and five documentation cases passed. PowerShell is the one explicit script skip; selected native/physical and hosted artifact gates remain separate |
| Hosted follow-up `849a15c` | Delivered `75c3c77` and `849a15c` to both remotes. Verify `40851468637` succeeds. Native `40851468824` has a completed ARM failure in consent probe 2/5: fresh-offer acceptance times out with regrant=false/publication=false, 14 passed/one failed. This disproves complete resolution by retiring fixture stream handles; exact job log is `123371717688` |
| Explicit-root source slice | Main passed complete `make check`, including 59 installer cases, three CLI cases, 48 JavaScript, 20 fixtures, 33 release and five documentation cases, plus separate ShellCheck. Installer scripts have 29 passes and one explicit PowerShell skip. A supplied public root and bundle are separately bounded/sealed before gh receives them; source mocks and CLI help inspection are not real air-gapped verification evidence |
| Native timeout label clarification | The earlier fresh-offer label covers opening a stream, writing the control envelope, reading acceptance AND writing binary metadata; it does not establish that acceptance itself was missing. Four substage labels now separate those awaits without changing frames, deadlines, retry policy or assertions. All 15 local consent cases and all-target Clippy pass; this is diagnostic refinement, not a causal repair |
| CLI invitation port allocation | The corrective commit hook failed because an invitation's TCP bind reused a port selected by a UDP probe. Invitation fixtures now let the live TCP listener allocate port zero; UDP hints are explicitly named. A held TCP listener reproduces the busy-address refusal without exporting a ceremony, then the real invitation advertises a distinct nonzero port. All four CLI workflow cases and all-target Clippy pass; no failing-test retry or deadline change was added |
| Host offline-isolation preflight | `unshare --user --map-current-user --net true` is refused with `/proc/self/uid_map: Operation not permitted`. No host security/kernel policy was changed. A strictly air-gapped install is not qualified by the source tests or this refused probe; dedicated-guest qualification remains an available separate path |
| Native key follow-up | On `78a911c`, Linux Verify succeeds but ARM native stops at 17/18 protected-key tests, before its consent steps. A separate logical initialization-lease defect has a deterministic local red/green regression and 19 standalone plus 19 library passes with Clippy. The [focused evidence](native-key-qualification.md) preserves the unconfirmed native causality and safe diagnostic boundaries |
| Native consent admission diagnosis | `e284fbf` Intel macOS job `123589176811` passes probes 1-2 then stalls in probe 3 reading fresh Accepted, after old-transfer revocation/no-publication checks. Source now traces numeric endpoint/first-three-stream phases only in test builds and adds production Room-policy reuse coverage while retaining every OneShot case. All 16 consent cases and Clippy pass locally; no causal repair, deadline increase or reconnect workaround is claimed |

The first intermediate check caught test-module placement; it was fixed, not
suppressed. A real-service test run emitted a bounded-close-grace diagnostic;
the focused consent reruns did not. Investigation reproduced it in six of ten
focused runs: Quinn's three-PTO close drain can exceed the one-second best-effort
grace. New deterministic natural-drain and cancellation/socket-rebind tests pass;
no resource leak or incorrect shutdown ordering was demonstrated. Diagnostics
and production timeouts remain unchanged; check actual socket-release errors.
The combined gate also caught missing dialog methods in the auth-reset DOM test
fixture. The fixture now models close/clear and asserts private draft removal;
both 401/403 regressions and the complete gate pass without weakening production.

Profile field updates preserve absent fields under lock; the additional callback
supports atomic subfield edits. ZIP provenance includes both archive formats.
Exact-artifact replay and independent review remain mandatory before delivery.

CI tooling also reports a Rustup repository-override notice and an upstream Node
Buffer deprecation in the artifact download action. Neither is suppressed or a
product compilation error; artifact digests and provenance verification succeed.
