# Verification Record

Executed evidence for the `0.8.1` candidate, 2026-10-27. This page records proof;
[TODO.md](../TODO.md) alone owns current delivery work and publication status.

## Software And Hosted Checks

| Gate | Recorded result |
| --- | --- |
| Product check | 155 Rust, six JavaScript and 11 packaging tests pass; format/Clippy/docs clean |
| Explicit media tests | Nine pass, including the four tests ignored by the default suite |
| Dependency audit | Root graph passes strict known-advisory and maintenance checks without ignores |
| Notices | 485 dependencies covered; local overrides included; no missing notice |
| Expansion source | Three exact source hashes; six upstream XMD/XOF vector groups pass |
| Independent OPAQUE | Official/restricted peers verified as separate graphs; six positive/negative cross-role cases pass |
| Conditional Hax | Actual macro branch compiles with upstream nightly; not a formal extraction claim |
| Existing private state | Agent-owned multi-member 0.8.0 room reopens and stops with 0.8.1 |
| Offline source | Extracted candidate builds all product targets with fresh Cargo home and `--offline --locked` |

Hosted checks for tag/source `fa6a29f`:
[GitHub](https://github.com/kpihx-labs/kk/actions/runs/40495406943) and
[GitLab](https://gitlab.com/kpihx-labs/kk/-/pipelines/2994030672).
Both required pipelines completed successfully; the GitHub job produced a draft.

## Exact Binary Evidence

| Binary SHA-256 | Scope |
| --- | --- |
| `37851dc0dbdbb679f17a3044c3a2b135c7a355b2a9730297f1fd1fc599c54324` | Local archive: 25 public CLI scenarios, 66 baseline scenarios and native contention |
| `7f200c5fb6bfc6b52a4109bd8e58b4400f744d4174449a4d32f8ea61487f7fa7` | Earlier CI candidate: public CLI and actual native guest decode under contention |
| `17ed3530b51d1e608e8236b285660d5792ab34479aaeef918d1ab88c40a4f5b6` | Signed draft: authenticated and unpacked; final VM replay not yet completed |

Retained local reports, relative to the repository:

```text
lab/tmp/public-room-cli-a5d3fb56c5a3.json
lab/tmp/kk-results-efbfd52d3e4d-1793097224671051686.json
lab/tmp/native-media-7b85d698f3d9.json
lab/tmp/native-media-0efce200331f.json
```

The earlier CI native run decoded 1,321 packets and fresh private-display pixels
while a verified file was active. Nine text samples had a maximum 1.570-second
acknowledgement at 125 KiB/s. Cleanup reports are empty; guests/network stop and
the previous QoS policy is restored. These are run observations, not a latency SLA.

## Do Not Overclaim

Synthetic media is not physical capture. A virtual display is not the operator's
desktop. Native hosted ARM tests are not a local ARM-device test. Quorum checks
are not Byzantine consensus. An authenticated candidate is not a published release.
Earlier attempts and checkpoints remain in Git and retained lab/tool artifacts.
