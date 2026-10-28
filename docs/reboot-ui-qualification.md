# After-reboot UI qualification

## Scope and fresh dataset

2026-10-28. This is a new disposable dataset under
`target/qa-after-reboot-20261028`, not the old `/tmp` QA dataset, which was
purged at reboot. The foreground UI processes were provided by the main agent.
The tested binary was reported as 0.9.0-dev.1, compiled at HEAD 82 plus the
working-tree UI fixes; this document does not independently attest its build.
This is functional evidence only, not security acceptance. Independent review
reported IPC ancestor and final-entry races, incomplete namespace recovery, and
lock-entry binding concerns; correction and security review remain with main.

The existing default managed Edge profile and window 144025877 were used. Alex
reused its blank startup tab; Morgan was opened in one additional tab in that
same window. No original K&K profile, key, launch HTML content or invitation
was read. Both pages displayed the expected identities and public fingerprint
prefixes: Alex `5ea92dcc:9e0f6cf1`, Morgan `a6946524:053a11dc`.

## Observed results

| Case | Result | Evidence |
| --- | --- | --- |
| Independent UI startup | Pass | Both pages displayed their distinct names, fingerprints, version 0.9.0-dev.1 and READY ON THIS DEVICE. |
| Morgan settings display | Pass, old process | Settings opened with Morgan's name, name sharing unchecked, expanded sidebar, neutral hue, clarity 50 and saturation 20. Closed without saving. |
| Create room | Pass after correction | Initial SUN_LEN refusal and runtime-only reservation retry failure were reproduced. Corrected source creates `reboot-study` at the unchanged long path and serves Reboot Study; no reservation or key deletion used. |
| Invite, enroll, join and SAS | Pass with synthetic import | UI export returns HTTP 200; controlled private ceremony bytes enter the real file-import handler. Matching public SAS and exact peer fingerprints were compared before both visible approvals; Morgan joined and both show two members at epoch one. Native file picker and completed browser disk saving are not qualified. |
| Messaging and literal markup | Pass | Both directions show event-queue acknowledgement, not a read receipt. Two equal Reboot QA bodies have distinct authenticated references; markup remains literal with zero injected b elements. |
| Search and encrypted history | Pass for capture/search | Explicit enable confirmations on both profiles. Reboot QA finds two stored records with IDs 2 and 4 and only two matching rendered entries, not four duplicates. Synthetic sensitive text is hidden in live API/DOM and has zero stored matches on Alex. Disable/clear and final process restart remain separate gates. |
| Room and whole-UI restart | Not tested | Requires a created room and coordination with the main agent for process restart. |
| Synthetic screen sharing | Not tested | Basic two-party flow was blocked. No microphone, camera or desktop capture was attempted. |

## Initial handoff and cleanup (superseded)

On the requested retry, the main agent reported a successful rebuild with the
SUN_LEN implementation. The two existing UI processes were still running the
earlier binary. The tmux guard rejected the requested Ctrl-C before execution:
`pane %124 is leased by another OpenCode session (2142s active)`. No pane was
interrupted and the new build has not yet been exercised. The owning session
must release its leases or perform the two restarts using the same profiles and
data root; retry the existing room name without deleting the reservation.

The failure requires investigation of Unix socket path length in the fresh
dataset. Do not claim a create or pairing pass based on the click. Both UI
processes remain under main-agent control. The two QA tabs remain in window
144025877 for coordinated continuation: Alex target
`80568A6BE7D0E326A95C8506F3F36DDC`, Morgan target
`326AD78F990C95AA56E62742349240B6`. After continuation, close only the Morgan
QA tab and restore the Alex startup tab to `about:blank`; preserve any operator
tabs and the existing window. No cleanup of profile data was performed.

## First recovery observations (superseded)

Main stopped the original processes and restarted the same profiles in existing
idle panes `%127` and `%129` of its current `pro:0` window. The identities stayed
unchanged. Launch files were navigated without reading their contents; activate
the intended tab before pointer interactions or asynchronous evaluations.

The old failed create left only `node.lock`, not a manifest or private room state.
Start correctly refused missing state. A same-name create then reported
`File exists`: creation did not use the strict runtime-only retry supported by
join. Main changed that backend reservation path and added a passing regression
covering active-owner refusal, preserved lock inode/profile identity, successful
retry, and refusal to replace durable or ambiguous state. A fresh binary/UI
replay of this correction is still required; no pairing/history/media pass is
claimed from these observations. Both restarted UIs subsequently acknowledged
their authenticated HTTP shutdown actions and returned to their shell prompts.

## Additional recovery observations (superseded)

Main verified the old foreground child PIDs before sending SIGINT; both original
processes returned to their shells with tmux completion notifications. New
foreground processes used the identical profile paths in the current `pro:0`
window, panes `%127` and `%129`; no extra panes or profiles were created. The
private launch files were navigated without reading or printing their contents.
Replaying the consumed launch filename produced an expected file-not-found page;
browser history returned to the already authenticated application. Activate each
tab before coordinate clicks: background clicks did not open the intended modal.

Both identities survived. The old failed creation left only an empty `node.lock`
in `rooms/reboot-study`. Start refused the missing definition; a same-name Create
retry showed `File exists` while retaining the editable form. Main added a backend
runtime-only create retry and a passing regression covering active owners,
unchanged lock inode and identity, completed rooms and retained partial roots.
The real UI retry still requires rebuilding and relaunching that corrected source.

Both replacement UI processes then acknowledged authenticated shutdown requests
and returned to their shells. No files or keys were removed to bypass either
failure. Pairing, history, file consent and synthetic sharing remain unqualified
on this fresh dataset until the corrected build completes those actual flows.

## Corrected-source continuation

The operator explicitly approved nonrecursive group-write removal on the
workspace/repository/target ancestors; each changed from 0775 to 0755. Current
foreground QA uses existing panes in the main agent's window and the same data
root. Rapid close/reopen exposed a queued dialog-close event erasing the new form;
modal revisions, stale-result guards and actual-close cleanup now have regressions
and the rebuilt Edge page preserves all three joining inputs.

Opaque import uses `13-ui-file-fixture.py --ceremony-invitation`: only a private
app-generated ceremony inside the checkout's disposable target tree is accepted.
Its structural, ownership, mode, link, symlink and size guards have offline tests.
The backend invitation body and keys were not printed. Existing Downloads entries
were from the previous day and were not treated as a successful new UI download.
The completed cases above supersede earlier blocked observations, not native
picker, hardware capture, network traversal or cross-platform qualification.

## Continued functional evidence

The subsequent file-consent, byte-integrity and simultaneous generated-screen
results are preserved in [the focused data/media report](reboot-data-media-qualification.md),
including source-stop isolation, cleanup and the limits of those observations.
