# Backend Completion Map

> A feature is complete only with implementation, observable behavior and evidence.
> Frontend work is authorized only after the backend acceptance gates pass.

## Primitive Inventory

| Primitive | Existing foundation | Remaining work / proof |
| --- | --- | --- |
| Identity and encrypted transport | Pinned mutual TLS over QUIC | Device lifecycle, ergonomic trust and multi-peer admission |
| Message and URL bytes | Positional text/stdin, wire sensitive marker and protected output | Continuous inbox and peer aliases |
| File transfer | Consent, hashes, prefix resume, no clobber | Unified interface, permission integration, progress/cancellation events |
| Persistent encryption | Standard age envelopes, recipient/passphrase paths, no sender plaintext staging | Session/contact integration and additional platform evidence |
| Discovery | Unverified IPv4 mDNS | Pair/session capabilities, address changes and clear diagnostics |
| Pairing | Manual certificate comparison | Single-use invitation, PAKE, transcript/key binding, two-sided confirmation |
| Contacts and permissions | One selected certificate per operation | Private trust registry, grants, revocation and changed-key refusal |
| Sessions | One connection/operation | Multiple concurrent authenticated links and bounded events |
| Membership | Not implemented | Join/leave, leases, reconnect, authorized admission, partition behavior |
| Shared state | Not implemented | Replicated snapshots/events, bounded convergence and signed authority |
| Audio | Absent from both old and new project | Source/sink, codec negotiation, pacing, congestion and latency proof |
| Camera video | Absent from old project | Capture/encode/transport/decode, bounded frame queues and measured loss behavior |
| Screen sharing | Old separate JPEG/TCP prototype | Native capture consent, encoding once, track lifecycle and multi-peer distribution |
| Deployment | Local Linux binary | Verified portable artifacts, installation, provenance and offline bundle |
| Web adapter | Not implemented | Only after backend gates; no business or trust logic in UI |

The 0.5 foundation passes 45 local tests and 34 real two-guest scenarios. Pairing,
contacts, multi-peer sessions, mesh/state, media and the frontend are not complete.
Public remotes now exist at github.com/kpihx-labs/kk and gitlab.com/kpihx-labs/kk;
this is not yet a signed release or a claim of a finished product.

### Next Pairing Implementation Decision

Evaluate opaque-ke 4.0.1 (RFC 9807, Ristretto255/TripleDH) with Argon2id, not the
Identity KSF used by its quick examples. NCC audited an earlier implementation
in 2021; neither 4.0.1 nor our ceremony is thereby independently audited.

Generate a fresh high-entropy invitation and an in-memory registration record
locally. Never send the OPAQUE registration upload over the bootstrap channel.
One global attempt consumes the invitation; expiry and cancellation also invalidate
it. The public PAKE handshake is separate from ordinary data TLS. After mutual
key confirmation, exchange bounded certificate/permission data over keys derived
with role/transcript/invitation domain separation. Persist only after both sides
confirm. Ordinary payloads then use strict pinned TLS, never an insecure verifier.

Test wrong codes, malformed/replayed messages, expired invitations, concurrent
guesses, denied permissions, changed keys and revocation on actual guests. Test
invitations are synthetic and must not be printed into result logs. The peer alias
is operator-chosen, not a remotely supplied path or an identity proof.

The 2024 loc-stream source has working prototype paths for file and JPEG screen
transfer, but meeting buttons are stubs, departure only switches pages, participant
propagation is incomplete and admission has a dictionary bug. There is no audio,
camera, mesh or replicated memory implementation to preserve. No legacy source is
copied or relicensed.

## Unified Send Contract

```text
 send TARGET "literal text"          explicit text bytes
 send TARGET PATH --file             explicit file, never guessed from existence
 send TARGET -                       bounded stdin
 send TARGET --sensitive             hidden local input, no secret in argv
 send TARGET PATH -f --seal-to AGEKEY encrypted envelope, even after reception
```

The unified syntax and envelope paths are implemented in 0.5 with explicit IP and
certificate arguments. Named targets and trust defaults are not implemented yet.
There is no `--message`. Quoting does not make argv secret: sensitive inline text
must be rejected. Normal traffic is already encrypted; sensitive mode does not
pretend ordinary mode uses weaker transport cryptography.

Sensitive input/output policy and persistent encryption are independent. Secrets
must not be printed, logged, copied to a clipboard or stored as plaintext by
default. The recipient explicitly chooses reveal or private output. Owned buffers
are cleared where possible; compromised endpoints, OS swap and forensic erasure
are not solved by a CLI flag.

Use the age format rather than inventing a cipher or relying on weak ZIP defaults.
A retained ciphertext artifact is necessary for deterministic transfer resume:
fresh randomized encryption must not silently replace an earlier partial's input.

## Pairing Boundary

Use a maintained PAKE implementation with a documented review history. A past
library audit does not imply the current version or our composition is audited.
OPAQUE/RFC 9807 and opaque-ke are candidates; the short-code protocol must not
be improvised from a hash or a permissive TLS verifier.

| Required property | Evidence |
| --- | --- |
| Single-use, expiring invitation | Consumed/expired/replayed invitation tests |
| Global attempt budget | Racing peers cannot multiply guesses by changing IP |
| PAKE key confirmation | Wrong code and tampered messages fail before trust storage |
| Binding | Roles, invitation ID, protocol version, both identities and transcript |
| Two-sided consent | Neither unverified discovery nor a display name grants authority |
| Strict data channel | Certificates pinned and normal TLS restored before payload |
| Permission store | Incoming authorization checked at receiver, not only sender UI |

QR transports an invitation/public identity, never the permanent private key.
An invitation can be photographed; it expires and does not bypass confirmation.

## Mesh And State

```text
 A <------> B             Every node owns its identity and local resources.
  \        /              The creator is not a permanent traffic relay.
   \      /               No remote RAM pointers or imaginary shared memory.
    > C <                 State is explicitly replicated and versioned.
```

Use authenticated membership events and per-author monotonic ordering, not wall
clock timestamps as authorization. A partition is not proof of departure. Revocation
cannot be promised instantaneous on a peer that has received no update; freshness,
leases and partition policy must be explicit and tested.

Creator departure must not erase cached membership/state or stop remaining links.
New admission after departure requires an explicitly delegated admission right,
not an automatic promotion of every member to administrator.

Files and secrets are not automatically replicated as shared state. Continued
availability after a source leaves requires explicit replication permission and
verified replicas. Secret data defaults to no replication or durable history.

## Acceptance Matrix

At least three independent guests for mesh tests; cap RAM/vCPU and stream rates.
Verify joins, clean/unclean departure, creator loss, partition/heal, duplicate and
reordered events, wrong/revoked identity, denied operations and resource exhaustion.

For media distinguish synthetic codec/transport tests, actual virtual display
capture and physical device tests. Record bitrate, frame/packet loss, latency,
CPU/RSS and queue bounds. A generated test pattern is not camera/Wayland support.

Publish only capabilities actually verified. Signed provenance is not a security
audit. UI, releases and installation have their own gates; no global agent-policy
changes or subagents are authorized.
