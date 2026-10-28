# Encrypted History Store Foundation

This is an SDK storage foundation, not live room capture or a history UI.
The authority is `src/rooms/store/history.rs`; numeric hard bounds live there.
The standard AGE record codec is described in [history-records](history-records.md).

## SDK Boundary

All operations are methods on `rooms::store::Provider`. Every operation requires
`&Transaction` from that same Provider. Start it with `Provider::transaction()`.
The existing connection, file lease, MLS storage, checkpoint and commit quarantine
remain the only durability owner. No secondary database connection is created.

| Method | Remaining arguments | Result |
| --- | --- | --- |
| `history_enable` | `&Binding, &Identity, Policy, now` | `Result<()>` |
| `history_status` | `Option<&Binding>, Option<&Identity>, now` | `Result<Status>` |
| `history_disable` | `&Binding, &Identity, now` | `Result<()>` |
| `history_clear` | `&Binding, &Identity, now` | `Result<()>` |
| `history_record` | `&Binding, Option<&Identity>, Capture, now` | `Result<Recorded>` |
| `history_prune` | `&Binding, &Identity, now` | `Result<()>` |
| `history_search` | `&Binding, Option<&Identity>, Query, now` | `Result<Page>` |

`Identity` and `Recipient` are typed `age::x25519` keys. The SDK caller supplies
them. No private key file, environment lookup, path, key generation, recovery or
provisioning occurs here. `Binding::new(profile, room, &recipient)` binds exact
32-byte profile fingerprint and room ID to the public recipient. Supply expected
bindings from trusted caller context, never by trusting copied database fields.

`now` is caller-supplied Unix seconds. Mutations and successful keyed status/search
advance a stored clock in the supplied transaction; earlier times are refused.
Commit before exposing read results as well as mutation results. A committed clock
floor survives reopen; rollback discards the observation, so results from that
transaction must not escape. Locked status cannot observe bodies or advance time.
Retention runs on inserts, explicit prune and policy updates; search hides expired
rows without deleting them. The caller must still supply a trustworthy clock.

## State And Migration

New and existing V1 stores are off. Opening V1 never creates history tables or
performs the V2 migration. Opening recognizes only V1 and V2, and validates V2
public state/counters. Unknown versions and malformed states are refused.
Existing storage connection pragmas retain their normal durability setup.

Only `history_enable` migrates V1 to V2, inside the caller's transaction. Migration
adds history tables and changes `user_version` without replacing MLS tables or
metadata. A local savepoint makes migration/each mutation error-atomic without
committing the outer transaction. Dropping the outer transaction rolls everything
back; successful method return is not durable until `Transaction::commit()`.
Commit failure retains the existing poisoned-handle/reopen rule.

An old V1-only binary refuses V2. There is no automatic downgrade or recovery.
Enabling records future calls only and does not backfill existing messages.
Disable changes future recording only, preserving encrypted rows and key check.
Clear explicitly deletes rows but preserves policy, recipient and next ID.
Re-enabling cannot replace the binding/key. Key rotation is not implemented.

Configured stores, including empty, cleared or disabled ones, require a matching
identity and authenticated encrypted key check for access. Status alone returns
`Locked { binding, enabled, policy }` without a matching key, not empty counters.
With a key it returns `Configured` and verified counters; V1 returns `Off`.
Status accepts optional expected context; a supplied binding must match. All
other access requires explicit expected binding. Missing/wrong keys refuse reads
and mutations, never an empty fallback. V1 off operations create no key.

## Record And Search Contract

`Capture` supplies event, epoch, exact-wire SHA-256 reference, borrowed body and
sensitivity. Sensitive inputs are excluded. Only bounded nonsensitive text bodies
are stored. `OutgoingPrepared` and `IncomingProcessed` are storage semantics, not
delivery, peer acceptance or human-read claims. The caller must establish those
facts and the wire hash before invoking storage. No live integration exists yet.

`Recorded` reports `Off`, `SensitiveExcluded` or `Stored { id, event }`.
Each encrypted typed row includes its own version, all binding fields, monotonic
ID, event/direction, epoch, wire hash, capture time and body. Reads authenticate
the whole AGE record and compare expected bindings and the SQL record ID.
No plaintext body, preview or full-text index is written into SQLite.
Public state includes policy, enabled flag, counters, clock and binding; SQL rows
expose IDs, ciphertext lengths and ciphertext. This is not metadata concealment.

Validation, decryption, expiry/oldest pruning, checked counter reservation and
insertion occur within the existing transaction. Counter overflow, inconsistent
counts/lengths, malformed policy and substituted/corrupt rows fail closed.
IDs of committed records are not reused after prune or clear. Rolled-back IDs
were never committed and may be allocated by a later successful transaction.

`Query` contains literal case-sensitive UTF-8 substring, exclusive `after_id`,
result limit and serialized response byte budget. Empty substring lists records.
There is no SQL query interpolation, normalization, remote fetch or persistent
plaintext index. Every query validates/decrypts the bounded retained set before
pagination, including rows not returned by that page. `Page` contains rows,
`next_after` and `has_more`. A budget too small for one matching row errors rather
than presenting an empty page. Pagination is not a cross-transaction snapshot;
pruning between pages can remove records.

## Security And Next Ownership

Row bodies wipe their owned String allocation on drop. Codec-owned plaintext and
serialized private row buffers use `Zeroizing`; page sizing counts without a
second plaintext allocation. Callers must protect any
copies they create. This does not promise erasure of every serde/AGE internal
buffer, allocator copy, swap, journal, backup or physical storage block. Public
recipient encryption is not a signature proving who authored a database record;
this is not an anti-rollback ledger against an attacker replacing the database.

Tests use synthetic in-memory keys/profiles and local temporary stores only.
They cover defaults, exclusion, binding/ID substitution, bounded Unicode search,
retention, byte limits, rollback, populated MLS migration and failed commit.
Runtime/Group capture, same-transaction MLS integration, SDK identity lifecycle,
private key files, CLI, IPC adapters, configuration and UI remain separate work.
