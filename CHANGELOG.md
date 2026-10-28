# Changelog

This records changes, not live delivery status. See [TODO.md](TODO.md) for the
current state and Git history for detailed implementation checkpoints.

## 0.9.0-dev.1 - Unreleased Development Checkpoint

- Add explicit local profile catalogs, locked settings updates, CLI management
  and public-only offline/live status without identity regeneration.
- Resolve local room nicknames by exact member identity, with explicit provenance;
  local aliases are not authenticated remote profile announcements.
- Make file consent revocable while online, preserving lifetime budgets and
  preventing publication after withdrawal, cancellation or owner shutdown.
- Reserve control IPC admission and bound stalled control handshakes.
- Add quantified profile themes, primary profile names, editable invitation/join
  workflows and independent keyed media controls with concurrency regressions.
- Add deterministic ZIP archives and bounded immutable-snapshot verification.
- Add nonpublishing native foundation jobs; these are not product qualification.
- Add default-off, authenticated room profile-name metadata with bounded refresh,
  explicit provenance and disclosure fencing against cooperating opt-out writers.
- Reject stale Settings saves through lossless revisions; retain editable drafts
  and reload consent explicitly before retrying a conflict.
- Add retractable sidebars, bounded panels, transient overlay notices and a compact
  action center; preserve message space rather than accumulating permanent cards.
- Derive concurrent media capacity from room membership and source kinds; add
  screen/window/tab hints, actual-surface reporting and explicit device choices.
- Permit safe pre-enrollment reservation retry without replacing partial identity
  or room state; distinguish human invitation waiting from ordinary RPC deadlines.

## 0.8.2 - 2026-10-27

- Replace accumulated work diaries with concise, schematic current guides.
- Align command examples, wire formats, feature claims and platform limits.
- Add documentation checks for links, tables, source-owned wire names and examples.
- Use `main` as the operator-selected primary branch; retain existing tags.
- Publish corrected-source artifacts after exact-byte qualification, instead of
  replacing the signed 0.8.1 draft; install the authenticated executable.

## 0.8.1 - Superseded Draft, Not Published

- Remove the four crypto advisory routes and two maintenance warnings through
  explicit, licensed [dependency overrides](docs/dependency-overrides.md).
- Preserve the Ristretto wire suite and byte-identical RustCrypto expansion
  files; add hash guards, upstream vectors and independent OPAQUE wire checks.
- Include local source dependencies in the complete notice inventory.
- Validate the conditional Hax macro binding with its upstream nightly.
- Add strict audit/SDK release gates and signed drafts: verify/replay the final
  binary before promoting the same assets to a public release.
- Keep per-version user installation immutable and retain previous binaries.

## 0.8 - Integrated Software Checkpoints

- Public room CLI, scoped enrollment/recovery and creator-independent operation.
- Explicit controller changes separate from data membership; signed address moves.
- Durable MLS/SQLite state, ordered public control and crash/replay recovery.
- Consented room files and audience/epoch-bound copy offers, lookup and retrieval.
- Native Opus/VP8 processes, room media authorization and browser media adapter.
- Private loopback UI with neutral light/dark themes and bounded workflow feedback.
- Fix late driver ownership, failed-recovery public cache, interrupted media-frame
  shutdown, socket-backed stdio and bounded TAR/PAX verification.

## Earlier Source Checkpoints

| Version | Date | Change |
| --- | --- | --- |
| 0.7.0 | 2026-10-25 | Persistent pre-paired nodes, private IPC, reused links and bounded lanes |
| 0.6.0 | 2026-10-25 | OPAQUE pairing, QR bundles, profiles, contact rights and revocation |
| 0.5.0 | 2026-10-25 | Positional send/receive, sensitive handling and age envelopes |
| 0.4.0 | 2026-10-24 | Consented streaming files, verified resume and no-replace publication |
| 0.3.0 | 2026-10-24 | Optional discovery as untrusted metadata |
| 0.2.0 | 2026-10-24 | Pinned mutual-TLS text and real guest-to-guest verification |
| 0.1.0 | 2026-10-24 | Rust library/CLI scaffold, architecture contract and original-work license |

Source checkpoints are not evidence of corresponding public releases. Earlier
running notes and per-attempt results remain in Git history and retained lab
artifacts; they do not override the current contract or usage guides.
