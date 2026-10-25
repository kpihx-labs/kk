# Changelog

## 0.6.0 - 2026-10-25

Implement bounded network OPAQUE pairing with local-only registration, six-word
invitations, monotonic expiry and one global attempt. Encrypt certificate/grant
exchange, confirm both documents, prove both private keys with strict pinned TLS
and bind the proof to its exporter before contact persistence. Add private QR
image export/import, aliases/default profiles, incoming/outgoing permissions,
fingerprint revocation, explicit restore and archived atomic trust updates.

Enforce permissions in the library before effects and at file checkpoints; old
explicit-certificate commands cannot bypass known restrictions. Reject changed
keys, missing identity over existing trust, and stale pairing permission commits.
Resolve account data directories without relying on HOME in service environments.

56 local tests and all 53 cold-start guest scenarios pass. Tests cover malformed,
wrong, expired, replayed and racing attempts, false certificate ownership, missing
confirmation, denied operations, active-stream revocation and concurrent policy
changes. This composition is not independently security-audited. Persistent mesh,
media, optional UI and signed binary distribution remain later gates.

## 0.5.0 - 2026-10-25

Replace the message-specific CLI with positional send/receive and -f for files.
Remove --message, listen and file send/receive aliases; update real VM consumers.
Add bounded stdin, hidden terminal input, cancellation wake-up and zeroized owned
buffers. Sensitive inline arguments are refused, wire-sensitive messages are
masked by default, and private output/reveal are explicit receiver decisions.

Add standard age recipient/passphrase envelopes, private key generation, explicit
authenticated decryption, bounded header parsing and no-replace publication.
Sender artifacts contain ciphertext only and can be retained for resume. Change
text/file ALPN and resume metadata to version 2; preserve/refuse earlier state.

45 local tests and 34 multi-guest scenarios pass, including sensitive stdin,
private output, argv refusal, sealed transfer, correct/wrong recipient keys and
ciphertext modification. These fixtures contain no operator credentials.
The full pairing, contacts, mesh, media, UI and publication scope is authorized
but not yet complete; progress and remaining gates are tracked explicitly.

## 0.4.0 - 2026-10-24

Stage 3 follows the successful discovery VM gate. Add explicit-consent file
receive, bounded streaming, pinned-peer/content-bound partial metadata, independent
prefix verification, full on-disk SHA-256 checks, atomic no-replace publication
and idempotent acknowledgement of an already complete file. Harden Linux staging
against symlinks, hard links, unsafe permissions and concurrent writers.

Move TLS authentication, QUIC limits and connection closure into one transport
module shared with messages. Keep `kk/1`; use `kk-file/1` for the file protocol.
CLI parsing/presentation is separate from the process/signal entrypoint. Normal
file refusals flush a sanitized close frame before process exit.

37 local Rust tests pass. The first 26-scenario VM suite passed, including actual
sender and receiver process interruption, prefix recovery, corrupt-prefix refusal,
constrained networks and no-overwrite behavior. See docs/files.md and docs/lab.md
for precise guarantees, retained results and later cold-replay evidence.

Final cold replay `1f02ac661f62` passed all 27 scenarios, adding reverse-direction
file reception on Mint. Earlier 26-scenario records remain retained as history.

The BUSL change date remains 2030-10-24. No media, UI, installation on the host,
global policy changes, commit, hosted repository or push is introduced.

## 0.3.0 - 2026-10-24

Stage 1 accepted by the operator. Add scoped opt-in mDNS/DNS-SD with mdns-sd,
bounded UNVERIFIED results and asynchronous socket-error reporting. Advertised
metadata does not create a Peer or grant authorization. The 13-scenario VM gate
passed before file implementation, including withdrawal and address fallback
with UDP 5353 blocked. No public rendezvous or permanent daemon is required.

## Stage 1 VM Validation - 2026-10-24

K&K 0.2.0 passed ten guest-to-guest scenarios on Fedora 44 Cloud and Mint 22.3,
then passed the same suite after a complete cold restart. Cases cover clean and
impaired networks, total packet loss, both directions, empty/Unicode/maximum-size
messages and rejection of both incorrect peer identities. Packet evidence and
provenance are retained; no production security or physical-radio claim is made.

The sibling vm-lab owns the isolated network, virtio control adapters, reversible
domain definitions and headless 2-vCPU/1,536-MiB budgets. Its six mocked adapter
tests, Ruff and ShellCheck pass. No Ubuntu guest, global policy change, hosted
repository, commit or push was added. See docs/lab.md for reproduction and limits.

## 0.2.0 - 2026-10-24

Stage 1 adds manual certificate pairing, mutual TLS 1.3 over Quinn QUIC and one
bounded UTF-8 message with a receiver receipt. CLI commands create identities,
inspect public fingerprints, listen once and send once. Private identities use
Unix permission checks; unsupported platforms fail closed.

Local verification covers IPv4/IPv6, two CLI processes, wrong peers, malformed
frames, false receipts, size bounds, deadlines, cancellation and identity files.
The contract and README now use diagrams, spaced sections and short tables.
AI text detectors are on request only for this project. No global policy change
is part of this version.

Two physical devices on an isolated LAN remain unverified. Discovery, files,
media, UI, Windows ACL integration, installation and publication are not included.
The BUSL change date stays 2030-10-24; earlier-version dates are not postponed.

## 0.1.0 - 2026-10-24

Initial Rust scaffold with a library, help/version CLI, CLI acceptance tests and
offline Makefile checks. The architecture contract records trust boundaries,
resource requirements and operator approval gates. Study repositories are isolated
under `refs/repos/`. No network or media operations are implemented.

The new original work uses BUSL-1.1 with the approved noncommercial production
grant and a 2030-10-24 change date to AGPL-3.0-only, subject to the earlier-date
rule in LICENSE. Historical collaborative work and upstream references are not
relicensed.
