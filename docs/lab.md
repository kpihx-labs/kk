# Verification Lab

Three independent Linux guests exchange actual encrypted traffic over an isolated
network. The hypervisor controls tests over virtio, outside the impaired data LAN.

```text
                  trusted host control
                /         |           \
          Fedora 44 --- Mint 22.3 --- Fedora peer
                \_____ isolated LAN _____/
```

| Test class | Proves | Does not prove |
| --- | --- | --- |
| `make lab-check` | Tooling, shell and mocked management behavior | Any real network operation |
| `make lab-test` | Cross-guest direct/node regression, faults and storage | Dynamic room behavior by itself |
| `make lab-room-cli-test` | Public room admission, roles/votes, copies and lifecycle | Physical capture or Byzantine safety |
| `make lab-native-media-test` | Actual owned-display capture, guest decode and bulk/control contention | Physical camera/mic, Wi-Fi or OS portals |

## Run Safely

```sh
make lab-check
make lab-room-cli-test
make lab-stop
```

For exact release replay, set `KK_TEST_BINARY` to an authenticated extracted
executable. Deployment records its digest; do not substitute another build's
digest as proof. Native prerequisites/options and provisioning controls are in
the [lab operator guide](../lab/README.md).

Only declared disposable guests/network are modified. Provisioning is not an
ordinary test rerun and must not target valuable disks or Windows. Guests have
distinct identities; private keys are not copied through management.

## Read The Result

Reports in ignored `lab/tmp/` retain binary hashes, scenarios, process results,
shaping and cleanup outcomes. A failed run is not passing evidence. Finish with
guests stopped, the dedicated network inactive and any temporary QoS restored.

One host shares scheduling/disk resources between guests. There is no physical
radio, hardware-codec, energy or production-confinement claim. Current results
and exact report paths: [verification record](progress.md).
