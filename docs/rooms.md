# Room Identity And Membership Boundary

## Private Persistence Update

Private storage is now implemented through the official OpenMLS SQLite provider,
with a thread-confined Libcrux provider and bundled SQLite. Use
`CryptoMember::create_private` for a new local client, `open_private` for an
existing one, and `Group::restore` for existing membership. Missing signer,
checkpoint or matching crypto state is an error, never implicit regeneration.

Transactions cover MLS table writes and local checkpoint/package/replay metadata.
Ciphertext, received text and Welcome are not returned before successful commit.
Commit failure quarantines the handle; public status retains the last durable
view. Known rejected mutations reload after rollback instead of retaining dirty
in-memory ratchets. The database is private (0700 directory, 0600 regular
single-link file, exclusive lease), not encrypted at rest. Physical erasure,
endpoint compromise and full-disk protection remain separate concerns.

Tests exercise reopen, real process kill with an outstanding SQL transaction,
durable sender/receiver ratchets, prepared changes and actual failed SQLite
COMMIT. This persists crypto epochs, not a verified consensus/log position:
network ordering, global package consumption and room replication remain open.
Older implementation snapshots below are retained for traceability.

> Working 0.8 layer. Credential proofs exist; live room admission, ordered
> membership, persistence and replicated application state are not complete.

Update: `rooms/group.rs` now implements the in-memory MLS crypto state machine,
including group messages and membership-change validation. Its ordered-delivery
precondition is external, not a proof that the network runtime exists. See
`room-runtime.md` for the remaining public/private state and persistence boundary.

## A Credential Is Not Admission

```text
 explicitly approved room anchor
             |
 signed, bounded delegation proof
             |
 certificate-signed MLS key binding
             |
 valid RFC 9420 KeyPackage
             |
 CURRENT membership + permissions + ordered change
             |
       admitted participant
```

The steps above are distinct. The first four do not authorize the last one.
Introduced participants never become global contacts automatically.

## Keys And Proofs

| Item | Meaning |
| --- | --- |
| Room anchor | Exact approved creator certificate, title and random nonce; hashed room identity |
| Delegation | Exact room, parent, issuer, subject and authority ceiling; maximum eight links |
| Device key | Existing protected P-256 identity; never copied to another participant |
| MLS key | Distinct per-room Ed25519 signing key plus private package-init material |
| Binding | Device signature over room, subject fingerprint, grant ID and MLS public key |

A valid MLS signature authenticates its MLS key, not an arbitrary certificate
written into an opaque BasicCredential. The K&K binding and complete authority
proof must also verify. Changed subjects, keys, rooms or authority cannot be
silently substituted.

Current group-crypto dependency: OpenMLS 0.9.2 with its official Libcrux 0.4.0
provider and the RFC ciphersuite X25519/AES-128-GCM/SHA-256/Ed25519. Application
record signatures remain P-256/SHA-256 through Ring. No algorithm fork, custom
group cipher or insecure TLS verifier is used. This composition is not audited.

## Bounds

`config.rs` owns MLS frame, package-generation and lifetime limits. Raw bundles
are bounded before decoding. Packages expire, may not be last-resort packages,
and cannot expand the candidate's private init-key inventory without a bound.
Export includes only the public KeyPackage, never its private KeyPackageBundle.

The caller must still consume package references once in committed membership
state. Re-validating a public package is not evidence of one-time admission.

## Before Membership Effects

- Check the actor's current authenticated membership and effective permissions.
- Check a new admission's immediate issuer is currently allowed to invite/delegate.
  A historical ancestor may have left; its public signature is not a live service.
- Enforce local fingerprint revocation even when a room proof remains valid.
- Validate added, updated and update-path credentials before merging MLS commits.
- Bound member count, pending work, replay state and accepted proposal types.
- Withhold Welcome until the change has verified ordered-delivery acceptance.
- Reject unresolved concurrent/forked authority. A wall clock or silence does not
  establish a new authoritative membership view.

Creator departure must not copy its private key or promote everyone to admin.
An authorized remaining participant may continue with its own keys and previously
delegated rights. Revocation cannot be instantaneous on an uninformed partition;
freshness and fail-closed control policy require explicit implementation/evidence.

## Storage And Evidence

Private MLS state, lost-state behavior, atomic persistence, secret retirement,
restart recovery and ordered delivery remain implementation gates. Do not silently
regenerate missing state over an existing membership or archive retired ratchet
secrets as ordinary history. Physical erasure and endpoint compromise are not solved.

`tests/rooms.rs` covers offline authority. `rooms::mls::tests` covers real native
package signatures, identity substitution, lifetime/budget bounds and cross-room
replay. `make lab-crypto-test` executes those native tests separately on each guest;
it is explicitly not a cross-guest room data or admission test. Network room and
replication gates must still use real joins, departures, crashes and partitions.
