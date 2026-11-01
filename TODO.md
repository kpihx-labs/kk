# Delivery State

K&K **1.0.2 is delivered**. The immutable 1.0.1 assets remain historical evidence;
1.0.2 removes the permanent read-only publication banner without relaxing publication controls.

```text
1.0.2 stable release
    |
    +-- remove permanent read-only publication banner       DONE
    |
    +-- project + SDK + workflow gates                      DONE
    +-- native hosted checks                                DONE
    |
    +-- exact archive install / update / rollback            DONE
    |
    `-- signed 1.0.2 GitHub release                         DONE
```

## Current Batch

| Area | State | Exit condition |
| --- | --- | --- |
| Product behavior | Local project/SDK/workflow gates and hosted Linux/native matrices pass | Bound to commit `ff7b85284b52ce2a5dbf850c3796be3f610f9ef8` |
| Version | `1.0.2` in Cargo metadata, lockfile, license and immutable tag | Public stable release resolves to these exact bytes |
| First use | Progressive README and getting-started path complete | Six-file archive opens directly to the same journey |
| Connected install | Public versionless `curl` and `gh` latest paths install 1.0.2 | Installed SHA-256 matches the attested helper |
| Portable archive | Six standalone files; notices deduplicated by digest | TAR/ZIP checksums, manifests and Sigstore attestations pass |
| Documentation | 19 historical diaries removed; 37 current pages, zero orphans | Hosted links resolve on tag `v1.0.2` |
| Local hygiene | 187 GiB of build/cache debris purged | Final release assets retained; QA data remains isolated |

## Release Evidence

1. Local project, SDK and workflow gates pass on the frozen 1.0.2 tree.
2. Tagged Verify [41293799185](https://github.com/kpihx-labs/kk/actions/runs/41293799185)
   and Native [41293799229](https://github.com/kpihx-labs/kk/actions/runs/41293799229)
   pass on the exact commit without rerun.
3. Fresh install, `1.0.1 -> 1.0.2` update and rollback pass on attested assets.
4. Public stable [v1.0.2](https://github.com/kpihx-labs/kk/releases/tag/v1.0.2)
   exposes 16 signed assets; versionless `curl` and `gh` latest installs pass with helper
   SHA-256 `bcc6630a89741ad954a0c5639f65bf418fac8988101a2e5b973e8fddde07dba7`.

The previous `v0.9.0-dev.2` release and its detailed qualification remain in
[Git history](https://github.com/kpihx-labs/kk/commits/main/) and the GitHub release
history. They are evidence, not a second active plan.
