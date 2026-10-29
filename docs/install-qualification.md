# Prebuilt Installation Evidence

This records executed cases, not universal platform support. Usage and contract:
[installers](installers.md). No new release was published by this qualification.

## Real Published Linux Artifact

| Binding | Executed value |
| --- | --- |
| Release | Public, non-draft, non-prerelease `v0.8.2` |
| Source commit | `6a0a085b14d6536ebb4801f09d75eaf8a04f8500` |
| Asset | `kk-v0.8.2-linux-x86_64.tar.gz` |
| Helper | Development CLI built from the reviewed installer source |
| Verifier | Temporary Go-built GitHub CLI `v2.103.3`; system CLI unchanged |
| Prefix | `$HOME/.cache/kk-install-qualification-20261028` |
| Installed executable SHA-256 | `3aad41a3b5cf532ce927a01309d7a5d2730f0765899d316865ebc16f36db1cb7` |

The normal helper path downloaded the public asset, required real GitHub
attestation verification with repository/workflow/tag/source-commit restrictions,
validated the immutable archive and ELF requirements, then activated it. No mocked
`gh` or `curl` was used. The temporary verifier was selected only for these commands;
no global PATH, shell config, system GitHub CLI or active K&K installation changed.

| Operation | Observed result |
| --- | --- |
| Fresh online installation | Success; previous version null |
| Installed executable `--version` | `kk 0.8.2` |
| Identical reinstall | Success; previous `0.8.2`, bytes unchanged |
| Rollback to retained same version | Real reauthentication succeeds; previous `0.8.2` |
| Compare active stable executable | Both executable SHA-256 values match exactly |

## Isolated Unprivileged Linux Replay

On 2026-10-29, the development helper replayed the same public release in
`fedora-44` (project UUID checked), under UID 65534 (`nobody`). No candidate
executable was used as the installer. Inputs were prepared online separately;
all installation operations used local inputs with the guest NIC down, the live
hypervisor link disconnected and no other non-loopback guest interface.

| Binding | Executed value |
| --- | --- |
| Development helper SHA-256 | `3f162932fb2e8d0da3bc38d5312b1c04fc68ce0e545cf9d6209f20390fb0f3c6` |
| Real verifier | Go-built `gh v2.103.3`, private guest configuration |
| Archive SHA-256 | `c1bc1c9bfc26aed72f04f4528df65dcf498202dac634fc1d14de5735c39215dc` |
| Attestation JSONL SHA-256 | `fafb588de235ed659e6d18ea3d2c5b3e69f55401c0753fb122fe0823e66997b6` |
| Separately provisioned public roots SHA-256 | `65ca537f6ed8a47fd0e560c421baa1f6c1efb8b25fc200d8c5c02c0e92eb2b9c` |
| Guest fixture | `/tmp/kk-offline.gUfqUyNd`, fresh private prefix |
| Installed bytes/version | Same executable digest above; `kk 0.8.2` |
| Fresh / reinstall / same-version rollback | Each exits 0 with real verification and no stderr; previous null / `0.8.2` / `0.8.2` |
| Cleanup | Guest confirmed shut off; `kk-isolated` inactive, persistent, not autostarted |

The initial real run exposed gh's decoder-suffix requirement; a subsequent run
exposed inherited-pipe output reopening. Both were retained as failures, then
corrected with separate regressions before this successful replay. The guest
gate also keeps hypervisor isolation on timeout: an expired guest-agent wait is
not proof that its process tree ended. Preparation manifests intentionally retain
their original `not tested`/`not run` labels; they are not this execution report.

This proves successful verification with connectivity unavailable, not an absence
of attempted network system calls. The trusted development helper was built
locally from a worktree, not authenticated as a new published bootstrap asset.

## Evidence Boundaries

- This is Linux x86_64 on the current workstation, not Windows/macOS/WSL proof.
- The actual rollback case reselects the same version. Inter-version rollback,
  interrupted activation and hostile inputs have fixture tests, not this real-run proof.
- The development helper was not itself downloaded/authenticated as a bootstrap
  asset. Public raw-helper bootstrap remains a separate release gate.
- Archive ZIP parsing has regression evidence; this published real asset is TAR gzip.
- Offline verification/trust-root provisioning, physical power loss and new-version
  artifact replay remain separate gates. Existing runtime profiles were not opened.

The isolated replay above closes offline local-input execution for that exact
Linux artifact/helper combination only. The earlier workstation boundary remains
historical; cross-version rollback, physical power loss, new-version replay and
other platforms are still separate gates. Existing profile data was untouched.

## Unsigned Development CI Artifact, `a60aab6`

On 2026-10-29, Main downloaded artifact `12925205974` from successful Verify run
`40933771509`, not a release/tag. Its Linux x86_64 TAR and ZIP each pass the source
verifier over all 852 declared members. All eight per-target checksum entries pass,
including raw bootstrap executable/notices and both installer entrypoints.

| Subject | SHA-256 |
| --- | --- |
| TAR | `a4887c2461b4cc82598edf4b782033a7f231624ea2e9899a454b32a5029117f2` |
| ZIP | `04951e8684e217219fdce0d0523ca1e5fcc28443e2fc521bffdf312f25c09cc5` |
| Archived/raw bootstrap executable | `b24a478a435a8ad2ac7485b8456480b2ecf1c0be1974e91c61d3d2b8e2183db0` |

Retained input directory: `target/qualification-a60aab6`. This is internal integrity
and source-workflow retrieval evidence, not signed release provenance or a passing
installation. No new tag, public release or stable installation was created.
