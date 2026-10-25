# K&K: Isolated VM Lab

> Two kernels. Two libc versions. Real UDP between guests.
>
> Not a physical Wi-Fi test or a production security audit.

Current core: K&K 0.5.0, unified payloads + protected input + age envelopes.
The stage 1 evidence below is retained as history; later gates are recorded next.

## Discovery And File Gates

0.5 extension: run `4bd7dfba2ee2` passed all 34 scenarios. The seven new cases use
synthetic content only: redacted stdin delivery, private output, inline-secret
refusal, sealed transfer, explicit decryption, wrong-key rejection and modified
ciphertext rejection. Private decryption identities never leave the guest.
The result is `kk-results-4bd7dfba2ee2-1792924854156865575.json`.
Local checks now pass 45 Rust tests; the earlier counts below remain historical.

| Gate | Evidence |
| --- | --- |
| Discovery, before files | 0.3.0 run `e6d64ca97842`: 13 scenarios passed |
| Files, first full run | 0.4.0 run `359f9856b927`: 26 scenarios passed |
| Files, cold replay | 0.4.0 run `2ee80708c724`: 26 scenarios passed |
| Full final cold replay | 0.4.0 run `1f02ac661f62`: 27 scenarios passed, including reverse files |
| Local checks | 37 Rust tests; 6 mocked lab-control tests; Clippy/Ruff/ShellCheck |

The discovery gate tests UNVERIFIED metadata, withdrawal, and pinned explicit
address operation while UDP 5353 ingress is blocked on both guests. The file
gate adds explicit consent, 4 MiB binary transfer, empty files, completed-file
retry without rewriting, size/existing-file/symlink/authentication refusals,
constrained networks and total loss.

Actual SIGINT interruption is applied to each transfer side. The first file run
retained and resumed from 83,995 bytes after sender cancellation, and 65,540 bytes
after receiver cancellation. Both completed with the source SHA-256. A separate
modified prefix was refused and preserved. These are observations, not fixed
chunk-alignment assumptions. Local hostile-frame and filesystem-race tests are
described in [files.md](files.md).

The first full file capture recorded 2,797 K&K-port UDP packets with zero reported
kernel drops and no packet outside the two guest addresses. This filter does not
capture multicast discovery or every background process. The no-NAT/no-default-
route topology is the separate Internet-isolation control.

```text
kk-results-e6d64ca97842-1792872260293089163.json
kk-results-359f9856b927-1792874254301291235.json
kk-results-2ee80708c724-1792878174471553143.json
kk-results-1f02ac661f62-1792878428943189369.json
```

The final suite also exercises reverse-direction file reception, so both kernels
and both guest filesystems perform the no-replace storage path. The complete
27-scenario cold replay passed. No private key is copied to the controller.

## Topology

```text
 HOST: libvirt controller                     No host application relay
              |
              +---- virtio management channel ----+
              |                                  |
       +------v-------+                   +------v-------+
       | Fedora 44    |   ISOLATED LAN     | Mint 22.3    |
       | Cloud        +<----------------->+ Ubuntu base  |
       | .124.10      |    QUIC / TLS      | .124.20      |
       +--------------+                   +--------------+
              |                                  |
              +--------- virbr-kk ---------------+

       No NAT adapter. No default IPv4/IPv6 route.
       No DNS forwarder. Administration does not use this LAN.
```

The actual addresses are `192.168.124.10` and `192.168.124.20`.
`vm-lab/xml/kk-isolated.xml` owns the topology and MAC reservations.

## Resource Profile

| Guest | vCPU | Assigned RAM | Boot target | Kernel / libc |
| --- | --- | --- | --- | --- |
| Fedora 44 Cloud | 2 | 1,536 MiB | `multi-user.target` | 6.19.10-300.fc44 / glibc 2.43 |
| Mint 22.3 | 2 | 1,536 MiB | `multi-user.target` | 6.14.0-37-generic / glibc 2.39 |

The earlier definitions assigned 4 vCPU and 6 GiB to each guest.
The lab therefore uses 75% less assigned RAM and half the vCPU count.

Observed host RSS after the first suite: about 753 MiB for Fedora and 530 MiB for
Mint, about 1.25 GiB combined. Guest swap-in/out counters were zero. These are
observations, not permanent caps or guarantees under other workloads.

The installed libosinfo profile recommends 2 GiB for generic Fedora guests.
The smaller budget here is deliberately tested for a headless CLI workload, not
claimed suitable for a Workstation desktop or future media/compilation workloads.

Installer media, SPICE display/audio and unused input devices are removed from
the final headless definitions. Windows is untouched. Guests and the dedicated
network are stopped outside test runs, without deleting their disks or results.

## Verified Scenarios

First completed run: `c67f6d035a7e`, 2026-10-24, K&K `0.2.0`.

