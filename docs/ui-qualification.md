# Two-Profile UI Qualification

Required operator-requested gate: two independent profiles on this workstation,
two foreground UI processes in separate panes, two tabs in the existing Edge
`default` profile. This is not replaced by HTTP or mocked DOM tests.

## Isolation And Fixtures

- Disposable profile catalog: `/tmp/opencode/kk-product-20261027`.
- Ordinary test names: Alex and Morgan; room: Study group.
- No operator keys, profiles, real microphone, camera or desktop are used.
- Each UI has a distinct loopback origin, private bootstrap and independent room
  owner. Invitation contents/session capabilities never enter reports or chat.
- Existing owned QA window: `144025616`; Alex target:
  `BA8764503AB102AE7EBBE060E136CD0C`; temporary pane `%195`, `kpihx-labs:15`.
- Morgan target: `6BBA38682701A7DBBA6D450F175A3412`; temporary pane `%196`
  in the same window. Never close operator panes/tabs or replace the browser profile.
- Initial processes predated the final profile/join patches and were stopped.
  Initial source run used `b740b086eb63bc68fa40f86b1d107d84a6b171d153f6582af94644f4680f49a0`.
  Earlier retry-fixed `0.9.0-dev.1` run used executable SHA-256
  `30c2df240ec628e40e4ef92725caae8da5fb37ab9dd8deb0894784d148f876a9`.
  Alex origin: `http://127.0.0.1:35369`; Morgan: `http://127.0.0.1:45403`.
  This debug source build includes uncommitted native/capacity fixes, not a release.

## Required Scenarios

| Case | Acceptance | Execution state |
| --- | --- | --- |
| Profile creation/settings | Create the second profile through UI; names/independent keys visible, settings isolated | Morgan created through Alex's Profiles form; Alex remains the running profile; settings isolation pending |
| Room creation/invitation | Create/start on Alex, find private invitation from obvious action, no fingerprint/token confusion | Study group created and started in Alex UI; private invitation downloaded from issuance operation and imported into Morgan; no identity fingerprint used as invitation |
| Join/retry | Import on Morgan; invalid input remains editable; confirm matching authentication on both sides | Same-name retry after refused/expired attempt succeeds without deleting state; matching confirmation digests approved through both UIs; both show two members/epoch 1; invalid JSON retains editable details and persistent error |
| Participant presentation | Profile/local names first in cards and selectors, exact IDs secondary; duplicate names remain separate | Explicit opt-in renders remote names and message authors with self-announcement provenance; duplicate-name routing has API/JS coverage, rendered duplicate-name case pending |
| Messages | Both directions, Unicode, multiline, literal HTML, sensitive hiding and honest delivery states | UI sends both ways; multiline/Unicode match, literal image markup creates no image/script; sensitive incoming text is null/hidden; text also arrives during dual sharing |
| File consent | Disabled receive refuses; enable works while online; withdraw changes actual backend state | Disabled Morgan refuses with no file; enabling live updates backend; same open sender dialog retries successfully; active-transfer UI withdrawal pending |
| Files | Both directions, binary/empty, hash verification, collision refusal, interrupted resume and explicit copy offers | 65,536-byte pattern Alex-to-Morgan and empty Morgan-to-Alex downloaded through UI and SHA-256 matched; 54-byte note sent during dual sharing; resume/copies UI cases pending |
| Screen simulation | Two simultaneous independent synthetic screen streams; both receivers advance; stop one preserves the other | Real browser VP8 encode/QUIC/decode with two generated canvases, no physical capture; opposite source colors/hash changes verified; stopping Alex removes Morgan's receive tile while Morgan's publication and Alex's reception continue |
| Other media | Generated audio/video where supported, explicit consent/device errors and independent cleanup | Pending |
| Recovery | Stop/restart one owner, preserve profiles/room identity, reconnect honestly without phantom sessions | Morgan stopped/resumed through UI; same room/identity/epoch and two members; next sensitive message decrypted and explicitly revealed with exact body; zero stale media sessions |
| Administrative parity | Explain data roles versus votes; controller retirement and member removal with actual backend permission errors | Removal action exists in source; actual permission/advisory-role UI qualification still pending |
| Responsive workspace | Retractable desktop rail, mobile drawer, protected messages/composer and bounded panels at narrow/short sizes | At 881x786 expanded rail shrinks from 210 to 68px; rebuilt spacing refinement increases chat from 192 to 337px with composer visible and no horizontal overflow; narrow/short sizes pending |
| Notifications | Timed floating notices consume no document space; unresolved confirmations remain accessible | Live notice verified fixed, visible, no composer/chat displacement, hidden after 5.5 seconds; action center stays collapsed when empty; pending-action keyboard/overlay cases remain |
| Appearance/accessibility | Default midpoint exact, light/dark endpoints, keyboard/modal focus, responsive layout and contrast | Pending |
| Final cleanup | Stop both UI/room owners; close only owned tabs/window/panes; no capture or socket descendants | Pending |

