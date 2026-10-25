# Completion Progress

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
