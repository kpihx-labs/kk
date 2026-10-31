# Working-Tree Integration Qualification

Source checks on 2026-10-30, after `ab28491`. These are not release, signed
artifact, native macOS execution or full-product acceptance claims.
[TODO.md](../TODO.md) remains the only product plan.

## Executed Source Checks

| Gate | Observed result |
| --- | --- |
| Installer library | 19 passed, none ignored: ELF, pure Mach-O CPU/minimum-OS/dependency bounds, verifier files, publication finalization, process cancellation/reaping |
| Room service | 23 passed concurrently, none ignored: live consent/regrant/budgets, saturated lane, public receipts, real mutual-TLS routing, retained-inode forwarding and restart without receiving consent |
| Transfer jobs | Five passed: bounded active jobs/history, descriptor lifetime after HTTP temporary-file cleanup, receipt-only completion, post-handoff cancellation uncertainty and shutdown |
| Owned copies | Seven passed: five file-owner checks plus two real-loopback forwarding cases, including runtime restart with reception disabled |
| HTTP owned-copy preview | New `ui::tests::files_http_tests::owned_copy_http_preview_is_explicit_authenticated_and_keeps_verified_inode` passed: authenticated PNG bytes/headers, retained-inode download/preview after pathname replacement, refusal of later tampering, SVG attachment allowed but raster preview refused, no foreground room or reception grant |
| Two-device HTTP/QUIC | Exact mandatory `ui::tests::ui_http_pairs_independent_devices_owns_foreground_rooms_and_redacts_sensitive_events` passed in 10.92 seconds on the final test executable below |
| JavaScript | `make ui-check`: 112 passed, none skipped. File and recovery suites are now included in the canonical gate rather than only invoked manually |
| Formatting | `cargo fmt --package kk -- --check` and `git diff --check` pass after formatting the returned source batch |

The service batch ran on test executable `a622e3d6...fa211ddd` before the new
HTTP test was added. Installer, jobs, owned-copy and mandatory two-device cases
were rerun on the final test executable. Counts overlap; do not sum them as
distinct test coverage.

| Executable | SHA-256 |
| --- | --- |
| Final library test snapshot | `0f168d1c3a091c727a9af3865978fe3b350d8bd8c04bfc580036110dde53533b` |
| Preserved, older graphical candidate | `18feeb36c49b50f60c7043c70b67e090e2cf60da75057f7d2afd0b682d684617` |

The test executable is `target/debug/deps/kk-d2cd2cd1146ee40d`. The graphical
candidate is `target/debug/kk`, deliberately not rebuilt during browser QA.

## Failure And Diagnostic Boundaries

Initial forwarding tests failed before networking with `unsafe identity path
ancestor`. The agent's `TMPDIR` parent was group writable and its `umask` was
`002`; selecting `/tmp` alone still left default fixture roots group writable.
Restrictive `umask` established the cause, then service fixtures were corrected
to request `0700` explicitly. The corrected 23-case batch passes under the
original `002` umask with `TMPDIR=/tmp`. Production identity/IPC checks and
existing directory permissions were not weakened or modified.

One passing service case emitted `bounded close grace elapsed` with Quinn
drivers listed. `ControlNode::shutdown` logs this before dropping the node,
terminating/reaping its owned drivers and awaiting storage release; it does not
treat the grace timeout as evidence of socket release. No socket-release error
was emitted. The earlier drain explanation and deterministic release tests are
retained in [progress](progress.md); this observation alone proves neither a
new leak nor a causal fix.

## Actual Graphical Boundary

The new isolated B profile is `/tmp/kk-graphical-b-20261030`, directly below the
root-owned sticky temporary directory, not the unsafe `/tmp/opencode` parent.
Its UI runs in existing pane `%125`, port `45682`, on the preserved candidate.
The existing owned Edge tab was reused through the explicit private opener;
no launch capability or key was printed. Initialization became visible after
activating the background tab. A remains in `%124`, UI port `45680`.

A produced an invitation through its actual controls and downloaded it. B's
join form has local name `first`, bind `127.0.0.1:45582`, participant role. Native
chooser input arrived as a misspelled `Doznloads` path. The attempted alternate
keyboard backend was rejected by `desk-proxy`; no successful import or pairing
is inferred. Operator file selection is requested without disclosing invitation
contents. No two-profile graphical delivery is claimed.

Follow-up: the operator selected the file successfully; B confirmed an imported
file and retained the intended nonsecret fields. Starting pairing then returned
connection refused while A reported the previous invitation deadline exceeded.
A fresh invitation was created through the UI. Its download handler returned,
but no fresh file was established in Downloads. The extension rejects the
unavailable downloads metadata API; native focus of the exact owned K&K window
is also refused by AT-SPI. Manual foreground focus is requested. No focus
fallback, global keystroke continuation or approval was attempted after refusal.

This qualification does not close the missing graphical transfer-job controls,
native Windows protected runtime, actual macOS/WSL execution, physical media or
signed current-candidate installation/update/rollback gates.

## Transfer Adapter And Invitation Follow-Up

The formerly missing transfer-job HTTP/browser wiring is now implemented in
source. Upload/forward requests opt in with `background=true`; synchronous
callers remain compatible. Authenticated owner-scoped list/status routes and
origin-guarded cancellation submit the existing local typed commands, not a new
registry. The browser exposes File transfers, progress, explicit cancellation
and reinspection. Retired foreground/dialog responses cannot control a new
lifetime. Only a valid completed receipt is labeled durable recipient storage;
request acknowledgement and post-handoff uncertainty remain distinct.

The mandatory real two-device HTTP/QUIC test now also passes asynchronous
forwarding while incoming reception remains disabled, an actual 128-KiB upload
whose temporary path is retired after descriptor admission, receipt-only job
completion, no digest in sensitive status, owner-ID isolation, missing session
and foreign-origin refusal, job listing and harmless cancellation after receipt.
Execution returned 11.78 seconds with no ignored case. Library Clippy with
warnings denied passes; canonical JavaScript now passes 119 cases, including
five transfer-monitor and two explicit invitation-copy/stale-read cases. All
five documentation checks pass.

Copy invitation is an explicit clipboard action: no automatic clipboard access,
no secret in notices and no download. It checks live operation/profile identity
again after fetching; stale/expired/session-retired reads cannot copy. The UI
warns that a private capability remains on the clipboard and should be cleared.
Actual native clipboard execution remains a graphical qualification gate.

Six task-owned Downloads files were inspected by pathname/size/mtime only and
trashed using the installed safe `rm` wrapper: `kk-invitation.json`, suffixes
`(1)`, `(2)`, `(4)`, `(5)`, `(6)`. The last three were empty; all invitations had
expired. A fresh scoped glob returns no matching Downloads file. No profile,
received copy, unrelated download or browser history entry was removed.

A separate-target candidate build is armed in `%162` with two Cargo workers,
preserving `target/debug/kk` and existing running profiles. Graphical validation
must inspect the resulting actual bytes; source and browser mocks are not its
substitute. The earlier native-focus inference was incorrect: the operator
confirmed the K&K window was already visible on the second screen. Refused
AT-SPI focus does not establish that a window is absent or not foreground.
