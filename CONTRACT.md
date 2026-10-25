# K&K Contract

> One core. Two verified devices. No mandatory intermediary.
>
> Authority: this contract wins over implementation and other documentation.

Operator authorization 2026-10-25 supersedes the earlier scope gates: finish the
backend primitives, secure pairing/contacts, multi-peer sessions and media, then
the optional UI, verified deployment and publication. Work is performed by the
principal agent alone. The detailed inventory and acceptance conditions are in
[docs/backend-map.md](docs/backend-map.md); unimplemented targets are labelled.
Pairing/profile authorization and its failure semantics are defined by the
normative addendum [docs/pairing.md](docs/pairing.md). Local tests are not a
replacement for the required cross-device and adversarial VM acceptance gate.

Persistent pre-paired nodes and their delivery/resource semantics are defined by
the normative addendum [docs/node.md](docs/node.md). It supersedes the historical
one-shot-only lifetime below without changing the text/file wire formats. Dynamic
rooms and replicated state remain separate gates in [docs/sessions.md](docs/sessions.md).

Unified CLI target: `send TARGET PAYLOAD`, `send TARGET PATH -f`, stdin `-`, and
local sensitive handling `-s`. The `--message` option is removed, not aliased.
Literal sensitive text in argv is refused. All transport remains pinned mutual
TLS; sensitive handling is not a weaker/stronger transport selector. Optional
age encryption retains ciphertext at the receiver and never stages plaintext.
The current CLI/wire version and local input/output policy are defined in the
normative addendum [docs/cli.md](docs/cli.md). Its 0.5 wire formats supersede the
earlier text/file wire descriptions retained below. Explicit decryption may stage
private plaintext; sender-side encryption never does.

| Direction | Current work | Not authorized yet |
| --- | --- | --- |
| Rust core, CLI first, local network first | Messages, discovery and resumable files | Media, browser UI, mobile integration |
| Initial contract approved on 2026-10-24 | Local and isolated multi-guest checks pass | Next layer still requires operator approval |

Contract version lives in `Cargo.toml`. Public API, CLI and wire versions are distinct.

---

## The Whole System

```text
           DEVICE A                                  DEVICE B

  +--------------------------+              +--------------------------+
  | CLI now                  |              | CLI now                  |
  | Browser/mobile later     |              | Browser/mobile later     |
  +------------+-------------+              +-------------+------------+
               |                                         |
  +------------v-------------+              +-------------v------------+
  | K&K CORE                 |              | K&K CORE                 |
  |                          |              |                          |
  | Identity + authorization |              | Identity + authorization |
  | Session + message rules  |              | Session + message rules  |
  +------------+-------------+              +-------------+------------+
               |                                         |
  +------------v-------------+   DIRECT     +-------------v------------+
  | Quinn / Rustls / Ring    +<------------>+ Quinn / Rustls / Ring    |
  | QUIC + mutual TLS 1.3    |  UDP / LAN   | QUIC + mutual TLS 1.3    |
  +--------------------------+              +--------------------------+

          NO cloud account    NO relay    NO discovery server
```

K&K unifies the primitives, not traffic on a central server.
An IP network must already exist. Wi-Fi/Bluetooth link creation is not stage 1.

---

## Identity Before Connection

```text
  Create identity A                          Create identity B
         |                                         |
         +------ exchange PUBLIC certificates -----+
         |       through a trusted channel          |
         +------ compare SHA-256 fingerprints ------+
         |                                         |
  select verified B                         select verified A
         |                                         |
         +---------- mutual TLS verification -------+
                           |
                  application data allowed
```

| Rule | Meaning |
| --- | --- |
| An address is not an identity | IP, name and discovery announcements grant no trust |
| `--peer certificate.der` | Authorizes that exact verified certificate for this invocation |
| Standard TLS validation + exact certificate check | Both sides verify before application processing |
| No automatic first-contact acceptance | No implicit TOFU, short pairing code or trust database |
| Revocation | Stop selecting the old certificate; exchange and verify a new identity |
| Renewal | Not automatic; certificates must remain valid and clocks correct |

### Local Identity

```text
identity/                       Unix mode 0700, created new
  certificate.der               PUBLIC: safe to exchange after verification
  private-key.der               PRIVATE: PKCS#8, Unix mode 0600, never share
```

