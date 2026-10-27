# Pending Work

## 0.8.1 Qualification In Progress (2026-10-27)

The local product dependency barrier below is resolved without an ignore. The
0.8.1 root graph passes strict audit, all 155 Rust/six JavaScript/11 packaging
checks pass, and the independent SDK/vector/Hax gates pass. All 485 dependency
notices are present. Full review: `docs/dependency-overrides.md`.
Remaining release work is the frozen 0.8.1 VM/native replay, clean-source archives,
offline rebuild, hosted x86_64/ARM64/audit/SDK gates and final signed publication
and installation. No final tag is created by this local qualification. Native
Windows/macOS and physical device/portal qualifications are not silently closed.
The 0.8.0 hold/probe entries below are dated historical evidence.

## Release Blocker (2026-10-27)

Publication is blocked by the unsuppressed RustSec result, not by a missing
functional command. The current product graph still has four crypto advisories
and two maintenance warnings; see SR-004 in `docs/security-review.md`. No version
tag is created, and the release job now depends on the strict audit job.

The pinned-source experiment under ignored `target/dependency-probe` compiles
after also selecting the matching upstream `openmls_traits` snapshot. The default
storage mode remains synchronous; no product storage migration has been applied.
The experiment removes the three Libcrux advisories and basic-toml warning, but
still has crypto-bigint 0.5.5 through OPAQUE/VOPRF and proc-macro-error2 through
conditional Hax tooling. It is not promoted into `Cargo.toml` or `Cargo.lock`.
Next work is a reviewed dependency remediation, not an audit ignore, followed by
qualified rebuild/replay and final publication. Native Windows/macOS and physical
capture/portal qualification remain separate uncompleted platform gates.

Current 0.8.0 freeze (2026-10-26): all 25 public room/copy and 66 baseline scenarios
pass on the final binary, followed by actual native private-display decode under
limited bandwidth with simultaneous verified file and acknowledged text. The
requested second software review is complete; its two extra counterexamples are
fixed with regressions. Local checks pass 155 Rust, six JavaScript and seven release
integrity tests. All 474 dependency notices are covered and offline vendoring works.
Remaining delivery work is remote CI/ARM64, provenance/archives, local install and
dual-remote source/tag publication. Native Windows/macOS and separately consented
physical device/portal qualification remain unsupported/unverified, not completed
by these Linux gates. The older current-pass notes below are historical snapshots.

Replica update (2026-10-26): explicit audience-bound offers, selected-peer lookup,
verified pull and withdrawal pass the public 25-case three-VM gate, including the
original VM powered off. The GUI and HTTP adapters also pass copy publication,
lookup, verified storage and withdrawn-fetch refusal. Registry offers are bounded,
membership-epoch-bound and never silently restored after service restart.
Next current gate: actual private-display native capture/decode with live bandwidth
shaping, bulk transfer and control/text measurements. Deep review and portable
publication follow; the newly added native harness is not itself a passing result.

Latest completed integration (2026-10-26): the native grey UI passes browser
pairing, cold resume, escaped text, consented file receipt and live synthetic
VP8/Opus encode/transport/decode in the existing configured profile and owned
window. Both themes pass a 390 x 844 script-free layout fixture with retained
errors and an unobstructed composer. Full host check: 147 Rust + five JavaScript
tests. Socket-backed stdio and UI session-expiry regressions are included.
Continue with remaining native contention, replica availability and VM evidence,
then the dedicated deep audit and portable publication. Browser synthetic media
and responsive fixtures do not waive physical capture/mobile/platform gates.

Operator correction (2026-10-26): the modal backdrop still looked green because
the neutral override selected a dialog inside `.workspace`, while the actual
dialog is its sibling. Target the real dialog and verify its computed backdrop,
not just the panel palette. Continue autonomously while the operator is away;
use ordinary browser actions or alternative evidence instead of waiting for
`do raw` HITL. Never relax browser or application security to make QA work.

Browser QA instruction (2026-10-26): use the operator's existing, fully configured
browser profile and the separate agent-owned window already created there. Do not
continue using the smoke browser profile. Keep unrelated operator windows intact.

UI direction reaffirmed by the operator (2026-10-26): prefer neutral grey themes,
in the visual family of Microsoft Edge, Claude and Ubuntu. Replace the earlier
green/teal cast with graphite dark and neutral light surfaces; keep both desktop
and mobile verification in the final end-to-end pass.

Current pass (2026-10-26): public CLI 17-case VM gate and six explicit native media
tests pass on their recorded slices. Replicated-admission CLI SIGKILL recovery and
authenticated self-address movement now pass targeted integration tests. The
foreground light/dark UI is implemented for SDK room lifecycle, confirmed pairing,
messages/sensitive reveal and consented file download/forward; HTTP integration and
auth-boundary tests pass. Browser/media visual QA is IN PROGRESS. Finish remaining
media/platform/replica acceptance, deep audit and dual-remote publication; no final
release, portable-platform support or complete-product closure is claimed.

