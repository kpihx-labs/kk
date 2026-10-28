# In-memory history record prerequisite

`kk::envelope::record` seals and opens one bounded binary record entirely in
memory. This is a codec prerequisite, **not durable history**. It creates no
files, temporary files, database, runtime, key, profile or configuration and
does not access environment variables or log payloads.

## SDK contract

- `seal(Zeroizing<Vec<u8>>, &age::x25519::Recipient) -> Result<Vec<u8>>`
  consumes caller-owned plaintext on success and failure.
- `open(&[u8], &age::x25519::Identity) -> Result<Zeroizing<Vec<u8>>>`
  returns plaintext only after whole-record authentication and stream completion.
- Typed keys are deliberately narrower than file `Lock`/`Unlock`: no paths,
  passphrases, scrypt, identity lists, plugins or ambient key discovery.
  Callers that hold serialized keys must validate/parse those separately before
  calling this API. Do not format or log private identities.
- Output is ordinary binary AGE v1, interoperable with standard AGE readers.
  There is no custom ciphertext wrapper, AEAD construction or KDF protocol.

This is **not a universal AGE decoder**: accepted inputs contain one X25519
stanza, zero or one grease stanza and bounded STREAM chunks within the limits below.
The pinned writer emits one grease stanza. ASCII armor is explicitly rejected.

The exact pinned source, `third_party/overrides/age-0.12.1/src/native/x25519.rs`,
declares `Identity(StaticSecret)` with `#[derive(Clone)]` and no `Debug` impl.
Its `Debug` is therefore unavailable, not a redacting formatter. A compile-fail
doctest guards that distinction. `Identity::to_string()` explicitly serializes
the secret into `SecretString`; a synthetic-key test checks only that type's
redacted `Debug`. Never expose, format or log its secret contents.

## Bounds before cryptography

The authoritative limits are the `MAX_RECORD_*` constants in `src/config.rs`.
Plaintext allows `MAX_MESSAGE_BYTES * 6 + 8192`: worst-case JSON text escaping
plus bounded row context. The typed-row caller must validate text, context bytes,
field counts and structure separately; this byte codec cannot validate metadata.
Ciphertext and the line-boundary header scan are capped before parsing.

This gate is resource/policy screening, not a replacement syntax parser. It
permits exactly one `X25519` stanza and at most one inert `*-grease` stanza
(the pinned AGE writer adds grease). All other stanza types are refused before
decryption. The standard AGE parser and X25519 identity retain ownership of
syntax, canonical key validation, header MAC and payload authentication.

The payload cap is nonce + maximum plaintext + ceil(plaintext/chunk size) tags.
Header and recipient-count bounds are unchanged. The pinned AGE reader validates
the final flag and rejects post-final chunks (`primitives/stream.rs`). Opening
drains that reader, drops it, then requires the borrowed bounded Cursor's position
to equal the full input length. No plaintext returns before all authentication
and exact EOF consumption succeed; no custom frame parser or crypto is used.

## Memory, work and latency

Each operation performs a single bounded X25519 recipient operation and bounded
linear header/payload work. Sealing writes to a fixed-capacity ciphertext slice.
Opening allocates its plaintext output once under `Zeroizing`, with an extra
overflow sentinel byte, and never grows/reallocates that allocation. Partial
plaintext on every error path is dropped through the same zeroizing owner.
Returned plaintext remains zeroizing unless the caller explicitly copies it.

**Upstream limitation:** the pinned AGE implementation keeps encryption's
internal plaintext frame in an ordinary `Vec`; its writer does not wipe that
allocation on drop. Its decryption frame uses a secret owner. This codec does
not claim complete process-memory erasure, protection from allocator/core-dump
copies, or erasure of copies retained by callers. Fixing the upstream writer
would require a separately scoped dependency change. No unsafe memory probes
are used to pretend to verify freed-memory erasure.

Proposed separately scoped upstream-owned fix, not implemented here: make AGE's
`StreamWriter::chunk` a `Zeroizing<Vec<u8>>`, updating both sync and async
constructors, and verify finish, error and early-drop paths in AGE's tests. A
zeroizing field avoids adding a writer `Drop` impl that interferes with moving
`inner` out in `finish`. Review async pin projection and buffer reuse as part of
that upstream change. No Cargo or override changes belong to this codec pass.

The API is synchronous so a future SQLite transaction need not cross an await
point. It runs on the current thread, including randomness and public-key work;
there is no wall-clock deadline or hard latency guarantee. Bound individual
records and admission/concurrency at the future caller, and benchmark target
devices before choosing transaction placement. Do not call repeatedly on a UI
or async executor thread expecting nonblocking behavior.

### Observed current-thread timing

Local x86_64 Linux, rustc 1.99.1, unoptimized test profile, debug assertions on:
20 sequential seal/open pairs at the extended cap (401,408 bytes, seven chunks),
without warm-up or load isolation. This is not SQLite transaction performance.

| Operation | Min ms | Median ms | Mean ms | Max ms |
| --- | ---: | ---: | ---: | ---: |
| Seal | 75.264 | 77.255 | 77.405 | 86.025 |
| Open | 75.094 | 77.419 | 77.194 | 79.186 |

Key/fixture preparation and assertions are untimed; seal includes consumed-owner
drop, open excludes returned-owner drop. There is no absolute CI timing threshold.
Reproduce with the ignored test (prints numeric summaries only):
`cargo test --package kk --test envelope_records maximum_record_current_thread_latency_20_iterations -- --ignored --exact --nocapture --test-threads=1`.
Remeasure optimized builds on target devices before future SQLite capture;
these observations are not latency guarantees or transaction-placement approval.

## Evidence and next stage

`tests/envelope_records.rs` covers empty, Unicode and arbitrary binary records,
standard AGE interoperability, exact plaintext bounds and bound-plus-one,
ciphertext/header bombs, recipient multiplicity, hostile stanza types, wrong
identity, bytewise tampering, every truncation and trailing data. It checks the
maximum-size record at each chunk and final authentication tag, chunk-boundary
neighbors, truncations and small/full-frame/concatenated suffixes. Armor is rejected.
Identity-formatting compile failure and synthetic serialized-key
redaction are separate checks. Wiping checks cover the zeroizing public type and
explicit in-place wiping, not freed memory. Tests use
synthetic keys only and the codec has no filesystem API or filesystem effects;
this is source-level evidence, not syscall tracing.

Still required before history integration: explicit opt-in, key lifecycle and
provisioning, profile binding, migration policy, authenticated record-context
binding and replay/substitution policy, bounded database transactions, retention,
and caller concurrency budgets. AGE integrity alone does not bind a record to a
profile or prevent replay of a different valid record under the same key.
