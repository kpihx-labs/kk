# K&K

## Current Candidate: 0.8.1

The dependency hold recorded for 0.8.0 below is resolved in the local 0.8.1
candidate, without an audit ignore. The full product graph passes strict RustSec
audit; all 155 Rust, six JavaScript and 11 packaging checks pass. Preserved
upstream expansion vectors, independent old/new OPAQUE wire tests and the Hax
macro check also pass. See [the scoped dependency review](docs/dependency-overrides.md).
Final hosted CI, frozen VM replay and authenticated release artifacts are still
required; this source update alone is not a published release. Older hold and
checkpoint entries below are retained dated history.

## Release Gate Blocked (2026-10-27)

This is a tested software candidate, not a security-qualified production release.
The final RustSec pass reports four crypto advisories and two maintenance warnings
in the current locked graph. No release tag has been created. Upstream remediation
snapshots are being evaluated outside the product dependency graph; experimental
snapshots are not silently promoted or substituted for the tested build.
See [the blocking findings](docs/security-review.md). The release workflow now
requires the unsuppressed dependency audit before signing or publishing a release.

**Communication between your devices, without an obligatory cloud.**

## Current Linux Delivery: 0.8

The implemented stack now includes confirmed room enrollment, distinct data and
voter authority, durable crash recovery, consented resumable files, targeted copy
availability, native Opus/VP8 media and the bundled neutral-grey light/dark UI.
The separate post-integration review found and fixed two additional boundary
counterexamples. Current evidence is in [progress](docs/progress.md) and the
[software review](docs/security-review.md); the older checkpoint paragraphs below
are retained historical snapshots, not the current command inventory.

```sh
make check
make build
./target/release/kk ui
# A separately confirmed device uses its own profile and backend.
```

`kk ui` stays in the foreground and opens a private single-use browser launch.
Initialize the device explicitly, create/join a room, compare CONFIRM on both
devices and separately consent to file/media reception. No mandatory intermediary,
background service, remote-triggered capture or automatic secret replication is
installed. See [the UI](docs/ui.md), [explicit copies](docs/replicas.md),
[native media](docs/media.md), [platform claims](docs/platforms.md) and
[archive/provenance verification](docs/distribution.md).

Full-feature native Windows/macOS protection and capture are not qualified and
are not advertised by these Linux artifacts. Linux ARM64 is conditional on its
actual required CI result. A responsive mobile layout is not a physical-device
or permission-portal qualification. This is not an independently audited production
security product.

Latest unreleased integration (2026-10-26): `kk ui` now provides the foreground
grey light/dark device workspace over the existing room SDK. Browser pairing,
text, consented files and live synthetic VP8/Opus encode/transport/decode pass in
the operator's configured Edge profile; responsive layout passes at 390 x 844.
The full host check passes 147 Rust and five JavaScript tests. These results do
not close physical-device, replica-availability, platform, deep-review or release
gates. See [the UI boundary](docs/ui.md) and [current evidence](docs/progress.md).
The older progress paragraphs below are retained historical snapshots.

Current unreleased progress: the public room CLI/file slice passes fourteen real
three-VM scenarios plus the complete 66-case baseline, including explicit verified
copy forwarding after the original VM is off. Native media work has begun with
`kk media doctor` and explicitly consented local preview, not room conferencing.
See [native media boundaries](docs/media.md) and [current evidence](docs/progress.md).
The in-progress notes below predate these results. Final product delivery remains
open, including live room media, GUI, platform releases and the deep review.

## Public Room CLI (Unreleased)

