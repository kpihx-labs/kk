# K&K Verification Lab

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

## Evidence And Limits

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
