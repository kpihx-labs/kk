# K&K

**Communication between your devices, without an obligatory cloud.**

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
