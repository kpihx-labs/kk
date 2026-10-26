# Backend Completion Map

> A feature is complete only with implementation, observable behavior and evidence.
> Frontend work is authorized only after the backend acceptance gates pass.

## Current 0.8 Software Map

The historical tables below predate the integrated delivery. Public room CLI,
confirmed enrollment, separate voters/data rights, durable crash recovery, signed
address movement and explicit copy availability are implemented and exercised by
the public cross-VM gate. The native private-display shaped-link gate now also
proves actual decode during verified bulk transfer and acknowledged text. The
bundled grey UI uses the same owners and has HTTP/browser evidence.

The dedicated second review is recorded in [security-review.md](security-review.md),
including reproduced/fixed cache and lifetime counterexamples. Actual binary hashes
and result boundaries remain in [progress.md](progress.md). Artifact/provenance CI
and installer behavior are governed by [distribution.md](distribution.md), and
[platforms.md](platforms.md) explicitly declines unsupported native Windows/macOS
claims. Neither a successful build nor synthetic media is physical-device proof.

## Public Adapter Update (2026-10-26)

`rooms::local` now owns explicit profile-scoped creation/resume and foreground
lifecycle. The CLI is a thin adapter for create/serve/invite/join/broadcast/status,
controller selection, data removal and stop. Private IPC reuses the node's single
same-UID/frame/lock implementation, with separate bounded command/invitation lanes.
An actor publication captures ciphertext and its exact epoch audience together;
the adapter does not infer audience from a later cached status.

Two actual CLI-process integration tests pass on the host, including three-way
enrollment/broadcast and same-port cold resume. The dedicated public CLI VM gate
is in progress, not yet claimed as evidence. Earlier source-inventory rows below
remain historical. Interrupted enrollment recovery, authenticated address changes,
room files/replicas, native media, GUI, portable distribution and the requested
post-integration adversarial audit remain open.

## Current Source Audit (2026-10-26)

The core contract, README, changelog, TODO, all ten `docs/` files and lab README
have been reread in full. That is a document review, NOT a completed source,
security or whole-product audit. The checkpoint inventory below is historical.

| Layer | Implemented source / evidence | Still required |
| --- | --- | --- |
| Paired identities, text/files and profiles | Published 0.6/0.7 foundation, private IPC and 66 three-guest node scenarios | Room/session integration, native Windows/macOS storage and physical devices |
| Room authority and private MLS | Signed bounded delegation, durable official SDK state, applied cursor and nonce/replay recovery | Scoped invitation and complete lifecycle integration |
| Public room control | Signed Raft order, private-owned SQLite log, snapshots, exact receipts; seven real cross-VM scenarios pass | Authorized voter/learner changes, application-level reconnect/address lifecycle |
| Private room runtime | Thread-confined actor, quorum freshness, profile veto, direct TLS/MLS sender binding, bounded queues | Complete room CLI/IPC and private guest-to-guest admission/data/fault scenarios |
| Welcome availability | Ciphertext committed atomically with admission; bounded replicas and recipient-authenticated retrieval | Verify creator-VM loss during actual delivery and explicit rejoin paths |
| Room status/cancellation | Cached actor status is typed; work/deadlines are bounded | Prove status/cancel responsiveness while quorum work occupies the private actor |
| File availability after source loss | Consented resumable point-to-point files exist | Explicit replica consent, replica verification and room-aware availability |
| Native media | Not implemented; image parsing is not capture | Real audio/camera/screen sources and sinks, codecs, negotiation, pacing and measured contention |
| GUI | Not implemented | Required light/dark thin adapter after backend gates; no UI-owned trust rules |
| Distribution | Source checkpoints on both remotes | Tested platform storage/builds, signed artifacts/provenance, offline kit, verified installers and final CI |

Detailed test runs, code versions and failed evidence remain in `progress.md`.
Successful per-OS crypto execution and loopback private RPCs are not a private
cross-VM room gate. Preapproved fixture bootstrap is not scoped OPAQUE admission.
Old permission/gate wording remains below as provenance, not a new approval gate.

## Primitive Inventory

| Primitive | Existing foundation | Remaining work / proof |
| --- | --- | --- |
| Identity and encrypted transport | Pinned mutual TLS, default profiles and key proof | Multi-peer admission and further platform evidence |
| Message and URL bytes | Positional text/stdin, sensitive policy, named contacts | Continuous inbox and typed session events |
| File transfer | Consent, resume, no clobber, incoming/outgoing grants and revocation | Persistent-session integration and typed progress events |
| Persistent encryption | Standard age envelopes, recipient/passphrase paths, no sender plaintext staging | Session/contact integration and additional platform evidence |
| Discovery | Unverified IPv4 message/file/pair mDNS | Session capabilities and authenticated address refresh |
| Pairing | OPAQUE, expiring one-attempt code, private QR import/export and strict TLS key proof | Native camera QR capture and further review |
| Contacts and permissions | Private registry, named peers, grants, revocation, changed-key/CAS refusal | Live remote grant synchronization and session integration |
| Sessions | One connection/operation | Multiple concurrent authenticated links and bounded events |
| Membership | Not implemented | Join/leave, leases, reconnect, authorized admission, partition behavior |
| Shared state | Not implemented | Replicated snapshots/events, bounded convergence and signed authority |
| Audio | Absent from both old and new project | Source/sink, codec negotiation, pacing, congestion and latency proof |
| Camera video | Absent from old project | Capture/encode/transport/decode, bounded frame queues and measured loss behavior |
| Screen sharing | Old separate JPEG/TCP prototype | Native capture consent, encoding once, track lifecycle and multi-peer distribution |
| Deployment | Local Linux binary | Verified portable artifacts, installation, provenance and offline bundle |
| Web adapter | Not implemented | Only after backend gates; no business or trust logic in UI |

The 0.5 foundation passed 45 local tests and 34 real two-guest scenarios. The 0.6
checkpoint passes 56 local tests and a 53-scenario cold replay. Multi-peer sessions,
mesh/state, media and the frontend are not complete.
Public remotes now exist at github.com/kpihx-labs/kk and gitlab.com/kpihx-labs/kk;
this is not yet a signed release or a claim of a finished product.

### Pairing Implementation Decision

The 0.6 implementation uses opaque-ke 4.0.1 (RFC 9807, Ristretto255/TripleDH) with Argon2id, not the
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

The unified syntax and envelope paths are implemented in 0.5. The 0.6 checkpoint
adds named targets and default private trust profiles; explicit certificates
remain an expert path, subject to the selected profile's restrictions.
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
