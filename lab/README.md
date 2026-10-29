# Lab Operator Guide

Test machinery, not an application dependency. Only declared disposable guests
and their dedicated network may be changed. Do not provision valuable disks or
modify Windows. Evidence scope: [verification lab](../docs/lab.md).

## Prerequisites

| Component | Purpose |
| --- | --- |
| QEMU/KVM, libvirt and guest agent | Isolated guests and trusted virtio management |
| libguestfs and signed OS assets | Explicit initial provisioning |
| UV, Ruff, ShellCheck | Script execution/static checks |
| GStreamer, host Xwayland, guest Xvfb | Optional owned-display native gate |

`xml/kk-isolated.xml` owns the guest/network roster. Each declared guest uses a
headless resource profile; the suite verifies identities, hashes and route isolation.
Fedora keeps SELinux enforcing with an explicit labelled management entrypoint.
This grants the trusted hypervisor authority; it is not an application sandbox.

## From The Repository Root

```sh
make lab-check                 # No VM mutation
make lab-test                  # Actual direct/node regression
make lab-room-cli-test         # Actual public room/copy lifecycle
make lab-stop                  # Graceful shutdown, retain evidence/disks
```

Initial preparation is a separate destructive-to-disposable-guests operation:

```sh
uv run --no-project lab/scripts/05-kk-prepare.py --help
```

Select an existing asset store with `--assets DIRECTORY` when needed. Normal
reruns do not repeat provisioning. Existing domain definitions/disks are retained;
host networking and unrelated guests are not reconfigured.

## Exact Artifact And Native Replay

```sh
KK_TEST_BINARY=/absolute/authenticated/kk make lab-room-cli-test
make lab-native-media-test MEDIA_RECEIVER=DECLARED_GUEST MEDIA_XVFB=GUEST_XVFB_PATH
make lab-stop
```

Without `KK_TEST_BINARY`, deployment uses `target/release/kk`. With it, deployment,
digest guards and host native publication use the same selected executable.
No other build's digest substitutes for qualification of these bytes.

The native gate captures an owned private host display and renders into an owned
guest display. It requires fresh pixels during bulk transfer and concurrent text
acknowledgements. Optional `MEDIA_XVFB_LIBS` affects only the test X server's private
library path. A live bandwidth cap is restored exactly; an existing conflicting
policy is not replaced. Host return-flow preparation is recorded, not presented
as automatic inbound traversal or a firewall-policy change.

## Results And Cleanup

Offline installer qualification has two distinct phases:

```sh
uv run --no-project lab/scripts/16-offline-kit.py --help
uv run --no-project lab/scripts/17-offline-install-vm.py --help
```

The first downloads public artifacts, attestations and separately provisioned
public roots with an explicitly selected real `gh`; it does not prove isolation.
The second requires an explicitly started, UUID-bound disposable guest, trusted
helper/verifier executables and an independently authenticated expected binary
digest. It tests fresh installation, reinstall and same-version rollback as
`nobody`, with guest NIC and live hypervisor link down. On failure or uncertain
process lifetime it keeps the virtual cable disconnected: confirm guest shutdown
before reconnecting. Always run graceful VM/network cleanup, including on error.
Executed scope and digests: [installation evidence](../docs/install-qualification.md).

Generated state/reports belong to ignored `tmp/`; generated definitions to `xml/`.
Check scenario failures, process exits, binary hashes and `cleanup_errors` before
claiming a pass. Guests/network must stop and shaping must be restored. Failed
reports remain retained. Physical capture, radio and native non-Linux tests are
not inferred from these results.