Cold-start replay: `6cc0bbbad743`, also 2026-10-24. The full `make kk-test` chain
booted the headless guests, rebuilt and redeployed the same binary, made fresh
identities and passed all ten scenarios again. Fedora reported SELinux `Enforcing`.

| Scenario | Expected | Observed |
| --- | --- | --- |
| Clean guest-to-guest exchange | Matching receipt and text | Pass |
| 1 Mbit/s, 50 ms per direction, 1% loss | Exchange completes | Pass |
| 256 kbit/s, 150 ms per direction, 3% loss | Exchange completes | Pass |
| 100% packet loss | Timeout, no fake success | Pass |
| Reverse direction | Both OSes act as sender and receiver | Pass |
| Empty text | ACK for zero payload bytes | Pass |
| Unicode and multiline text | Exact UTF-8 receipt | Pass |
| Maximum payload | ACK and exact 65,536 bytes | Pass |
| Wrong server certificate | Authentication fails, no message output | Pass |
| Wrong client certificate | Authentication fails, no message output | Pass |

The deployed binary hash is checked inside both guests before the suite.
Each run creates independent identities inside the guests. Only public
certificates cross the management channel; private keys are never exported.

The first capture recorded 140 UDP packets on the K&K port, zero reported kernel
drops and no captured packet outside the two guest addresses. This filter covers
K&K traffic, not every background OS packet. The network definition and absence
of default routes provide the separate Internet-isolation checks.

Full provenance and results remain in:

```text
vm-lab/tmp/kk-state-<run-id>.json
vm-lab/tmp/kk-results-<run-id>-<timestamp>.json
```

Logs include kernel/libc versions, profiles, binary hash, exit statuses and a
packet summary. Run IDs and filenames keep earlier evidence instead of replacing
it. Loss profiles are stochastic; an individual run is not a reliability study.
Harness timings include control RPC overhead and are not pure network RTT.

Recorded successful results:

```text
kk-results-c67f6d035a7e-1792868836449548811.json
kk-results-6cc0bbbad743-1792869620649883020.json
```

The stage 1 lab's non-invasive `make check` passed six mocked control-adapter tests,
Ruff and ShellCheck. The corresponding core check covered 22 Rust tests; current
counts are in the discovery/file gate table above.

## Reproduce

From the sibling `vm-lab/` directory, after initial guest provisioning:

```sh
make check       # Lint + shell checks + mocked adapter tests; no VM changes
make kk-test     # Boot, build, deploy, isolate, pair and run the full suite
make kk-stop     # Graceful shutdown and private-network stop
```

`make kk-test` intentionally changes only the two test guests and their private
network. `make kk-stop` never force-destroys a guest and preserves test artifacts.
The normal host network and libvirt default NAT are not reconfigured.

Final verified state after the cold-start replay: both Linux domains are shut off,
their dedicated network is inactive and not set to autostart, and Windows remains
shut off. The pre-existing default NAT stays active as before the experiment.

### Initial Provisioning

Both earlier QCOW2 disks were blank. The lab keeps those original disks and backs
up domain definitions under `/tmp/opencode/kk-vm-backup-*/` as `.xml.bak` files.
New disks are `~/VMs/images/kk-fedora-44.qcow2` and `kk-mint-22.3.qcow2`.

| Source | Use |
| --- | --- |
| Fedora 44 Cloud Base 44-1.7 | Small, headless Fedora system; no new distribution |
| Existing Mint 22.3 ISO | Unattended installation, then offline finalization |
| `05-kk-prepare.py fedora` | Verify Fedora image, create overlay, configure and boot |
| `05-kk-prepare.py mint` | Install Mint and finalize the agent/text target offline |

Fedora's CHECKSUM signature and image SHA-256 were verified. Public signing keys
came from `https://fedoraproject.org/fedora.gpg`; the checksum and image came from
the Fedora release distribution. No Ubuntu 26.10 guest was added.

## Explicit Management Boundary

The trusted hypervisor uses QEMU Guest Agent over virtio, not SSH or the network
being impaired. This grants privileged management of disposable guests.

Fedora keeps SELinux enforcing. Its labelled `kk-lab-control` entrypoint and
`virt_qemu_ga_run_unconfined` boolean permit the management child process to enter
the supported unconfined management domain. This is not an application sandbox
test and must not be presented as one. Global host policy is not edited or
disabled; libvirt manages the dedicated bridge's own temporary firewall rules.

The provisioning adapter removes UV's interpreter directory only from native
tool subprocess PATH, preserving the trash-safe wrapper. libguestfs uses a readable
kernel extracted from the matching Ubuntu package and a home-filesystem cache.
It does not alter system Python, `/boot` permissions or global shell settings.

## Remaining Boundaries

- One physical host still shares CPU, disk and scheduling between the guests.
- No physical radio, access-point isolation, hardware codec or energy test.
- No native Windows/mobile private-storage validation.
- No production confinement audit or sustained-throughput benchmark.
- Discovery and files followed separate operator-approved VM gates; voice and UI remain future stages.
