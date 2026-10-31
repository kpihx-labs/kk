# Verification Record

The current tree is the **1.0.0 candidate**. [TODO.md](../TODO.md) owns its final
qualification and delivery sequence. The prerelease records below remain useful
baseline evidence, but they do not automatically qualify changed 1.0.0 bytes.

```text
source tests -> actual UI/native execution -> signed artifact -> install/rollback
    |                    |                        |
    +---------- different evidence scopes --------+
```

## Current Batch: What Must Be Proven Again

| Need | Action | Accepted result |
| --- | --- | --- |
| Integrate the refactor | Freeze source, run product/SDK/workflow gates | Current 1.0.0 tree passes all three local gates |
| Bind native behavior | Run Linux, Windows and both macOS hosted jobs | Exact source commit and actual job outcomes |
| Deliver final bytes | Build/attest artifacts, replay install/update/rollback | Installed bytes match authenticated subjects |
| Publish | Complete the final version/rebuild sequence in TODO | Public assets and installer verified after publication |

No new product/platform/artifact gate is claimed by this documentation cleanup.

## Qualified Baseline, Not Current-Tree Acceptance

Published prerelease `v0.9.0-dev.2`, source
`352e1007bb0db5c5c229f476a2f788760105944d`:

| Boundary | Recorded result |
| --- | --- |
| Integrated behavior | 358 library, 15 CLI, seven room CLI, four native-profile and nine platform-foundation cases pass on the common L3 candidate |
| Core/history | 24 focused cases cover expiry-stable committed replay, private replay, snapshot recovery and preservation of local history |
| Files/copies | 28 focused cases cover consent, retained copies, forward/cancel/resume/delete and durable terminal inspection, including restarted-process forwarding |
| Media | 64 library/CLI/JS/native synthetic cases; simultaneous synthetic sources coexist with chat and files |
| Actual graphical flow | Independent profiles pass pairing, chat/reply, file consent/forward/cancel/resume, history/restart, roles/revocation and simultaneous generated screens; compact rail and mobile layout checked |
| Final local gates | Product check and workflow check pass for the frozen candidate |

These counts describe different scopes/snapshots and must not be summed. A source
test, browser interaction, native host and signed artifact are different evidence.

## Hosted And Exact-Byte Delivery

| Gate | Binding and result |
| --- | --- |
| Tagged Verify | [41214581888](https://github.com/kpihx-labs/kk/actions/runs/41214581888): SDK/Hax, audit, x86_64/ARM checks, clean archives and signed provenance pass |
| Tagged Native | [41214581884](https://github.com/kpihx-labs/kk/actions/runs/41214581884): Windows and Intel/ARM macOS selected build, lint, smoke, storage/IPC/history/publication gates pass |
| Windows VM | Windows 11 Pro native storage, no-replace publication, identity exclusion, authenticated same-owner pipe/timeout recovery and bounded stdio pass; transferred snapshot `a506937965585651b21b44db8c3e44a2bd221e5c1d2d49b8c04f15a2d68467b3` |
| Published assets | [GitHub prerelease](https://github.com/kpihx-labs/kk/releases/tag/v0.9.0-dev.2), 16 unchanged assets; x86_64 TAR/ZIP/helper pass digest, manifest and tag/commit/workflow Sigstore verification |
| Exact executable | Attested helper SHA-256 `adf82179df86550c9e57103254f142e461674806ace70deadf086ba704eb0348` |
| Installation | Exact prerelease fresh install, update from 0.8.2 and rollback pass in isolated managed prefixes; installed bytes match the attested helper |

The operator's stable 0.8.2 link and profiles remain intentionally unchanged.
GitHub delivery is active; GitLab synchronization/CI is deferred. Native selected
tests do not imply every product workflow
ran on every operating system; see [platform scope](platforms.md).

## Evidence Limits And History

Generated canvases and owned virtual displays are not physical screen, camera,
microphone or Wayland-portal acceptance. Static mobile layouts are not a physical
phone test. Shaped guest traffic is not physical Wi-Fi/traversal or a sustained-load
SLA. OS notification grant/display and external security review remain separate
boundaries. Signed provenance establishes origin, not absence of defects.

Earlier releases, failed attempts, diagnostics, exact intermediate hashes and
ignored local report paths are preserved in
[Git](https://github.com/kpihx-labs/kk/tree/44f1a88c976eb44c7886df9233c980ce96ab6383/docs),
not repeated as current blockers. Lab interpretation: [verification lab](lab.md),
[native candidate qualification](native-candidate-qualification.md), and
[installation qualification](install-qualification.md).
Navigation: [documentation](index.md).
