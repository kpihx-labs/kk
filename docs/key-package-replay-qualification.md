# Committed Key-Package Replay Blocker

Observed during the 2026-10-30/31 GUI qualification. Initially an unresolved
control-state convergence issue, not a reason to grant extra reader permissions,
reset identities, erase pending enrollment or claim the product complete.

## Retained Evidence

| Observation | Actual result |
| --- | --- |
| Existing room | A/B passed the earlier exact f817 GUI qualification; A then issued a reader invitation for a third independent fixture |
| C | `/tmp/kk-current-c-20261030`, pane `%163`, HTTP `45686`, intended UDP `45586`; image SHA-256 `3494dc01c39682e011b163652832b3998bb4bdf970cca6619a268fa304eb700b` |
| Identity/authentication | C initialized once. Matching bilateral confirmation and both expected public identities checked before approval; no invitation or key printed |
| Issuer result | A shows three data members, epoch two; its reader invitation operation completed |
| Recipient result | C's join reports `REVOKED` (`peer identity is revoked`) and no running room. Explicit resume of the saved room fails the same way; no new invitation, key regeneration or state deletion is used as recovery |
| Public applied state | Read-only SQL metadata: A applied index nine, epoch two, three members, two Welcomes; C applied index nine, epoch zero, one member, no Welcomes |
| Protected log comparison | All ten retained public Raft entries are byte-identical between A and C; origin, room and stored membership metadata compare equal. A has two accepted-operation receipts, C zero |
| Fresh reproduction | New real three-process CLI case, including participant enrollment and controller add/retire before reader enrollment, passes in 5.60 seconds. It does not reproduce long-delayed replay and is not accepted as a fix |

The read-only checks touch public-control metadata only, never private MLS/TLS
keys or decoded Welcome material. All profile/control/private state remains
intact. A/B remain admitted; do not silently remove C's already committed grant.

## Time-Dependent Replay Hazard

`StateMachine::apply` calls `apply_proposal`, then `View::apply_accepted`, whose
transition calls `mls::validate_package`. That calls OpenMLS 0.9.2
`KeyPackageIn::validate`, including `life_time.validate()` against the current
clock. K&K currently creates one-hour key packages with a bounded clock margin.
The first participant's committed package is old when C replays its admission.

Using current-time package validity while replaying an accepted log can reject
a previously accepted admission, leave the public view at epoch zero and still
advance its applied Raft index. The observed divergence is established; a
controlled expiry-specific regression is still required before attributing every
part of C's failure to this path or claiming a causal repair.

`ControlNode::receive_welcome` currently routes the remote artifact, performs a
fresh local barrier, then requires the exact protected local committed bridge.
Its `(Some(remote), None)` branch returns `REVOKED`. Do not weaken that matching
check, trust remote hints as private-join authority or repair/regenerate a Welcome.

## Required Correction Boundary

- Fresh submissions must still reject expired/not-yet-valid packages, malformed
  signatures, unsupported suite/extensions, invalid credential/grant bindings,
  cross-room/epoch proposals and consumed packages.
- Committed replay must be deterministic and retain every cryptographic check
  without depending on the replaying node's current wall clock.
- Existing accepted records, exact retry receipts, snapshots, local applied
  journals and private join authority must remain compatible and distinguishable.
- A correction may need an explicit-time OpenMLS validation API and a distinct
  fresh-admission/replay boundary or a replay-stable acceptance-time envelope.
  This is a security/consensus design change; no unchecked conversion, TTL
  increase, system-clock change or permission relaxation has been introduced.
- Already-divergent C needs explicit validated public-state recovery after the
  invariant is fixed, preserving its identity, candidate state and cached proof.

Regression source: `tests/room_cli.rs` /
`a_third_reader_receives_its_local_welcome_bridge_after_controller_reconfiguration`.
This currently demonstrates only the fresh path and passes before any correction.

## Verified Correction And Recovery

| Gate | Actual evidence |
| --- | --- |
| Expiry reproduction | `rooms::mls::tests::committed_control_replay_keeps_acceptance_after_package_expiry` accepts the proposal while fresh, waits past its short fixture lifetime, then reproduces the old `Authentication` replay failure. No system-clock change or extended package lifetime |
| Corrected validation | Same test passes: expired fresh validation/preflight refused, committed replay retained; exact accepted retry remains valid while modified proposal/package signatures fail |
| Invariant owner | Guarded leader submission holds the existing write gate, obtains an applied barrier and performs fresh preflight. State-machine replay uses the original signed package's not-before instant, with all cryptographic, scope, grant, replay, suite and lifetime-span checks retained |
| SDK dependency | Direct path OpenMLS 0.9.2 override exposes explicit committed-replay verification; normal `validate` remains current-time. No unchecked-conversions feature or primitive changes; exact upstream revision recorded in `third_party/overrides/sources.json` |
| Historical Welcome tree | OpenMLS also rejects old active members' tree leaf lifetimes by default. Only `Group::join_committed`, with local record room/epoch/recipient binding and the existing full bridge checks, uses the standard historical-tree builder; uncommitted joins retain the default validation |
| Recovery refusal/preservation | `public_recovery_requires_expected_proof_and_preserves_logs_vote_and_prior_state` passes: wrong expected hash leaves state untouched; correct recovery retains logs/vote and backs up original state/journal |
| Actual C recovery | Explicit `room recover-control utility --expected <A's independently obtained applied-record hash> --confirm` rebuilt the complete local committed prefix. C changed from index nine/epoch zero to index nine/epoch two, three members/two Welcomes; two original-state backup records retained |
| Actual private resume | Corrected image `f94ac1bd2fb8e604c7f145a0fa67d425158e0309d701e0f1db560f786c879e96` resumed the existing candidate and cached proof: same C identity, active, three members/epoch two, publishing/admin false; no new invitation or key generation |
| Reader enforcement | C retained an editable local draft; GUI send remained disabled, forged send request was refused, no phantom outgoing appeared. A's real SDK broadcast acknowledged both B/C and the reader GUI received/rendered it |
| Explicit removal | A's actual removal confirmation targeted C only: remaining A/B count two, epoch three. C's fresh veto then marked it inactive, refused publication and recorded no phantom outgoing; identity/profile state retained |
| Source checks | 345 library cases pass with 14 explicit ignores and the mandatory HTTP case reserved; that real HTTP/QUIC workflow separately passes in 11.23 seconds. Control subtree: 74 pass/one native-media ignore. Fresh three-process reader/controller regression passes in 5.59 seconds |
| Notices | Read-only canonical inventory still validates 492 dependencies/848 notice files with none missing |

Recovery is explicit, pending-enrollment-only and refuses active leases, existing
private membership, incomplete/purged/oversized prefixes, mismatched expected
record proof or concurrent state changes. It rebuilds public state in isolation,
then atomically backs up/replaces only machine and applied-journal metadata. It
does not rewrite logs, votes, keys, room definitions or private MLS state, and
does not turn a remote hint into private join authority. Normal resume still
requires exact local committed Welcome matching against the current authority.

The two old GUI processes remain pinned to their earlier images until the next
integrated candidate rollout. Passing this invariant's gates is not complete
product/platform/release acceptance. Independent core review is in progress.
