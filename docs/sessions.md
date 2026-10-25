# Persistent Sessions And Mesh: Next Acceptance Contract

> Planned next layer, not a claim of implemented functionality.
> No frontend or media gate is closed by this document.

## Boundaries

```text
 CLI / future web adapter
          |
 typed commands + bounded events
          |
 persistent node and contact authorization
          |
 authenticated links / room membership / replicated state
          |
 shared message/file handlers + later media tracks
          |
 the existing pinned mutual-TLS transport owner
```

The node must not copy the message/file protocol implementations or TLS setup.
Extract stream handlers from one-shot lifetime orchestration and reuse them.
Use an explicit interactive transport policy for connection/stream/datagram limits;
legacy one-shot operations keep their own restrictive policy.

## Initial Node Gate

| Property | Required evidence |
| --- | --- |
| Multiple links | Three independent guest nodes, not three processes on one OS |
| Identity | Only exact admitted certificates; no system CA or insecure verifier |
| Authorization | Receiver admission/effects plus active-stream revocation |
| Commands/events | Typed and bounded; no arbitrary remote execution |
| Messages/files | Several operations without starting a new receiver each time |
| Congestion | Control is not starved by bulk files; queues/resources bounded |
| Reconnect | Backoff, duplicate-link handling and authenticated address hints |
| Cancellation | Pending operations fail honestly; no invented delivery success |

Discovery is an address hint, never an admission grant. A cached last address may
be stale. Reassigned IPs must fail key verification rather than replace contacts.
Do not retry sensitive operations automatically across an uncertain outcome.

## Rooms Without A Permanent Host

```text
        A -------- B
         \        /
          \      /
             C

 creator A leaves -> B and C keep their authenticated link and authorized state
```

There is no shared RAM address space over the LAN. Explicitly replicate bounded
state and signed/versioned events. Each producer owns its tracks and authored
events. Wall clocks are not authorization or conflict-order authority.

Creator departure, explicit member removal and key revocation are different
events. Silence may be a partition, not proof of departure. Revocation cannot be
promised instantaneous on a peer that has received no update; freshness and
partition policy must be stated.

Three pre-paired contacts are a useful initial mesh test, but do not close dynamic
room admission. That gate additionally requires verifiable, explicitly delegated
membership credentials and no automatic global trust of introduced participants.
After the creator leaves, admission continues only if a remaining participant
has the relevant delegated right. Remaining peers retain verified public state;
they do not inherit the creator's private key or arbitrary administrator powers.

Membership provenance and forwarded author events require authenticated signing
bindings or an equivalent reviewed mechanism. Do not simply trust a forwarded
claim that another member authorized something.

## Data, History And Replication

- Bound seen-operation caches, event history, snapshots and per-peer work.
- Define duplicate/uncertain delivery behavior across reconnect/restart explicitly.
- Sensitive text defaults to no durable history or replication beyond chosen recipients.
- Files are not silently replicated to every member. Continued availability after
  a source leaves needs explicit replica consent and verified content.
- A file's completion is not proof that an unrelated peer has cached it.
- No local display name or remotely supplied filename chooses an authority/path.

## Acceptance Matrix

Join, clean leave, process kill, creator loss, reconnect, partition/heal, duplicate
and reordered events, wrong/revoked keys, denied operations, simultaneous streams
and explicit resource exhaustion. Record all guest identities, topology, limits,
profiles and software hashes. Keep control over virtio, outside impaired links.

Media source/codec/transport/sink gates follow this session layer. A generated
pattern is not a camera or screen capture test. Actual virtual display capture,
physical capture and native platform permission dialogs are separate evidence.
