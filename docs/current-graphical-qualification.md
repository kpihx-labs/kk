# Current Candidate Qualification

Actual observations on 2026-10-30. [TODO.md](../TODO.md) owns the product plan;
[integration evidence](current-integration-qualification.md) retains earlier source snapshots and failed attempts. This is not release acceptance.

## Exact Artifacts And Ownership

| Artifact or process | Verified boundary |
| --- | --- |
| Earlier GUI executable | `target/x86_64-unknown-linux-gnu/debug/kk`, SHA-256 `46f6700ce3ffd6b995826edd2ba1dd2ab588a1d73ead60814d4425951c7646f5` |
| Prior separate-target candidate | `33b76c1c4d14c24ae38f3ca9c72e29f6fd282ceddc38d476075d2726e312047f`: source controls and native invitation copy observed; then rebuilt to add explicit Paste |
| Preserved old executable | `target/debug/kk`, unchanged SHA-256 `18feeb36c49b50f60c7043c70b67e090e2cf60da75057f7d2afd0b682d684617` |
| Current A | `/tmp/kk-current-a-20261030`, pane `%162`, HTTP `45684`, room `utility` at `127.0.0.1:45584` |
| Current B | Existing `/tmp/kk-graphical-b-20261030`, pane `%125`, HTTP `45682`, intended room bind `127.0.0.1:45582`; identity preserved across candidate restart |
| Protected processes | Operator pane `%160` untouched; old QA `%124` is leased by another session, its stop was refused and not forced |
| Rust source qualification | Temporary task-created pane `%163` runs the combined target-specific Rust tests, excluding the separately mandatory HTTP workflow; completion not yet recorded |

The earlier A profile and every retained profile remain intact. A distinct
candidate-specific A fixture was initialized in an empty root, not by replacing
an old identity, relaxing permissions or overriding the other session's lease.
Both candidate UIs reuse the existing owned Edge tabs. Private launch files are
navigated by the explicit opener without printing their capabilities.

## Executed Graphical Steps

1. Candidate A initializes through its actual control, creates `utility`, starts
   the room, stops and reopens the same stored identity/room across the candidate
   rebuild. File transfers and invitation copy/Paste controls are present in the
   actual served assets, not only source fixtures.
2. A creates an invitation with TCP port zero, letting the live listener choose
   its port. Explicit Copy invitation succeeds and reports only a generic notice.
   There is no invitation payload in agent output and no downloaded file.
3. B opens Join with the intended nonsecret alias/bind. Native keyboard paste
   does not populate the invitation; no joining request is sent on that failure.
4. The new explicit Paste invitation control requests the browser's clipboard
   permission. Actual permission state is `prompt`, the button remains reserved,
   and the intended B page has focus. Operator approval is requested; no browser
   permission override or forced grant was used.

The invitation may expire during that native permission wait. Do not blindly
reuse it after approval: verify the operation and create one fresh invitation
only when the recipient can consume it. No current two-profile graphical pairing,
remote message delivery, file receipt or simultaneous browser media is claimed.

## Source And Native Checks

- Five new JavaScript transfer-monitor cases pass: receipts versus counters,
  exact-ID reinspection, explicit cancellation/request acknowledgement,
  foreground retirement, unavailable status without a duplicate send.
- Two invitation-copy cases pass: explicit-only clipboard write and retirement
  after profile/operation/session change without copying late data.
- Three explicit Paste cases pass: no read on opening, no duplicate read or
  replacement of edits, stale-dialog/profile/input retirement, denied permission,
  invalid/oversized input with no automatic joining request.
- Installer scripts: 39 cases executed, 38 passed and PowerShell explicitly
  skipped. Release scripts: 34 cases passed. These are source/mocked packaging
  cases, not signed current-candidate installation.
- Six explicitly ignored native media cases were invoked and passed, none
  skipped: codecs/RTP decoding, bounded child cancellation, owned virtual-display
  pixels, owner-SIGKILL cleanup, stopping backpressured video while audio continues,
  and synthetic codecs across a concurrent real QUIC meeting. Only generated
  media and an owned virtual display were used, not physical capture.
