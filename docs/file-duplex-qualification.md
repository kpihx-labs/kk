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
The exact `a60aab6` VM/GUI evidence predates this production sender change;
new-byte replay and native gates are still required before a release claim.

Upstream issue [1421](https://github.com/quinn-rs/quinn/issues/1421) and fix
[1869](https://github.com/quinn-rs/quinn/pull/1869) describe older stopped-stream
cases, not proof for the current failure. The locked proto version is 0.11.19;
no dependency upgrade or registry-source modification was made on that analogy.
