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
stanza, zero or one grease stanza and one payload frame within the limits below.
The pinned writer emits one grease stanza. ASCII armor is explicitly rejected.

The exact pinned source, `third_party/overrides/age-0.12.1/src/native/x25519.rs`,
declares `Identity(StaticSecret)` with `#[derive(Clone)]` and no `Debug` impl.
Its `Debug` is therefore unavailable, not a redacting formatter. A compile-fail
doctest guards that distinction. `Identity::to_string()` explicitly serializes
the secret into `SecretString`; a synthetic-key test checks only that type's
redacted `Debug`. Never expose, format or log its secret contents.

## Bounds before cryptography

The authoritative limits are the `MAX_RECORD_*` constants in `src/config.rs`.
Plaintext fits in one AGE STREAM frame. Ciphertext is bounded before parsing;
the header is scanned only within its smaller cap, using the existing file
envelope's line-boundary MAC delimiter pattern.

This gate is resource/policy screening, not a replacement syntax parser. It
permits exactly one `X25519` stanza and at most one inert `*-grease` stanza
(the pinned AGE writer adds grease). All other stanza types are refused before
decryption. The standard AGE parser and X25519 identity retain ownership of
syntax, canonical key validation, header MAC and payload authentication.

The remaining payload must fit one nonce and one authenticated STREAM frame.
This bounds frame work before crypto and makes appended bytes part of the
same authenticated frame, rather than allowing ignored suffixes. Wrong keys,
tampering, truncation, suffixes, armor and oversized records fail without a
plaintext result. These restrictions are intentionally narrower than general
AGE files; multi-recipient and multi-frame files are not record inputs.

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
20 sequential seal/open pairs for one maximum-size record, no warm-up or load
isolation. This observes one chunk only, not multi-chunk or SQLite performance.

| Operation | Min ms | Median ms | Mean ms | Max ms |
| --- | ---: | ---: | ---: | ---: |
| Seal | 18.717 | 19.091 | 21.026 | 30.089 |
| Open | 21.888 | 22.600 | 24.865 | 50.838 |

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
maximum-size record at nonce/body positions, every final authentication-tag byte
and truncations immediately before and throughout that tag; valid ASCII armor
is rejected. Identity-formatting compile failure and synthetic serialized-key
redaction are separate checks. Wiping checks cover the zeroizing public type and
explicit in-place wiping, not freed memory. Tests use
synthetic keys only and the codec has no filesystem API or filesystem effects;
this is source-level evidence, not syscall tracing.

Still required before history integration: explicit opt-in, key lifecycle and
provisioning, profile binding, migration policy, authenticated record-context
binding and replay/substitution policy, bounded database transactions, retention,
and caller concurrency budgets. AGE integrity alone does not bind a record to a
profile or prevent replay of a different valid record under the same key.
