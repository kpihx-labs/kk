# Delivery State

K&K **1.0.1 is delivered**. The immutable 1.0.0 assets remain historical evidence;
1.0.1 keeps product behavior unchanged and repairs the public entry point with
latest-stable `curl` and `gh` paths plus an explicit redirect regression.

```text
1.0.1 stable release
    |
    +-- user journey + compact archive + simple installer   DONE
    |
    +-- project + SDK + workflow gates                      DONE
    +-- native hosted checks                                DONE
    |
    +-- exact archive install / update / rollback            DONE
    |
    `-- signed 1.0.1 GitHub release                         DONE
```

## Current Batch

| Area | State | Exit condition |
| --- | --- | --- |
| Product behavior | Local project/SDK/workflow gates and hosted Linux/native matrices pass | Bound to commit `00abf2299e603f04cc97756f5802941b23670a61` |
| Version | `1.0.1` in Cargo metadata, lockfile, license and immutable tag | Public stable release resolves to these exact bytes |
| First use | Progressive README and getting-started path complete | Six-file archive opens directly to the same journey |
| Connected install | Public versionless `curl` and `gh` latest paths install 1.0.1 | Installed SHA-256 matches the attested helper |
| Portable archive | Six standalone files; notices deduplicated by digest | TAR/ZIP checksums, manifests and Sigstore attestations pass |
| Documentation | 19 historical diaries removed; 37 current pages, zero orphans | Hosted links resolve on tag `v1.0.1` |
| Local hygiene | 187 GiB of build/cache debris purged | Final release assets retained; QA data remains isolated |

## Release Evidence

1. Local project, SDK and workflow gates pass on the frozen 1.0.1 tree.
2. Tagged Verify [41261320366](https://github.com/kpihx-labs/kk/actions/runs/41261320366)
   and Native [41261320374](https://github.com/kpihx-labs/kk/actions/runs/41261320374)
   pass on the exact commit; Native needed one unchanged retry of a transient macOS diagnostic.
3. Fresh install, `1.0.0 -> 1.0.1` update and rollback pass on attested assets.
4. Public stable [v1.0.1](https://github.com/kpihx-labs/kk/releases/tag/v1.0.1)
   exposes 16 signed assets; versionless `curl` and `gh` latest installs pass.

The previous `v0.9.0-dev.2` release and its detailed qualification remain in
[Git history](https://github.com/kpihx-labs/kk/commits/main/) and the GitHub release
history. They are evidence, not a second active plan.
