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

## Required Scenarios

| Case | Acceptance | Execution state |
| --- | --- | --- |
| Profile creation/settings | Create the second profile through UI; names/independent keys visible, settings isolated | Morgan created through Alex's Profiles form; Alex remains the running profile; settings isolation pending |
| Room creation/invitation | Create/start on Alex, find private invitation from obvious action, no fingerprint/token confusion | Pending |
| Join/retry | Import on Morgan; invalid input remains editable; confirm matching authentication on both sides | Pending |
| Participant presentation | Profile/local names first in cards and selectors, exact IDs secondary; duplicate names remain separate | Pending |
| Messages | Both directions, Unicode, multiline, literal HTML, sensitive hiding and honest delivery states | Pending |
| File consent | Disabled receive refuses; enable works while online; withdraw changes actual backend state | Pending |
| Files | Both directions, binary/empty, hash verification, collision refusal, interrupted resume and explicit copy offers | Pending |
| Screen simulation | Two simultaneous independent synthetic screen streams; both receivers advance; stop one preserves the other | Pending |
| Other media | Generated audio/video where supported, explicit consent/device errors and independent cleanup | Pending |
| Recovery | Stop/restart one owner, preserve profiles/room identity, reconnect honestly without phantom sessions | Pending |
| Administrative parity | Explain data roles versus votes; controller retirement and member removal with actual backend permission errors | Pending; member-removal UI still needs implementation |
| Appearance/accessibility | Default midpoint exact, light/dark endpoints, keyboard/modal focus, responsive layout and contrast | Pending |
| Final cleanup | Stop both UI/room owners; close only owned tabs/window/panes; no capture or socket descendants | Pending |

## Evidence Boundaries

Native import currently exposed a desktop automation defect: AT-SPI window IDs
were passed to `xdotool` as XIDs, causing `BadWindow`. The authorized tool executor
is fixing that boundary without changing approval requirements. Invitations are
downloaded/imported through UI files; their secret contents are never inspected.
Activate the destination tab after moving it, and click the native file-input
button rather than the blank middle of the full-width input.

Record the exact source/executable hash and actual actions for each run. Distinguish
real UI clicks/form imports, API-only assertions, unit mocks, native codecs and
synthetic media adapters. Generated screen content proves routing/rendering,
not operating-system screen-picker permission or physical desktop capture.

Compare received bytes and decoded source-specific frames, not just a success
toast or an open WebSocket. Record errors and fix/retest their owner. Never call
the product fully qualified because only this two-profile case passes: native
platform, cross-device, network and final-artifact gates remain separate.