Never overwrite an identity. Reject symlinks, non-regular files and unsafe modes.
The parent directory must exist and be user-controlled. Bound file reads.

Private storage is Unix-only in this stage; other platforms fail closed until
their protection model is implemented. No at-rest encryption is claimed.
A compromised local account is outside the threat model.

---

## Public Surface

| Command | Input | Success |
| --- | --- | --- |
| `kk identity create <directory>` | New private directory | Public fingerprint |
| `kk identity show <certificate.der>` | Public certificate only | Public fingerprint |
| `kk listen` | `--bind IP:port --identity DIR --peer CERT` | One accepted UTF-8 message |
| `kk send IP:port` | `--identity DIR --peer CERT --message TEXT` | Matching receiver receipt |
| `kk --help`, `kk --version` | No network input | Help or Cargo package version |

| Output | Contract |
| --- | --- |
| stdout | Identity result, received text or send receipt |
| stderr | Diagnostics and actual listener address before waiting |
| Exit 0 / 1 / 2 / 130 | Success / operation failed / invalid syntax / Ctrl-C |

CLI and future UI are adapters. They never own authentication rules.
The library owns operations and typed errors. No JSON-RPC surface is implied.
Clap handles help/version immediately. Terminal output escapes received control
characters; redirected message output preserves the text and appends a newline.

---

## One Session

```text
 Disconnected -> Connecting -> Verifying -> Established -> Closing -> Closed
                     |             |            |
                     +-------------+------------+----> Failed

 SENDER                                         RECEIVER
   |---- length + UTF-8 text + FIN ---------------->|
   |                                               | validate
   |<--- SHA-256(text) + FIN ------------------------|
   | verify receipt                                |
   |---- close ----------------------------------->|
```

| Wire rule | Value |
| --- | --- |
| ALPN | `kk/1` |
| TLS name | `kk.local`, a protocol constant; no DNS lookup |
| Trust roots | Only the explicitly selected peer certificate |
| Payload | 4-byte unsigned big-endian length, then UTF-8 bytes, then FIN |
| Maximum | 65,536 payload bytes; empty text allowed |
| Receipt | Exactly 32 SHA-256 digest bytes, then FIN |
| Channels | One bidirectional stream; no uni streams or datagrams |
| Forbidden | 0-RTT, permissive TLS verifiers, public roots, silent fallbacks |

A receipt means validated receipt in memory, not durable storage or human reading.
Keep the receiver alive until sender close or deadline so queued receipts survive.
Timeout/cancellation can leave delivery uncertain. Never retry automatically.

---

## Bounds And Failure Space

| Bound | Stage 1 behavior |
| --- | --- |
| Operation deadline | Default 10 s; CLI 1-300 s; library >0 and <=300 s |
| Connections | One incoming attempt per invocation, including failure |
| Memory and work | Explicit transport windows, handshake buffers and frame limits |
| Cancellation | Drop operation, close owned resources; no background daemon |
| Input | Reject invalid UTF-8, bad lengths, missing/extra bytes and oversize frames |

One-shot listening is not a resilient multi-user service. An attacker can race
the expected peer and consume its one attempt. Do not claim LAN DoS resistance.

| Failure | Required outcome |
| --- | --- |
| Wrong, changed or revoked peer | Reject; no application acceptance |
| Unreachable peer / firewall | Bounded error, never a hang |
| Malformed / oversized message | Reject before unbounded allocation |
| Slow peer | Deadline and bounded queues; no unlimited task spawning |
| Persistence failure | No claim of remembered trust or durable completion |
| Unsupported capability / OS | Explicit refusal, never weakened protection |

Stable error codes:

```text
IO                     IDENTITY               UNSUPPORTED_PLATFORM
AUTHENTICATION         NETWORK                TIMEOUT
PROTOCOL               MESSAGE_TOO_LARGE      CANCELLED
CONFIGURATION
```

Never log keys, sensitive payloads or remote-controlled error text.
Local mode must not contact telemetry, discovery, STUN, relay or CA services.

---

## Architecture Rules

```text
 CLI/UI  ------>  Library rules  ------>  Transport/OS integration
   ^                   |
   +------ NEVER ------+

 refs/repos/  -X->  build or runtime
 tmp/         -X->  build or runtime
```