The working tree now exposes `kk room create`, `serve`, `invite`, `join`, `send`,
`status`, `controllers`, `remove` and `stop`. Three independent CLI processes pass
network enrollment, broadcast, explicit voter selection and cold resume on the
host. A dedicated three-VM public-CLI gate is being run; earlier fixture reports
are not substituted for this gate. See [the room CLI](docs/cli.md#public-rooms)
and [current evidence](docs/progress.md). Room files/replicas, native media, GUI,
portable packaging and the final security review are still incomplete.

The checkpoint descriptions below are retained historical observations, not the
current command inventory or test totals.

Verification completion note (2026-10-26): the repaired transport passes the ten
private cross-guest scenarios AND all 66 paired-node regression scenarios. Host
checks pass 111 Rust/19 lab tests; guests are stopped. Earlier replay-in-progress
wording below describes the preceding state, not a current failure. The remaining
product gates are still open; this is not a complete-product release.

Latest verification boundary (2026-10-26): ten actual private three-guest scenarios
passed on the recorded runtime slice, including creator-off delegated admission,
restart, sender-generation continuity and pre-crash replay refusal. Subsequent
transport hardening exposed another reconnect timeout; its cache fix and the full
regression are being replayed. Local checks pass 111 Rust tests. This remains an
unreleased SDK/CLI working tree, not a completed room CLI, scoped invitation/voter
lifecycle, media stack, GUI or signed portable release. Earlier first-run wording
below is retained as provenance; current evidence is in `docs/progress.md`.

Unreleased source update: public room consensus/recovery passes a real three-VM
gate, and the library now has private runtime, bounded actors and replicated
encrypted admission artifacts. The first actual private room VM fault gate is
running. This is not yet a complete `kk room` CLI, media stack, GUI or signed
portable product. Current versus historical capabilities are distinguished in
[the backend map](docs/backend-map.md) and [progress](docs/progress.md).

## Development Checkpoint: 0.8

The library now contains tested MLS group crypto and protected private persistence.
**There is no complete `kk room` network command yet.** Ordered control and actual
room transport/replication are the next boundary; media and UI remain gated.
See [room primitives](docs/rooms.md), [runtime boundary](docs/room-runtime.md) and
[exact evidence](docs/progress.md). Existing send/receive/node commands remain usable.

## Persistent Nodes: 0.7 Checkpoint

Already paired PCs can keep one foreground receiver open for successive exchanges:

```sh
kk serve --bind LOCAL_IP:45454
# Another terminal, same profile:
kk send alice "hello"
kk status
```

For file reception, explicitly add `--accept-files --inbox PRIVATE_DIRECTORY`.
The [node contract](docs/node.md) defines trust, delivery and resource semantics.
Three independent guests exercise link reuse, bulk/control isolation, live
revocation, partitions and process crashes. Exact versioned results are recorded
in [progress](docs/progress.md); older checkpoint descriptions are retained below.

The reproducible harness is now in [lab/](lab/README.md): `make lab-check`,
`make lab-test`, `make lab-stop`. It is not a product runtime dependency.
Dynamic room admission, replicated state, media and the UI are still separate gates.

```text
       YOUR PC                                      ANOTHER PC
  +----------------+      direct local network     +----------------+
  | K&K CLI        +<----------------------------->+ K&K CLI        |
  | Rust core      |       verified identities     | Rust core      |
  +----------------+                              +----------------+

            Messages + discovery + files. Media and UI later.
```

## Where We Are

| Available | Verified | Later, separately approved |
| --- | --- | --- |
| Unified transfer, PAKE/QR pairing, contacts, grants and revocation | 56 Rust tests + 53 VM scenarios | Persistent mesh, media, UI, portable publication |

The contract is approved. Local tests include IPv4, IPv6 and two CLI processes.
Fedora 44 and Mint 22.3 pass real guest-to-guest messaging, discovery and file scenarios.
The earlier 27-scenario suite passed after a cold restart. The 0.5 CLI and envelope
extension passed 34 VM scenarios. Full product completion is still in progress;
see [the backend map](docs/backend-map.md) and TODO.md for unimplemented targets.
The 0.6 cold replay passes 53 scenarios, including active-transfer revocation and
refusal to overwrite a concurrent local permission change during pairing.
This is a development tool, not an audited production security product.

## Build

Rust with edition 2024 support, rustfmt, Clippy, Make and Zsh are required.

```sh
cargo fetch --locked    # Download dependencies once
make check              # Offline format, lint, tests and documentation
make build              # Offline release build
./target/release/kk --help
```

Keep `Cargo.lock` with the source. No installer or daemon is created.

## Try Two Terminals

Create two identities on this PC using fresh directory names:

```sh
./target/release/kk identity create ./tmp/alice
./target/release/kk identity create ./tmp/bob
```

**Terminal A: receive once.**

```sh
./target/release/kk receive --bind 127.0.0.1:45454 \
  --identity ./tmp/alice --peer ./tmp/bob/certificate.der --timeout 60
```

**Terminal B: send once.**

```sh
./target/release/kk send 127.0.0.1:45454 \
  --identity ./tmp/bob --peer ./tmp/alice/certificate.der "hello"
```

```text
 A prints: hello       B prints: ACK 5 bytes       Both exit.
```

This example is loopback, not a two-device LAN test.

## Pair Two Devices

The current code-based path needs no manual certificate files:

```sh
# A: choose the local peer name and rights granted to it.
./target/release/kk pair --listen 192.168.1.10:45453 --as bob --allow text,files

# B: enter the code through hidden input, then compare CONFIRM on both screens.
./target/release/kk pair 192.168.1.10:45453 --as alice --allow text,files

# Later operations use the saved identity and certificate.
./target/release/kk receive --bind 192.168.1.10:45454 --from bob
./target/release/kk send alice "hello"
./target/release/kk trust revoke alice
```

QR export/import, private profiles and the security/failure model are documented
in [pairing.md](docs/pairing.md). The manual certificate path below remains an
explicit low-level option and cannot bypass revocation in its selected profile.

```text
  PC A creates its identity          PC B creates its identity
             |                                  |
             +--- exchange certificate.der ------+
             |    NEVER private-key.der         |
             +--- compare SHA-256 fingerprints --+
             |    through a trusted channel     |
             +--- select the verified --peer ----+
```

Inspect a received public certificate with `kk identity show <certificate.der>`.
Compare its fingerprint with the owner's result before selecting it as `--peer`.
Use the receiver's actual LAN IP instead of `127.0.0.1` and allow its chosen UDP
port through the firewall. No DNS, cloud service or automatic trust is involved.

| Limit | Behavior |
| --- | --- |
| Message | UTF-8, at most 65,536 bytes |
| Listener | One attempt, then exit; restart after any failure |
| Deadline | Default 300 s; `--timeout` accepts 1-86400 s |
| Ctrl-C | Cancels the operation; exit 130 |
| Success receipt | Validated receipt in memory, not durable storage |
| Private storage | Unix permissions required; Windows ACL support is not implemented |

Keys are not encrypted at rest. A changed identity requires a new verified
certificate. A timeout can leave delivery uncertain; do not retry blindly.

## Find A Listener

```sh
# Add to the receiving command: --advertise alice
./target/release/kk peers --interface 192.168.124.20 --timeout 3
```

Results are always **UNVERIFIED**. The selected local IPv4 interface is explicit;
discovery never imports a certificate or authorizes a connection. Addresses still
work when multicast is unavailable. Advertising exposes public metadata to the LAN.

## Transfer A File

```sh
# Receiver: choose a fresh private output directory, then explicitly consent.
mkdir -m 700 ./tmp/inbox
./target/release/kk receive -f --bind 192.168.124.10:45454 \
  --identity ./tmp/alice --peer ./tmp/bob/certificate.der \
  --output ./tmp/inbox/report.pdf --accept

# Sender: same verified certificates as for messages.
./target/release/kk send 192.168.124.10:45454 ./report.pdf -f \
  --identity ./tmp/bob --peer ./tmp/alice/certificate.der
```

The receiver defaults to a 64 MiB cap. It streams into a private partial file,
verifies the complete hash and publishes without replacing an existing file.
Repeat the commands to resume after interruption; a corrupt/conflicting partial
is preserved and refused, not silently erased. See [files.md](docs/files.md) for
the exact protocol, limits and Linux storage requirements.

## Sensitive Input And Envelopes

```sh
./target/release/kk send IP:PORT - -s --identity ID --peer CERT
./target/release/kk send IP:PORT ./report.pdf -f -s --identity ID --peer CERT
```

`-s` accepts text only from stdin or hidden local input, not literal argv. A
sensitive receipt is redacted unless the receiver explicitly reveals or saves it.
Ordinary traffic uses the same pinned TLS; it is never sent unencrypted.

For ciphertext retained after reception, use `--seal-to AGE_RECIPIENT` or `--seal`
(local passphrase prompt). The recipient receives an age file, not an automatically
decrypted value. See [cli.md](docs/cli.md) for key generation, explicit decryption,
retained artifacts and compatibility boundaries.

## Isolated VM Lab

The sibling `vm-lab/` project owns provisioning, resource limits and network tests:

```sh
make -C ../vm-lab check      # No guest changes
make -C ../vm-lab kk-test    # Boot, deploy and run the full scenario suite
make -C ../vm-lab kk-stop    # Stop both guests and their private network
```

Both guests use 2 vCPU and 1,536 MiB, without a desktop session or Internet route.
See [the lab report](docs/lab.md) for packet evidence, reproduction and boundaries.
Physical Wi-Fi, Windows/mobile storage and sustained throughput remain unverified.

## Read The Project

```text
 CONTRACT.md       Architecture, guarantees, failures and stage gates
 src/              One Rust library and a thin CLI
 tests/            Observable behavior and package isolation
 refs/repos/       Read-only study clones, never build dependencies
 tmp/              Ignored experiments
 TODO.md           Next work, not a claim of implemented capabilities
```

One layer works and is verified before the next begins.
See [CONTRACT.md](CONTRACT.md) for the visual map and exact boundaries.

## Origins And License

The idea follows the 2024 K&K Meet project by KpihX and shadow19walker.
No code or assets from that collaborative work are imported or relicensed.

| License | Meaning |
| --- | --- |
| BUSL-1.1 | Source-available until conversion; not OSI open source yet |
| Production grant | Personal noncommercial use; noncommercial teaching and academic research |
| Other production | Requires a commercial license |
| Evaluation and tests | Allowed, including in companies |
| Conversion | AGPL-3.0-only on 2030-10-24 or the applicable earlier date in LICENSE |

The complete terms are in [LICENSE](LICENSE).
Reference repositories and dependencies keep their own licenses.
