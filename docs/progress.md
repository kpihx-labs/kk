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
| Native foundations | Executor reports seven Linux foundation tests; Windows/macOS workflow is authored but not yet executed |
| Combined source gate | `make check` passes: 209 Rust, 21 JS, 22 release and five docs tests; fmt/Clippy/rustdoc clean |
| Explicit native source gate | All four ignored native codec, private virtual-screen and child/owner lifecycle tests pass when invoked explicitly |
| Independent security follow-up | Static checker finds no remaining concrete defect in publication/revocation/cancellation, owner/borrower lifetime, reserved IPC or immutable archive snapshot paths |

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