- Those media cases used library executable SHA-256
  `a38fd6f893cfef27731a05fc8dda6d57809b5fd7a39ee04ee7a29f78394f20fd`.
  The library snapshot predates only subsequent browser-asset/Paste work; it is
  not presented as the GUI executable above.

## Automation Dependency And Cleanup Boundaries

Earlier blocking browser-proxy repair attempts changed its canonical repository,
not global permissions: printable key codes now use `KeyV`/`Digit1` instead of
logical characters, Windows virtual codes are not reused as Linux native codes,
and only the exact ordinary paste chord binds Chromium's paste editor command.
A red/green dispatch regression, all 46 page-action tests and Ruff pass. Generated
help now explains key/modifier fields. Editable installation was confirmed; only
the daemon was restarted, preserving the live Edge profile. This is not claimed
to fix native paste: the live attempt still failed, hence the explicit UI Paste
and accurately observed native permission boundary. The final repair restores original virtual-key/shortcut behavior, corrects physical codes/help only, and permits scoped paste; [f817 GUI proof](ui-candidate-f817-qualification.md) records the actual successful ceremony and later workflows.

Six expired/empty task downloads were trashed and a fresh Downloads glob returned
no match. An attempted removal of invitation-only download-history cards reported
12 clicked controls, but verification still found 12 records; history cleanup is
NOT passed. No other download was removed. The owned inspection tab was returned
to blank. The initial `make notices-check` probe had no matching Makefile target;
no notices audit is inferred from that rejected command.

## Combined Source Gate Follow-Up

The first combined Rust library execution failed: 221 passed, 121 failed, 12
ignored, mandatory HTTP workflow filtered. Most failures were unsafe-ancestor
refusals from default fixture roots under the caller's `002` umask, not passing
production flows. A test-only shared constructor now requests `0700` explicitly;
unit and integration fixtures reuse it without changing the process umask or
production identity/profile/IPC validation. Existing unsafe-path tests retain
their deliberate unsafe permissions and assertions.

After that correction, the full library passed 343 cases with 13 explicit ignores
and the mandatory HTTP workflow reserved; binary unit tests passed eight. The
combined command then reached CLI integration and failed three of ten cases on
the same fixture-root contract. Those constructors now use shared private roots;
related integration fixtures were also corrected. This was not a complete gate.

A later combined library run exposed one physical writer-lock release case:
342 passed, one failed, 13 ignored. Its immediate post-drop assertion can observe
unrelated concurrent fork aliases, just like the earlier forwarding-lock case.
Both physical-release cases now run unchanged assertions in mandatory isolated
child executables, following the existing socket-lifetime test pattern. There is
no retry, deadline increase, ignored mandatory parent or production unlock change.
The corrected combined replay is still required; passing focused cases is not
accepted as its substitute.

Canonical JavaScript now passes 122 cases; all five docs checks pass. Offline-kit
source fixtures pass 21 cases, not a real isolated install. The actual read-only
`release.licenses()` path validates override sources and notice digests: 492
dependencies, 848 notice files, none missing; `object@0.39.1` carries its Apache
and MIT texts. The collector has no help parser; its earlier silent invocation
collected nothing, and the tracked notice manifest/license tree are unchanged.

Accepted source checkpoint: library 343 passed/14 explicit ignores, binary eight, all integration targets and doctest passed; mandatory HTTP and six native media cases pass on lib-test SHA `3f21e0eb...c62724ae`. All-target Clippy passed after moving unchanged publication tests into their own module and fixing a needless borrow. Later CLI job controls pass four parser cases, a real process admission/status/cancel/owner-retirement case, and targeted Clippy.
Actual GUI f817 subsequently passes bilateral pairing, bidirectional chat/reply, live file consent, verified forward while receiving is off, cancellation before/after handoff and a 16-MiB resume from 131,072 bytes; use its [exact-image record](ui-candidate-f817-qualification.md), not earlier candidate hashes. Initial task download/history cleanup was verified with native clicks after the failed scripted-click attempt; later disposable test artifacts still need final cleanup.
