# Room Runtime: Remaining Boundary

Current 0.8 update: the runtime, public CLI, scoped enrollment/recovery, voter and
address lifecycle, explicit copies, native/browser media and GUI are integrated
on the qualified Linux software slice. Public cross-VM and native shaped-link
results, with exact hashes and residual limits, are in [progress.md](progress.md).
The post-integration [review](security-review.md) adds actual counterexamples and
regressions. The design acceptance text below remains historical provenance;
portable release status follows [platforms.md](platforms.md) and
[distribution.md](distribution.md), never this old heading alone.

Implementation update (2026-10-26): the public OpenRaft service now passes a real
three-VM recovery gate. Protected local committed-record catch-up and an atomic
private MLS applied cursor are implemented and locally tested. Retention is
bounded to 64 records / 8 MiB; a missing bridge or public-only snapshot requires
verified rejoin, never private-state regeneration. Historical receipts are not
applied-record audience proofs. The older design notes below remain provenance;
the integrated private network runtime and delivery/replication gates are open.

> Design/acceptance boundary, not an implemented network room service.
> The tested Group object is a crypto state machine with an external order precondition.

## Keep Three Kinds Of State Separate

```text
 replicated/public control       private per-device crypto       local adapter
 -------------------------      --------------------------      -------------
 ordered record IDs/terms        MLS epoch/init/ratchet keys       CLI / later UI
 authenticated membership        own MLS signature private key    bounded events
 permissions/track metadata      local replay/recovery state      explicit consent
 explicit replica availability   protected durable storage        no trust rules
```

Never replicate an MLS private database or serialize private keys into a control
snapshot. There is no distributed RAM pointer. Public membership convergence and
each device's private crypto recovery are separate responsibilities.

## Ordering Is Not A Counter Argument

`Group::confirm_ordered` and `apply_ordered` check exact pending bytes/epochs. They
do not prove that remote peers agreed on the order. The runtime must supply that
proof and must not turn a timeout or an arbitrary peer ACK into commit authority.
Conflicting preparation blocks instead of silently selecting a winner.

An ordered-delivery implementation needs explicit crash/partition assumptions,
bounded logs/snapshots, replay behavior and creator-independent recovery. A lease
or a wall clock is not proof of departure. Membership changes must fail closed
when authoritative order cannot be established. Do not advertise crash-tolerant
consensus as Byzantine consensus.

OpenRaft stable 0.9.25 was inspected as a candidate, not added or implemented.
Do not select its default alpha release by accident or add a second framework
without a concrete ordering/storage need. Data remains directly peer-to-peer;
an elected metadata coordinator is not an obligatory external server or relay.

## Authorship And Revocation

- Check the original MLS author, not just the transport relay's certificate.
- A valid old authority chain is a ceiling, not current membership or permission.
- New admissions require a currently authorized issuer and committer.
- Profile fingerprint revocation is an additional local veto; room introduction
  must not create global contact trust.
- Group broadcast encrypts for the current epoch's entire audience. Merely
  skipping a revoked recipient's direct socket does not exclude it from a group
  key. Pause new broadcast until an appropriate removal/new epoch is committed.
- A partitioned peer cannot learn a revocation instantly. Define bounded view
  freshness and outbound-data policy; do not claim instantaneous global revocation.

Existing admitted delegates do not inherit the creator's private key. Historical
proofs remain public verifiable data; only current membership grants live effects.

## Durable Crypto Before Exposure

Sending must not reuse an MLS generation/nonce after a crash. Receiving must not
forget consumed replay state after restart. Durable storage must cover crypto and
the local applied-control position before exposing the corresponding effect.

Use private owned directories/files, atomic transactions and synchronization.
Missing state over an existing membership must require verified recovery/rejoin,
not silent regeneration. Retirement of obsolete ratchet secrets must not create
ordinary archival copies. Physical forensic erasure remains outside this guarantee.

The official OpenMLS SQLite storage 0.3.0 was inspected as a candidate. Private
storage needs real restart/power-failure/error tests, not merely serialization.
An operation rejected by application policy must not accidentally become a public
membership effect or a false success receipt.

## Catch-up And Data

An existing member with its own private state can process retained committed
changes. A new member needs its own KeyPackage/Welcome, not another member's key
store. A member that cannot bridge a compacted crypto history needs explicit
authorized rejoin. A control snapshot alone is not an MLS private-state snapshot.

Sensitive text has no default durable history. Files are not silently replicated;
availability after a source leaves requires explicit consent and verified content.

Acceptance still needs actual cross-guest room joins, departures, creator crash,
concurrent admissions, partitions/heal, stale views, bad authors, revocation,
restarts, replay and resource exhaustion. Per-OS crypto execution is separate evidence.