Latest evidence (2026-10-26): public CLI/file VM gate passes fourteen scenarios
(`public-room-cli-00150bf04863.json`) and all 66 baseline scenarios pass with the
same release hash. Explicit copy forwarding after original-VM loss works; automatic
replica discovery/availability remains open. Guests/network are stopped. Native
media is now in implementation: local source/codec/process APIs and consented
preview exist, targeted codec tests pass, private virtual-display capture is being
verified. Complete authenticated room media and the other open delivery layers;
do not stop at these native primitives or re-run old matrices instead of building.

## Current Delivery Position (2026-10-26)

- [DONE] Public room CLI: create/serve/invite/join/text/status/controllers/remove/stop.
- [DONE] Three independent VM public-CLI gate, including cold resume and actual
  creator VM off: nine scenarios, `public-room-cli-d14a1cb102f4.json`.
- [IN PROGRESS] Room file integration: explicit receiving consent and recipient,
  existing verified-prefix protocol, current room/file permissions per chunk and
  shared bounded inbox accounting. Host make check passes 133 Rust tests. The
  extended public-CLI VM gate and baseline regression are running.
- [OPEN] Interrupted enrollment recovery, authenticated address updates, explicit
  replica discovery/availability, native media, GUI, platform/distribution work.
- [OPEN] Dedicated post-integration adversarial review before final publication.

## Deep Review After Functional Completion (2026-10-26)

The operator explicitly requires a second, dedicated review of difficult boundaries
even when the implementation and ordinary tests appear correct. Perform this pass
after the complete user-facing workflows are integrated, before final publication;
do not let it displace delivery of the currently missing layers.

- Revisit crypto/SQLite atomicity: failed COMMIT, cancellation, process kill, lost
  state, partial enrollment and exact signed operation recovery without key reset.
- Revisit data membership versus controller/voter authority: stale permissions,
  revoked and nonvoting senders, joint transitions, creator loss and private keys
  never crossing device boundaries.
- Revisit scoped certificate/bootstrap trust and address changes: wrong purpose,
  copied public packages, replaced endpoints, old controller sets and verified rejoin.
- Revisit QUIC ownership and shutdown: every task/socket/lease owner, partial RPCs,
  drain timers, repeated same-port restart and cleanup failure diagnostics.
- Exercise boundary sizes, resource exhaustion, competing operations and partition
  schedules, including aggregate snapshot budgets and actual platform constraints.

For each critical property, record its invariant/owner, reproducible adversarial
test, actual evidence and residual limitation. Close confirmed blocking findings
and retain regressions. Absolute perfection is not provable; do not substitute
that claim for evidence or turn this review into an unbounded rerun loop.

## Full Completion Reaffirmed (2026-10-26)

The operator explicitly requires continued autonomous implementation through ALL
layers, including working multi-user rooms, real backend/media capabilities and
the final GUI. Do not stop at a crypto/public-control checkpoint or merely list
the remaining work. Repair broken tests and verify each implemented layer.
Finish final validation and documentation before add/commit/push/publication to
both remotes. Continue with the same principal agent; no delegation or global
policy changes. Record partial evidence honestly rather than declaring the
whole product finished from a narrower successful test.

## Current Autonomous Completion Order (2026-10-25)

The operator authorized completion and publication, without subagents. The older
gate notes below are historical; only the backend-before-UI gate remains active.

1. [DONE] Verify the 0.5 unified CLI, local sensitive handling and age envelopes in real VMs.
2. [DONE 0.6] Implement PAKE/QR invitation, transcript/key binding, contacts, grants and revocation.
3. Replace one-shot-only orchestration with bounded multi-peer sessions and typed events.
4. Implement authenticated membership, creator-independent lifecycle and replicated state.
5. Implement audio, camera/screen tracks, native sources/sinks, pacing and congestion behavior.
6. Test at least three guests: joins/leaves, creator loss, partitions, revocation and resource caps.
7. Implement the optional light/dark frontend only after backend acceptance evidence exists.
8. Finish portable signed artifacts, verified installers/offline bundle, CI and both remotes.

Current verified scope must be distinguished from this full target. No placeholder,
generated video pattern or two-peer test closes a native capture or mesh gate.

### 0.7 Node Foundation Update

Item 3's pre-paired persistent-node foundation is implemented and exercised with
three independent VMs. The versioned harness is now `lab/`; `docs/node.md` defines
its guarantees. Dynamic membership, automatic membership reconnect/backoff,
replicated history and media remain open under items 4-8, not implied by this gate.
The physical hardware follow-ups below also remain separate from VM evidence.

## Next Operator Gate

- Review the discovery and file evidence in docs/lab.md and docs/files.md.
- Approve the next layer explicitly; no media, UI or permanent service starts automatically.

## Physical Hardware Follow-up

- Select two physical test devices and an isolated LAN.
- Verify mutual pairing, message receipt, wrong-peer rejection and interruption.
- Observe attempted external traffic with Internet unavailable.
- Record hosts, OS versions, firewall settings and results.
- Obtain operator approval before implementing another layer.

Stages 1-3 are implemented with separate VM gates: messages, discovery, then files.
Completed work is recorded in CHANGELOG and the lab report, not repeated here.

## Later, Separately Gated

- Voice, followed by screen and video with measured contention behavior.
- Browser interoperability proof before a browser UI or additional transport commitment.
- Mobile capability matrix and native integrations where needed.
- Windows private-storage ACL support and verified multi-OS packaging.
- Security review and performance measurements before production distribution.