| Owner | Responsibility |
| --- | --- |
| `Cargo.toml` / `Cargo.lock` | Identity, contract version / resolved dependencies |
| `src/lib.rs` | Explicit library surface |
| `src/main.rs` | CLI parsing and presentation |
| `src/identity.rs` | Keys, certificates and private-storage policy |
| `src/session.rs` | Authentication, transport lifecycle and bounded messages |
| `src/config.rs` / `src/error.rs` | Typed limits / canonical failures |
| `LICENSE` | Legal terms for new original work |

One invariant, one owner. Add modules only for a real responsibility.
No speculative managers, registries, plugin systems or compatibility wrappers.

A local mistake gets a local fix and regression test. If a fix needs duplicated
state or cross-layer exceptions, reconsider the boundary before adding patches.

---

## Why Quinn Here

| Candidate | Decision for this slice |
| --- | --- |
| TCP + TLS | Viable; would need future stream multiplexing |
| Quinn + Rustls | Selected: native authenticated streams, no external service |
| Iroh | Useful connection machinery, but discovery/relay behavior is not needed yet |
| WebRTC | Later browser/media evaluation; not a native transport assumption |

No custom cryptography. Dependencies come through Cargo, not study clones.
Primary references: [Quinn](https://docs.rs/quinn/0.11.12/quinn/),
[Rustls client verification](https://docs.rs/rustls/0.23.46/rustls/server/struct.WebPkiClientVerifier.html),
[RCGen](https://docs.rs/rcgen/0.14.10/rcgen/fn.generate_simple_self_signed.html).

---

## Gates And Evidence

```text
 [0] Scaffold + contract       ACCEPTED
             |
 [1] Authenticated messages   LOCAL + ISOLATED VM CHECKS PASS
             |
 [2] Discovery               VM GATE PASSED BEFORE FILE IMPLEMENTATION
             |
 [3] Consented files         LOCAL + MULTI-GUEST CHECKS PASS
             |
       operator approval
             |
 [4+] Media / UI / mobile    NOT STARTED
```

| Gate | Evidence required |
| --- | --- |
| Local checks | `make check`, release build, CLI smoke and package isolation |
| Authentication | Both directions, wrong client, wrong server, changed identity |
| Message rules | Empty/max/oversize, malformed framing, deadlines and cancellation |
| Isolated network | Two independent Linux guests, no Internet routes, packet evidence |
| Physical follow-up | Real hardware/Wi-Fi tests, not claimed by VM results |
| Next layer | Explicit operator approval after results and limitations |

Loopback tests are not real-LAN proof, multi-OS coverage, a security audit or a
performance guarantee. Record unperformed checks. Measure latency, throughput,
memory, CPU, recovery and binary size with OS/hardware/workload provenance.

Initial evidence: `make check` passed 22 tests, including malformed frames, false
receipts, mutual rejection, key protection, deadlines, cancellation, IPv4/IPv6 and
two CLI processes. Package checks exclude local references and private scratch.
Two physical devices, Internet-disconnected LAN traffic observation, native Windows
storage, independent security review and throughput benchmarks are not verified.

Network evidence added on 2026-10-24: the operator approved a VM laboratory as the
next validation environment. Ten scenarios pass between Fedora 44 and Mint 22.3,
then pass again after a cold restart through the complete `make kk-test` workflow.
This supersedes the earlier lack of network evidence for the virtual LAN only;
the physical-hardware, audit and benchmark boundaries remain open.

The lab checks both binary hashes, removes NAT adapters and default IPv4/IPv6
routes, exchanges only public certificates and observes K&K-port traffic between
the two guests. Control uses virtio, not a network affected by the test profiles.
The capture is not an all-process traffic audit. Full results and the explicit
privileged-management boundary are in [docs/lab.md](docs/lab.md).

Current 0.4.0 evidence: 37 Rust tests and 27 completed VM scenarios, including
consent, size refusal, idempotent completion, verified-prefix recovery after
actual sender/receiver interruption and refusal of corrupted partial data.
Discovery stays outside the trust boundary and blocked mDNS preserves explicit
address operation. Later scenario extensions and cold replays are recorded in
the lab report; physical-radio and production-audit claims remain excluded.
The final full cold replay is `1f02ac661f62`; file reception is exercised on both
Fedora and Mint, not only on one filesystem.

Targets: Linux, Windows, macOS, then Android/iOS/browser capabilities as tested.
A Rust or Wasm build does not prove discovery, capture or background support.
AI text detectors are used only on explicit operator request.

---

## Authorized Extensions

Stage authorization update (2026-10-24): stage 1 is accepted. Stage 2 discovery
and, after its VM checks pass, stage 3 consented resumable files are authorized.
This does not authorize media, UI or a permanent daemon.

### Stage 2: Discovery Is Not Trust

```text
 Opt-in listener -> mDNS on selected IPv4 interface -> unverified candidates
                                                           |
                                      user-selected certificate still required
```

| Surface | Contract |
| --- | --- |
| `kk listen --advertise NAME` | Advertise only while the actual listener runs; explicit IPv4 bind required |
| `kk peers --interface IP --timeout SECONDS` | Browse `_kk._udp.local.` on that interface, for at most 15 seconds |
| Result | Name, message/file capability, endpoint and claimed SHA-256 fingerprint, always UNVERIFIED |
| Bounds | At most 64 services, 8 IPv4 addresses/service, 63-byte instance labels |
| Invalid data | Ignore unsupported/invalid metadata; report daemon/interface errors |
| Shutdown | Withdraw on normal exit; best-effort withdrawal on cancellation; remote caches can be stale |
| Privacy | Opt-in multicast discloses name, address and public fingerprint to the LAN |

No public DNS, rendezvous server, connection attempt, certificate import or
automatic trust. Explicit addresses remain available when multicast is blocked.
Use the maintained mdns-sd parser rather than inventing a discovery protocol.
Its internal TTL cache is not a hostile-LAN memory-exhaustion defense; output
bounds and deadlines do not constitute a discovery DoS audit.

### Stage 3: Files After The Discovery Gate

The 0.3.0 VM run `e6d64ca97842` passed all 13 scenarios, including discovery,
withdrawal and explicit-address fallback with mDNS blocked. Stage 3 is authorized.

| Invariant | Required implementation |
| --- | --- |
| Consent | Explicit receiver opt-in, chosen destination and maximum byte count |
| Authentication | Reuse the same pinned mutual TLS rules; separate `kk-file/1` ALPN |
| Names | Remote filenames never choose paths; receiver chooses the exact destination |
| Storage | Private directory, owned regular files, no symlinks/hardlinks or destination overwrite |
| Resume | Retain private partial data; bind metadata to content hash/size and peer identity |
| Prefix | Receiver sends offset and prefix hash; sender must verify before continuation |
| Completion | Verify complete SHA-256, sync and publish atomically without replacement |
| Interruption | No success; retain resumable data; do not silently reset conflicting state |
| Retry after lost receipt | A matching completed destination can be acknowledged without rewriting |

The transfer is streamed in bounded chunks. Storage protection is implemented
and tested on Linux first; unsupported storage platforms must fail closed.
The exact wire format, errors, bounds and filesystem assumptions are specified in
the normative addendum [docs/files.md](docs/files.md). TLS authentication and QUIC
resource policy have one owner in `src/transport.rs`, shared with messages.

## Lineage

The idea follows KpihX and shadow19walker's 2024 K&K Meet work.
No legacy code or assets are imported or relicensed by this new project.

Read-only shallow snapshots cloned on 2026-10-24; no automatic updates:

| `refs/repos/` clone | SSH source | Snapshot commit |
| --- | --- | --- |
| `loc-stream` (`master`) | `git@github.com:KpihX/loc-stream.git` | `5e8a9944d5dbd159246e8dd5abb884df37f88ea5` |
| `localsend` | `git@github.com:localsend/localsend.git` | `6e37efd5fe750299758b6778d2b150b31af3b95d` |
| `pairdrop` | `git@github.com:schlagmichdoch/PairDrop.git` | `1b0c9c9d903c3cfc706df1303dc75c3b54d04c77` |
| `pion-webrtc` | `git@github.com:pion/webrtc.git` | `207f81d7b1118243593c7516b7542da3bb9be994` |

Study provenance is recorded, not a completed implementation review.
Copying code requires rights, compatibility and notices review. Upstream licenses stay intact.
