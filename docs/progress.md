# Completion Progress

## Verified Node Checkpoint: 0.7.0 (2026-10-25)

- Final 0.7.0 binary and migrated canonical `lab/` pass all 66 scenarios after a
  cold start of the three independent guests. Rust checks pass 61 tests; lab static
  validation passes six mocked control tests, Ruff and ShellCheck.
- Raw evidence: `lab/tmp/kk-results-571c21e64207-1792941968896578628.json`.
  A compact public summary is versioned in `lab/evidence/0.7.0.json`.
- The earlier development cold run also passed 66 scenarios (`780a4755da65`);
  all old records remain in `../vm-lab/tmp/`, not copied or deleted.
- `make lab-test` now owns the suite. The older vm-lab Makefile forwards to it;
  media assets and original disks remain outside the source repository.
- Publication/CI monitoring follows this verification. Later sections are retained
  historical snapshots, including their then-current in-progress wording.
- Next: dynamic rooms and replicated state, then native media. No UI or complete
  product/distribution gate is closed by the persistent-node evidence.

## Cold Regression Update (2026-10-25)

- The 0.7 development binary passes all 65 VM scenarios after a cold restart of
  the three guests. `make check` passes 61 Rust tests, Clippy and rustdoc.
- Evidence: `vm-lab/tmp/kk-results-abeabe2f3319-1792940363567737238.json`.
  Installed binary SHA-256: `76011a4a5bc95b4164aceaf1fdcc9e1645ee487cf9fb1be2b6ea94fd86edfb68`.
- In this run, local status took 0.065 seconds including lab-control overhead while
  two file commands were active on the 4 Mbit/s, 50 ms shaped link. This is one
  measured fixture result, not a universal latency or throughput guarantee.
- Packet capture on the first guest contained no application-port traffic outside
  the three declared guest addresses. Every guest had no default Internet route.
- Additional active-file revocation/resume coverage is being checked. This remains
  an uncommitted development tree; the published checkpoint below is still 0.6.

## Working Tree: 0.7.0-dev.1 (2026-10-25)

- Message and file stream handlers are shared by one-shot and persistent lifetimes.
- Foreground `serve`, private same-UID Unix IPC, `status`, exact admitted TLS roots,
  reused authenticated links, live revocation and bounded event/operation queues.
- Long IPC work uses separate text/file lanes. Bulk saturation cannot occupy all
  status readers; excess work is refused, not queued without a bound.
- Uncertain operations retire their cached link. A later explicit operation can
  reconnect; the core does not automatically replay text or files.
- Three independent guests: Fedora 44, Mint 22.3 and a fresh Fedora Cloud peer.
  Private keys were generated separately and never copied between guests.
- Focused run `dbab1da297e8` passed 12 persistent-node scenarios: every directed
  text edge, file ring/hash checks, escaped/oversize sensitive input, two simultaneous
  throttled files with usable text/status, IPC ownership, revoke/restore, partition,
  first-node crash, same-identity restart and socket release.
- Evidence: `vm-lab/tmp/kk-results-dbab1da297e8-1792940065337103271.json`.
- Full regression after cold restart is running. This development tree is not yet
  a published checkpoint. Dynamic rooms, replicated state, media and UI remain open.

## Current Verified Checkpoint: 0.6.0 (2026-10-25)

- Network OPAQUE ceremony, encrypted certificate/grant metadata, two-sided approval,
  temporary strict mutual TLS with exporter binding, then private contact persistence.
- Private invitation/QR image roundtrip, default profiles, aliases, grants and
  fingerprint revocation enforced by library handlers, including active file streams.
- No silent key replacement/regeneration over trust data; CAS rejects stale pairing
  confirmation after a concurrent local permission change.
- 56 local tests pass; all 53 scenarios pass in cold VM run `2a684d63f667`.
- Evidence: `vm-lab/tmp/kk-results-2a684d63f667-1792933990374310154.json`.
- First development pass: 52 scenarios, run `cfb785cc0758`.
- Next implementation gate is persistent multi-peer sessions, followed by mesh/state
  and measured media. Read docs/sessions.md. No frontend or complete-product release yet.
- Historical snapshots below are retained, not current claims of missing 0.6 features.

Publication note: commit `e09bc779fb9b274c52328e2ecde9cc30f4e81ec1` reached both
remotes. The publication-time network timeout on SSH port 22 was resolved with the hosts'
official SSH port-443 endpoints, preserving strict known-host verification and
the existing host-key aliases. The Makefile exposes scoped per-remote SSH command
overrides; no global SSH or agent policy was edited.

## Verified Checkpoint: 0.5.0

- Principal agent only since the operator's no-subagent instruction.
- Unified positional send/receive, explicit file mode, stdin and protected local output.
- Standard age recipient/passphrase envelopes and explicit decryption.
- 45 local tests, 34 real two-guest scenarios, both foundation CIs successful.
- Commit `7296dfd208d90c47a65c985792b11f6467e1cae8` pushed to both SSH remotes.
- GitHub: https://github.com/kpihx-labs/kk
- GitLab: https://gitlab.com/kpihx-labs/kk
- Foundation CI is not a signed release or proof of complete backend functionality.

## Historical Working Tree: 0.6.0-dev.1

The PAKE and contact foundation is in progress and not yet published as complete.

- `src/pairing/pake.rs`: opaque-ke 4.0.1, Argon2id 19456 KiB/t2/p1,
  Ristretto255/TripleDH, six random dictionary words (66 bits), local registration,
  single consumed attempt, expiry/context tests, directional HKDF/AEAD metadata channel.
- `src/contacts.rs`: private local registry, incoming/outgoing grants, changed-key
  refusal, revocation and archived prior states. Not yet connected to networking/CLI.
- `make check` passes 49 local tests. These are not network pairing evidence.
- Real PTY regression run `fdc15a255887` passed all 36 VM scenarios. The two new
  cases force terminal echo initially on, check hidden input and exact terminal
  restoration, and verify Ctrl-C cancellation/restoration in an actual guest PTY.
- Record: `vm-lab/tmp/kk-results-fdc15a255887-1792927278765265426.json`.

## Historical Next Work At The Foundation Snapshot

1. Finish the bounded network pairing ceremony around the tested PAKE state.
2. Exchange certificates and grants only through transcript-bound encrypted metadata.
3. Prove both certificate private keys using temporary pinned mutual TLS before persistence.
4. Require both sides' confirmation/persistence receipts; do not promise atomic remote
   storage across a partition. Preserve partial local completion and make retries safe.
5. Expose pair/trust/default-profile/alias CLI and enforce receiver-side grants/revocation.
6. Add real VM tests for wrong/replayed/expired/racing invitations, changed keys and denied actions.

The remaining session/mesh/media/frontend/distribution phases are still required
and listed in TODO.md and backend-map.md. Do not close them on foundation tests.

## Operating Notes

- No global .agents policy edits. AI text detectors are opt-in for this project.
- Do not delegate, spawn help peers or invoke subagents.
- VM tests: `make -C ../vm-lab kk-test`; stop: `make -C ../vm-lab kk-stop`.
- Root app `make check` does not need elevation and does not boot guests.
- The normal host NAT/firewall configuration stays untouched.
- Do not publish a stable/full-product release or start UI before backend gates pass.
