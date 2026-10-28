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

## Evidence Boundaries

- This is Linux x86_64 on the current workstation, not Windows/macOS/WSL proof.
- The actual rollback case reselects the same version. Inter-version rollback,
  interrupted activation and hostile inputs have fixture tests, not this real-run proof.
- The development helper was not itself downloaded/authenticated as a bootstrap
  asset. Public raw-helper bootstrap remains a separate release gate.
- Archive ZIP parsing has regression evidence; this published real asset is TAR gzip.
- Offline verification/trust-root provisioning, physical power loss and new-version
  artifact replay remain separate gates. Existing runtime profiles were not opened.
