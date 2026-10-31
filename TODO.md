# Delivery State

K&K is now the **1.0.0 final candidate**. The integration refactor and clean
project gate passed from a clean tree; the one-time version switch is complete.
Project, SDK and workflow gates pass on the current tree. Hosted native checks
and exact-byte release replay remain.

```text
1.0.0 final candidate
    |
    +-- user journey + compact archive + simple installer   DONE
    |
    +-- project + SDK + workflow gates                      DONE
    +-- native hosted checks                                PENDING
    |
    +-- exact archive install / update / rollback            PENDING
    |
    `-- signed 1.0.0 GitHub release                         PENDING
```

## Current Batch

| Area | State | Exit condition |
| --- | --- | --- |
| Product behavior | `make check`, `make sdk-check` and `make workflow-check` pass | Hosted jobs bind the same committed source |
| Version | `1.0.0` in Cargo metadata, lockfile and license | Final gates and tag resolve to these exact bytes |
| First use | Progressive README and getting-started path complete | Hosted archive opens directly to the same journey |
| Connected install | Simple pinned installer path implemented | Public `v1.0.0` URL works without `gh`; strict Sigstore path remains available |
| Portable archive | Six standalone files; notices deduplicated by digest | Archive manifest, notices and extracted first run pass |
| Documentation | 19 historical diaries removed; 37 current pages, zero orphans, links pass | Hosted links resolve on the final tag |
| Local hygiene | 187 GiB of build/cache debris purged | Final release artifacts retained; temporary qualification data removed |

## Release Gate

1. Project, SDK and workflow gates pass on the frozen 1.0.0 working tree.
2. Run Windows, macOS Intel/ARM and Linux hosted jobs on the exact commit.
3. Build signed 1.0.0 assets and replay fresh install, update and rollback.
4. Publish 1.0.0, verify the public one-line installer, then remove superseded
   prerelease presentation and local build debris.

The previous `v0.9.0-dev.2` release and its detailed qualification remain in
[Git history](https://github.com/kpihx-labs/kk/commits/main/) and the GitHub release
history. They are evidence, not a second active plan.
