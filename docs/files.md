# Files: Consent, Integrity, Resume

> One verified peer. One chosen destination. No remote path authority.
>
> Normative file-protocol addendum to CONTRACT.md.

Current 0.5 changes are normative in [cli.md](cli.md): use `send ... -f` and
`receive -f`, ALPN `kk-file/2`, a one-byte sensitive flag before the offer and
73-byte resume metadata. The earlier 0.4 layout below is retained as history;
it is not accepted by the new wire protocol or silently migrated.

## User Flow

```text
 SENDER                                  RECEIVER
    |                               choose output + limit
    |                               explicitly --accept
    +---------- pinned mutual TLS ----------+
    |---- size + content hash ------------->|
    |<--- retained offset + prefix hash ----|
    | verify prefix                         |
    |---- remaining bytes + FIN ----------->|
    |                               verify whole file
    |                               sync + publish, no replace
    |<--- content-hash receipt + FIN --------|
    +--------------- close ---------------->|
```

## Commands

Receiver, with an existing user-owned inbox of mode 0700:

```sh
kk receive -f --bind 192.168.124.10:45454 \
  --identity ./alice --peer ./bob.der \
  --output ./inbox/report.pdf --max-bytes 67108864 --accept
```

Sender:

```sh
kk inspect ./report.pdf
kk send 192.168.124.10:45454 ./report.pdf -f \
  --identity ./bob --peer ./alice.der
```

These examples assume an installed binary; in the checkout use
`./target/release/kk`. No remote filename is sent or used. `--accept` authorizes
one file from the pinned peer within the byte limit.

`--advertise NAME` is optional on the receiver. `kk peers` reports an UNVERIFIED
file endpoint, never a certificate or permission grant.

## Wire Contract

| Item | Exact format |
| --- | --- |
| ALPN | `kk-file/1`, separate from message `kk/1` |
| Offer | 8-byte unsigned big-endian size + 32-byte SHA-256 |
| Resume proposal | 8-byte unsigned big-endian offset + SHA-256 of retained prefix |
| Request remainder | Exactly size minus offset bytes, then FIN |
| Completion receipt | Exactly 32 content-hash bytes, then FIN |
| Rejections | Close without a receipt; receiver logs its typed local reason |

A receipt is the peer's statement, not remote attestation. Honest K&K receivers
verify and sync before sending it. A compromised authorized peer can still lie.

## Storage States

```text
 output absent
      |
 output.kk-part + output.kk-meta       private staging
      |                               retain on interruption
      v
 full hash verified + sync
      |
 atomic RENAME_NOREPLACE
      v
 output                               complete, mode 0600
```

| Rule | Enforcement |
| --- | --- |
| Directory | User-owned 0700; final directory component cannot be a symlink |
| Staging | Descriptor-relative opens, NOFOLLOW, owned 0600 regular files, one hard link |
| Concurrency | Exclusive nonblocking lock; another writer is refused |
| Binding | 72-byte metadata: offer (40 bytes) + pinned certificate SHA-256 |
| Resume | Rehash retained bytes; sender independently checks the prefix |
| Publication | Linux atomic no-replace rename; sync file and directory |
| Existing output | Only acknowledge identical private content; never rewrite it |
| Conflict | Preserve data and fail; never silently truncate or reset |

Directory ancestors are trusted operator-supplied paths. A compromised local
account is outside the model. Sources must be regular files without a final
symlink and remain stable; no filesystem snapshot is implied.

## Bounds And Failure

| Limit | Value |
| --- | --- |
| Default receiver cap | 64 MiB; explicit override up to 1 TiB |
| User-space I/O chunk | 64 KiB; whole files are never buffered in memory |
| Default deadline | 300 seconds, including hashing and sync |
| Maximum deadline | 24 hours |
| New errors | INTEGRITY, DESTINATION, FILE_TOO_LARGE |
| Receive storage | Linux first; unsupported platforms fail closed |

Partial bytes are not a power-loss durability promise. Restarting rehashes what
actually remains; corrupt prefixes are refused, not erased. Filesystem calls
are not hard-real-time cancellable. Publication may finish around a timeout,
so a sender that missed its receipt retries explicitly: an identical completed
file can be acknowledged without rewriting.

## Verification

Local tests cover binary/empty files, chunk boundaries, prefix resume, changed
content/peer metadata, bad frames and hashes, symlinks, hard links, concurrency
and publication races. VM results, including actual process interruptions and
corrupt-prefix refusal, are recorded in [lab.md](lab.md). These tests are not a
production audit, a throughput benchmark or a whole-VM power-loss guarantee.
