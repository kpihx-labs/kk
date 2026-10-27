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

CI tooling also reports a Rustup repository-override notice and an upstream Node
Buffer deprecation in the artifact download action. Neither is suppressed or a
product compilation error; artifact digests and provenance verification succeed.
