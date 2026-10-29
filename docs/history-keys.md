# Local history key prerequisite

This helper owns only explicit creation and loading of a per-room AGE key.
It does not open SQLite, enroll history, change policy, capture messages, or
decide whether an existing key is an orphan. SDK-provided identities bypass it.

## Integration interface

All items below have `pub(crate)` visibility in `rooms::store::history_key`:

```rust,ignore
struct HistoryKeyAnchor;
impl HistoryKeyAnchor {
    fn open(path: &Path, owner: [u8; 32], room: [u8; 32]) -> Result<Self>;
    fn verify(&self) -> Result<()>;
    #[cfg(unix)]
    fn directory_file(&self) -> Result<std::fs::File>;
    fn initialize(&self) -> Result<HistoryKey>;
    fn load(&self) -> Result<HistoryKey>;
}
struct HistoryKey;
impl HistoryKey {
    fn recipient(&self) -> age::x25519::Recipient;
    fn with_identity<T>(&self, use_key: impl FnOnce(&age::x25519::Identity) -> T) -> T;
}
```

The SQL owner adds the module export. Keep the anchor alive for the room scope;
do not reopen a fresh anchor to mask a failed identity check. `directory_file`
returns a cloned validated room descriptor for the SQL owner's anchor API.
Call `verify` around SQL publication under the existing cooperating-writer gate.
Do not substitute a pathname-only database open for descriptor anchoring.

The path is an existing absolute room-private directory. `owner` is the public
profile fingerprint, not a private seed; `room` is the public room identifier.
The SQL database is authoritative: compare its binding and recipient before
using a loaded key. Loading proves file format and requested public binding,
not enrollment or authorization. An existing file always blocks initialization,
even if it is valid, malformed, a dangling link, or belongs to another room.

## Storage and publication

The private filename is `history.agekey`. Its strict bounded text format is a
version comment, lowercase public profile and room binding comments, and one
canonical standard AGE X25519 secret identity line. AGE owns key generation
and parsing. There is no custom cryptography or unanchored envelope keygen call.

Unix walks from a retained filesystem root descriptor, opening every component
with `NOFOLLOW`, `DIRECTORY` and `CLOEXEC`. Every operation rechecks each named
ancestor's device/inode identity and the retained room directory's ownership
and exact private mode. Parent traversal and symlink components are refused.
Key reads use descriptor-relative `NOFOLLOW`, `CLOEXEC`, `NONBLOCK` opens and
require an owned, single-link, private regular file with bounded content.

Creation is explicit only. Linux and Apple stage an exclusively created private
file relative to the retained room descriptor, sync the file, revalidate the
anchor and staging inode, atomically rename with `NOREPLACE`, then sync the
directory and verify again. Other platforms refuse initialization rather than
fall back to replacement or unsafe path resolution. Non-Unix loading refuses.

Initialization holds an independently opened, nonblocking directory lease across
scan, generation, publication and verification. Its guard explicitly unlocks on
return or unwind: a duplicated descriptor cannot extend a completed operation.
An active initializer still excludes both retained-anchor and fresh-anchor writers.

Failures never delete or replace private state. A failed write, file sync,
publication race or ancestor check can leave a private `.pending` file. A
directory-sync failure can leave the published key; the error must still reach
the caller. Do not infer absence or successful enrollment from that error.
Crash recovery, orphan handling and adoption require the explicit Group
workflow; this helper provides no delete, replace, repair or adoption operation.

The guarantee assumes the existing cooperating writers and private path scope.
It detects observed path swaps; it is not a sandbox against malicious same-UID
writers able to swap names between checks, mutate an open inode, or restore
an ancestor before revalidation. Public binding comments are not a MAC: SQL's
stored recipient binding must detect a different otherwise-valid replacement.

## Secret handling and evidence

`HistoryKey` has no Debug, Clone, serialization or private export interface.
The borrowed identity callback is for the trusted record codec. Rust cannot
prevent trusted callback code from deliberately copying an identity. Helper-owned
file bytes use fixed/preallocated zeroizing buffers, including failure paths;
AGE's formatted secret uses its own secret container. No claim is made about
complete erasure of dependency-internal allocations, OS caches or storage.

Run `cargo test --test history_keys`. The standalone integration target includes
the two new source files without touching the active SQL owner's module file.
Tests use synthetic temporary directories and generated test identities only.
They cover codec round trips, explicit-only loading, public-binding mismatch,
malformed and oversized files, modes, symlinks, hardlinks, FIFO refusal,
concurrent initialization, logical lease release with retained descriptors,
ancestor/root replacement, publication path swaps,
and injected file/directory sync failures with private-state preservation.
They are local filesystem regression evidence, not power-loss durability tests
or actual profile/runtime enrollment qualification.

The [native key qualification](native-key-qualification.md) separates the local
lease regression from the still-unclassified native pending-artifact failure.
