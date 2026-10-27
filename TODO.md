# Delivery State

Single source for current delivery status. Version: `Cargo.toml`.
Evidence: [verification record](docs/progress.md).

## Now

| Item | Actual state |
| --- | --- |
| Candidate | `0.8.2`, with corrected documentation and source manifest |
| Previous draft | Signed `v0.8.1` at `fa6a29f`; retained, **not publicly released** |
| Software | Previous runtime qualified; current candidate checks/CI must pass |
| VM evidence | Earlier binaries passed; no digest is transferred to the new artifact |
| Current work | Delivery resumed after operator-requested documentation/branch corrections |
| Branch name | `main` locally and by default on GitHub/GitLab; commits/tags retained |

## Before Publication

- [ ] Commit and qualify the corrected candidate on `main`.
- [ ] Build signed `0.8.2` assets without changing `v0.8.1` or its draft.
- [ ] Replay the exact signed draft executable on owned guests and verify cleanup.
- [ ] Verify final archive manifests, tag/workflow attestations and offline rebuild.
- [ ] Publish the same verified draft assets, without rebuilding them.
- [ ] Install the authenticated artifact; retain the managed previous version.
- [ ] Record final release URL, digest and qualification evidence.

## Not Qualified

| Scope | Work still needed |
| --- | --- |
| Windows/macOS native | Platform storage, IPC, lifecycle and real OS qualification |
| Physical media/Wayland portal | Explicitly consented hardware and portal lifecycle tests |
| Physical networks | Wi-Fi/AP isolation, sustained load and traversal evidence |

These are not silently closed by Linux compilation, virtual displays or a mobile
layout fixture. Public promotion requires the corrected source and exact final
artifact gates; the old draft is not a substitute for that qualification.
