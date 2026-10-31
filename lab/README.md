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

For inter-version qualification, also supply `--previous-kit ABSOLUTE_DIRECTORY`
and `--expected-previous-binary-sha256 SHA256`. Both prepared kits must use distinct
versions and independent binary digests. The sequence is fresh previous, update
current, rollback previous, return current; every step verifies its installed
digest and reported version before continuing. Each version uses its own bundle
and roots. Omitting both options preserves the original three-operation gate.
Mocked coverage of this mode does not establish successful real VM execution.

Prepared kits use `attestation.jsonl`, not gh's Unix-only `sha256:DIGEST.jsonl`
download name, so the public inputs can be copied to FAT/exFAT USB media. The
download runs in a private local temporary directory (its retained path is printed
to stderr), never directly on that media; final verification uses the exported
bundle and roots. Set `TMPDIR` to a local Unix filesystem when preparing a kit
directly onto USB. Copy the whole kit, including `kit.json`, without renaming its
members. The VM runner checks recorded member sizes and hashes again before
uploading. This is portable data transport, not Windows/macOS installer support,
and does not make the kit metadata an independent signature or trust anchor.

Supply `--bootstrap ABSOLUTE_PATH_TO_REVIEWED_INSTALL_SH` to the VM gate to test
the actual shell bootstrap instead of invoking the trusted helper directly. Both
fresh install and rollback receive an explicit local helper, bundle and roots;
installation also receives its local archive and pinned commit. Isolation and
per-step binary checks remain unchanged. Reports distinguish the chosen
entrypoint and record hashes of the trusted helper, verifier and optional script;
these hashes identify test inputs, not signed/current-release qualification.

Kit preparation additionally accepts `--with-bootstrap` for canonical Linux/macOS binary
asset names. It authenticates matching raw helper, notices and `install.sh`,
exporting portable `ASSET.jsonl` sidecars in the same kit.
All four subjects use pinned workflow/tag/commit and explicit local bundle/roots verification. The resulting directory can be used
as the production shell installer's `--bootstrap-dir`; see
[signed local bootstrap](../docs/offline-install.md).
The `bootstrap` member of `kit.json` records all six additional files. Older kit
readers retain their original archive-only behavior; this optional preparation
does not claim execution or eliminate independent trust in the script/verifier/roots.

Generated state/reports belong to ignored `tmp/`; generated definitions to `xml/`.
Native private profiles/logs use a physical 0700 OS temporary workspace rather than
shared checkout ancestors; the retained report names it. [Candidate evidence](../docs/native-candidate-qualification.md).
Check scenario failures, process exits, binary hashes and `cleanup_errors` before
claiming a pass. Guests/network must stop and shaping must be restored. Failed
reports remain retained. Physical capture, radio and native non-Linux tests are
not inferred from these results.
