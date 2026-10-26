# K&K Verification Lab

## Exact Archive Replay

Set `KK_TEST_BINARY` to the absolute path of an authenticated, extracted release
executable to replay that exact binary through deployment, public room scenarios
and the native media contention gate. Both deployment and host-side native media
use the same selection; the existing build is never overwritten. Guest SHA-256
verification and each result's binary digest still apply. Without this explicit
selection the lab continues to use `target/release/kk`.

> Test machinery, never a user runtime dependency.
> Provisioning is authorized only for explicitly disposable guests.

```text
 trusted hypervisor control (virtio, not the impaired LAN)
       |                 |                 |
 Fedora 44 ----------- Mint 22.3 -------- fresh Fedora peer
       \___________________________________/
                 isolated network
```

Each guest has 2 vCPU and 1536 MiB assigned. No GUI, default Internet route,
copied private identity or permanently installed host service is required.
SELinux remains enforcing on Fedora. A labelled guest-agent entrypoint grants
the trusted hypervisor explicit management authority; it is not an LLM agent.

## Commands From The Repository Root

```sh
make lab-check        # Ruff, ShellCheck and mocked control tests; no VM mutation
make lab-test         # Boot, build, deploy fresh fixtures, isolate and exercise
make lab-stop         # Graceful shutdown; retain disks and evidence
```

The focused persistent-node gate is `make -C lab kk-node-test`. It does not replace
the full regression. Historical `make -C ../vm-lab ...` commands forward here.

## Assets And Provisioning

The network roster has one source: `xml/kk-isolated.xml`. The scripts use those
declared guests only and verify network interfaces, installed binary hashes and
absence of IPv4/IPv6 default routes before testing.

System prerequisites: QEMU/KVM, libvirt, its Python CLI tools, libguestfs, UV,
Ruff and ShellCheck. Existing signed installer/cloud assets are not redistributed.
By default they remain in the sibling `vm-lab/` asset store. The preparation CLI's
`--assets DIRECTORY` selects another store; no global environment edit is needed.

```sh
uv run lab/scripts/05-kk-prepare.py --help
```

Do not run `fedora`, `mint`, `third` or `finish-*` against valuable guests.
These provision or customize the disposable machines. Powered-off domain
definitions are preserved under `/tmp/opencode/` before changes; original disks
are retained. Existing image guards and explicit `--resume` prevent blind reuse.
Ordinary test reruns use `make lab-test`, not provisioning.

Portability update: new backup/keyring directories use the operating system's
private temporary-directory API rather than requiring `/tmp/opencode` to exist.
Previously preserved backups remain at their original locations.

## Evidence And Limits

The optional native contention gate is separate from RTP-only interoperability:

```sh
make lab-native-media-test MEDIA_RECEIVER=DECLARED_GUEST MEDIA_XVFB=GUEST_XVFB_PATH
make lab-stop
```

It requires native GStreamer on the host/selected guest, host Xwayland in an
existing Wayland session, and a guest Xvfb executable. It captures only the owned
private host display, renders only into an owned guest display, and checks actual
decoded pixels during a verified bulk transfer and text acknowledgements. A live
libvirt inbound bandwidth cap is restored to its exact pre-test state. The harness
refuses to replace an existing bandwidth policy. This is not hardware camera/mic,
physical radio or automatic permission-portal evidence.

Keep guest Internet isolation intact during dependency preparation. A compatible
signed-distribution package may be transferred from the host and extracted under
the private test directory rather than installed system-wide; check its runtime
dependencies and startup before use. No host service or global shell setting is
created for this optional gate. Its generated logs and results remain in `tmp/`.

If an extracted X server needs a matching private X11 library, explicitly pass
`MEDIA_XVFB_LIBS=GUEST_PRIVATE_LIBRARY_DIRECTORY`. This affects only that test
server, never the product's native-child whitelist or the guest's system loader.
The harness records explicit outbound return-flow preparation for the host's
existing firewall policy; it does not modify firewall rules and does not qualify
automatic inbound traversal. Native decoder selection is explicit X11, and the
pixel probe must change from its pre-load image while the bulk transfer is active.

New results, guest definitions and state belong to ignored `lab/tmp/` and generated
`lab/xml/` files. Older results remain untouched in `../vm-lab/tmp/`; references
to them in the project history still identify the original files.

The suite records deployed hashes, distinct identities, OS/kernel/libc versions,
network shaping, process outcomes and application-port capture on the first
guest. It verifies all directed text links, real files and hashes, active-transfer
revocation, local IPC ownership, capacity limits, partitions and process crashes.

Synthetic fixture invitations may pass through virtio. Private keys never do.
Guest-exec status polling is the guest-agent completion API, not terminal polling.
All impairment is inside dedicated guests and removed in cleanup.

Three pre-paired nodes do not prove dynamic room admission, replicated state,
physical camera/microphone capture or real Wi-Fi behavior. Those need separate
evidence. Failed runs retain their reports and are never counted as passing gates.
