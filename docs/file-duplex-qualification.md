# File Sender Duplex Progress

The backend stream sender owns this invariant; CLI, contact and room adapters do
not implement separate upload/receipt loops. [File contract](files.md).

## Deterministic Rejection Regression

A real pinned mutual-TLS peer sends a valid resume prefix, then closes its response
without a receipt. It deliberately retains but never drains/stops the request,
so the finite receive window blocks a sufficiently large body upload.

| Source behavior | Observed result |
| --- | --- |
| Serial upload, then receipt | The public sender cannot observe the already-available response EOF; the new regression fails at its five-second outer bound |
| Concurrent bounded receipt and upload | The same regression returns Integrity in approximately 0.13 seconds in the focused Linux run; not a latency guarantee |
| Premature correct receipt while body is blocked | The sender still reaches its original operation deadline and refuses; receipt alone cannot report success |
| Existing transfer suite | Nine binary/empty/resume/tamper/nonclobber cases pass; all-target Clippy passes |

The correction joins both required outcomes rather than racing them for success:
complete authorized upload plus FIN, and exactly the expected digest with response
EOF. Any error cancels its companion future. Chunk/input/receipt bounds, wire
format, authorization callbacks and production deadlines remain unchanged.

## Separate Native Consent Investigation

On `114705b`, ARM job `123676620019`, probe 4 fails regrant=false/publication=false.
The old receiver logs DENIED at 19:25:14.682 UTC. The fresh client only begins its
new offer at 19:25:46.330, at the whole-case deadline. Its last stage label does
not establish a long fresh-Accepted wait. New elapsed traces separate withdrawal,
the revoked body write, response EOF and re-enable/fresh-offer entry.

The deterministic early-EOF defect is independently reproduced and corrected;
it is not established as the cause of that manual wire-peer/native failure.
Existing native cases, same-connection reuse and assertions remain intact.
The revoked-body fixture now also polls its terminal response while attempting
the old body: a receiver that rejects need not drain it to free upload credit.
EOF cancels only residual old writing; an ACK still fails the unchanged assertion.
Publication-paused cases retain their existing full-body/barrier sequence. Fresh
offer success and both lifetime-budget assertions are unchanged. After the earlier
16-case pass, a new blocked-credit wire-peer regression makes the serial fixture
mutation fail at eight seconds; restored duplex handling passes in approximately
0.03 seconds. All 17 local cases and Clippy pass. The helper is shared by receive
and fetch wire peers, not copied into production adapters.

`1f846ef` Verify passes; native Windows/ARM pass, but Intel job `123700874544`
fails probe 4 in `live_fetch_withdrawal_does_not_revive_after_regrant`, at the
whole-case bound (33.11 seconds observed). Its remote wire peer had the same serial
body/receipt sequence. Hosted qualification of the shared fixture correction is
initially pending; no assertion/deadline change, reconnect or retry hides this failure.
At `cfa8145`, Verify `40950864252` and Native `40950864320` both succeed. ARM job
`123718300554` and Intel job `123718301059` each pass five fail-fast probes of all
17 consent cases (zero ignored/failed). That qualifies the selected corrected
fixture snapshot, not every unexecuted native product feature or a Quinn root-cause theory.
The exact `a60aab6` VM/GUI evidence predates this production sender change;
new-byte replay and native gates are still required before a release claim.
The subsequent `cfa8145` executable is now independently replayed; its [binding and
evidence](install-qualification.md#corrected-fixture-checkpoint-cfa8145) do not authenticate a new release.

## Executed Production-fix Checkpoint, `1f846ef`

Verify run `40945852870`, artifact `12930301087`, supplies executable SHA-256
`b9e757c16d62ff7f5ea2427c6aa8abb4618298d05173ecd4da18d6072422d620`.
Both TAR/ZIP verify all 852 members and all eight checksum subjects pass.
That exact executable passes the 25-scenario three-guest CLI gate, report
`lab/tmp/public-room-cli-e66cd14cb9c0.json`, with failure=null/cleanup_errors=[].
Native private-display/bulk/control replay also passes; [metrics and retained
diagnostics](native-candidate-qualification.md#production-fix-checkpoint-1f846ef).
This is unsigned candidate functionality, not a new authenticated installation.

Upstream issue [1421](https://github.com/quinn-rs/quinn/issues/1421) and fix
[1869](https://github.com/quinn-rs/quinn/pull/1869) describe older stopped-stream
cases, not proof for the current failure. The locked proto version is 0.11.19;
no dependency upgrade or registry-source modification was made on that analogy.
