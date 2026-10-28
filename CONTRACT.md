# K&K Contract

This page defines current guarantees. Usage belongs in [CLI](docs/cli.md),
implementation ownership in [backend map](docs/backend-map.md), and measured
results in the [verification record](docs/progress.md).

## Trust Before Data

```text
address/discovery hint
         |
explicit invitation + two-sided confirmation
         |
certificate/private-key proof
         |
current membership + operation permission + local veto
         |
encrypted operation -> honest outcome report
```

| Boundary | Guarantee |
| --- | --- |
| Device identity | Owned private key and exact peer certificate; IP/name is not identity |
| Pairing | Expiring single-use OPAQUE invitation, bound context and two-sided consent |
| Transport | Strict pinned mutual TLS over QUIC; no permissive verifier or public-root fallback |
| Room admission | Current authority and certificate-bound MLS package; not automatic global contact trust |
| Data rights | Checked before effects and during long transfers |
| Votes | Explicit signed controller configuration; separate from application roles |
| Revocation | Local veto plus current room policy; not instantaneous knowledge across a partition |

## Three Different State Owners

```text
public replicated control     private device state       local presentation
membership / signed order     MLS keys / ratchets          CLI / browser
roles / controller config     replay / applied cursor     display / consent
          |                            |                         |
          +------ validated effects --+------ bounded events ---+
```

- Never replicate a private MLS database, device key or private signer.
- Commit private crypto and its checkpoint before exposing ciphertext, plaintext
  or Welcome. A failed/uncertain commit quarantines the handle.
- Reopen verified state after a crash. Missing keys or an unbridgeable retained
  history require explicit recovery/rejoin, never silent regeneration.
- Creator loss does not promote members or copy the creator's key. Data can remain
  peer-to-peer; fresh control authorization still needs a usable controller quorum.

## What Success Means

| Operation | Successful receipt means | Does not mean |
| --- | --- | --- |
| Direct/node text | Authenticated validation and bounded application acceptance | Durable chat history or human reading |
| Room text | Durable replay processing plus local event-queue acceptance | Human reading or atomic delivery to every peer |
| File/copy | Exact hash verified and destination durably published | Permanent availability or retroactive recall |
| Media datagram | Queued for the selected transport | Delivery, playback or synchronized A/V |
| Stop request | Shutdown accepted | Process/socket already released |

A timeout can leave a remote effect committed. Report uncertainty or partial
delivery; do not automatically replay a payload or invent an acknowledgement.
Process exit and socket release establish completed shutdown.

## Files, Sensitive Data And Media

| Concern | Rule |
| --- | --- |
| File selection | Explicit `-f`; remote names never choose a local destination |
| Receiving | Explicit consent, private inbox, byte/work budgets |
| Resume | Bind peer/content, verify retained prefix and final hash; preserve conflicting data |
| Publication | Atomic no-replace destination handling; no silent overwrite/reset |
| Copies | Explicit file and audience; offers expire with service/epoch; lookup is only a hint |
| Sensitive mode | Hidden local handling; plaintext argv refused; ordinary transport is already encrypted |
| Persistent encryption | Standard age recipient/passphrase envelopes; not a custom cipher |
| Capture/playback | Local source and recipient/kind consent; remote traffic cannot start capture |

Owned buffers are zeroized where possible. This is not forensic erasure, swap
protection, a guarantee about compromised endpoints or a recall of delivered bytes.

## Wire And Failure Rules

| Direct surface | Format |
| --- | --- |
| Text | `kk-text/2`: sensitivity byte, u32 length, bounded UTF-8, FIN |
| File | `kk-file/2`: sensitivity byte, u64 size, SHA-256; verified resume/receipt |
| TLS name | `kk.local`, a protocol name rather than DNS authority |

The package version is owned by `Cargo.toml`; it is not the wire version.
Room/control codecs and numeric resource budgets are owned by their typed code,
primarily `src/config.rs`, not by duplicated prose constants.

| Failure | Required outcome |
| --- | --- |
| Wrong key, scope, role or revoked identity | Refuse before unauthorized effects |
| Malformed/oversized input | Bound work and allocation; typed failure |
| Quorum unavailable or stale authorization | Fail closed; cached status is not a fresh quorum proof |
| Persistence failure | No false durable receipt; preserve last acknowledged public view |
| Unsupported OS/capability | Explicit refusal; no weakened filesystem/capture protection |
| Cancellation/slow peer | Bounded work, owned resources and honest uncertain outcomes |

The CLI/library do not share trust logic by copying it: the library owns the
invariant. Stable error names come from `Error::code` in `src/error.rs`.

## Limits Of The Claim

This is crash-fault consensus, not Byzantine safety or an independent security
audit. Local account compromise, malicious browser extensions, an authorized
compromised controller majority, physical radio behavior and unsupported native
platforms are not solved by these checks. See [platforms](docs/platforms.md) and
[security review](docs/security-review.md) for the precise evidence boundary.

## Profile Publication Boundary

See the [profile publication contract](docs/profile-publication.md) for per-write
consent fences, filesystem availability assumptions and stable-path authority.