## Evidence Boundaries

Native import currently exposed a desktop automation defect: AT-SPI window IDs
were passed to `xdotool` as XIDs, causing `BadWindow`. The authorized tool executor
is fixing that boundary without changing approval requirements. Invitations are
downloaded/imported through UI files; their secret contents are never inspected.
Activate the destination tab after moving it, and click the native file-input
button rather than the blank middle of the full-width input.
Native focus/edit interfaces did not safely support selection. Both old UIs were
stopped through their authenticated actions; navigating the owned tab closed the
native chooser. Functional imports use `lab/scripts/13-ui-file-fixture.py` with
opaque disposable fixtures and an exact owned origin/target. This is real browser
file-input handling, not proof of operating-system picker automation.

Record the exact source/executable hash and actual actions for each run. Distinguish
real UI clicks/form imports, API-only assertions, unit mocks, native codecs and
synthetic media adapters. Generated screen content proves routing/rendering,
not operating-system screen-picker permission or physical desktop capture.

Compare received bytes and decoded source-specific frames, not just a success
toast or an open WebSocket. Record errors and fix/retest their owner. Never call
the product fully qualified because only this two-profile case passes: native
platform, cross-device, network and final-artifact gates remain separate.

Observed screen samples: Alex receives blue `(35,77,195)` and Morgan red
`(186,24,42)`, with opaque received-frame hashes changing. Alex decoded 10,497
frames before its publication stopped and 11,173 afterwards while still receiving.
Both endpoints reported two sessions/one subscription during dual sharing, with
zero backend drops at the sampled observations; these are not a throughput SLA.
Pattern SHA-256: `7daca2095d0438260fa849183dfc67faa459fdf4936e1bc91eec6b281b27e4c2`.
Empty-file SHA-256: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
Received files still display content-addressed names; original filename presentation
and the misleading generic contact-policy denial text require refinement.

Name-sharing run: executable `b2daf1e495c530daea66c6df864d67f6945925c19b0217e34aca81acc41b1de5`;
Alex `http://127.0.0.1:34241`, Morgan `http://127.0.0.1:39541`. Both started with
sharing off and unknown remote names. Explicit Settings opt-in in each tab led to
actual peer cards and message authors showing Alex/Morgan with `self_announcement`
provenance. The checkbox and its privacy explanation were checked in the browser.

Revision-conflict run: executable
`e57c0c305a9da410d40a6b282ad17b3f61c2b100c4ca5311d361fd0b9c21e50d`.
Morgan's old unchecked dialog survived CLI on/off updates; its later opt-in and
clarity edit were refused atomically. The dialog reloaded consent as off, retained
clarity 70 as an editable draft and showed a conflict. Explicit retry saved clarity
70 without enabling sharing. The conflict text exposed an offscreen confirmation
button; fixed-footer/scrolling-content CSS is added and needs rebuilt replay.

Static layout fixture: owned blank target `D1A03291C796609D31262AE588ACD932`.
The 34 sandboxed, script/network-free rendered cases failed: every case had toast/
composer overlap, and small cases additionally had element overflow/chat-space
failures. Report: `/tmp/browser-proxy-autosave/page-evaluate_20261028_083639_928813.json`.
After CSS corrections, all 34 unchanged cases pass. Report:
`/tmp/browser-proxy-autosave/page-evaluate_20261028_084604_007904.json`.
CSS SHA-256: `ea6dabdac4ca0bc15d1f7f1699ec2e2b59a302db9226fee20063995192290ebc`.
These are static fixture findings/results, not interactive/native-mobile evidence.
Five long Settings/conflict-error modal cases also pass at all five dimensions:
`/tmp/browser-proxy-autosave/page-evaluate_20261028_085429_260478.json`.
All 39 static cases pass, including bounded scrolling and stationary modal controls.

Responsive/stale-settings run: executable
`35e004318fba11ffa9c4d8daf61942f3603ffc52c932994d56edf45f647ef7d9`.
Morgan opened Settings with sharing enabled; CLI disabled sharing while that
dialog remained open. Saving only clarity 60 preserved opt-out and compact mode.
This is actual CLI/UI concurrency evidence for changed-fields-only submissions.
Display preferences and device controls were rendered without submitting capture.
Viewport emulation through the approval-gated proxy timed out; no approval was
bypassed and no narrow/short rendered pass is claimed. The baseline observation
found only 192px of chat at 881x786. Rebuilt spacing replay used executable
`e286b483004b99ce50722e8693dbb516e03f3af4ce405d0112b9c035cbee2295`:
Morgan at `http://127.0.0.1:39585` has 337px chat, visible composer and no horizontal
overflow at the same size; compact sidebar survives foreground restart.
