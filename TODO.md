# Delivery State

K&K is now the **1.0.1 corrective candidate**. The immutable 1.0.0 assets passed
all product, native and provenance gates, but its documented one-line fetch did
not follow GitHub's release redirect. The patch keeps product behavior unchanged
and repairs that public entry point with an explicit regression.

```text
1.0.1 corrective candidate
    |
    +-- user journey + compact archive + simple installer   DONE
    |
    +-- project + SDK + workflow gates                      DONE
    +-- native hosted checks                                PENDING
    |
    +-- exact archive install / update / rollback            PENDING
    |
    `-- signed 1.0.1 GitHub release                         PENDING
```

## Current Batch

| Area | State | Exit condition |
| --- | --- | --- |
| Product behavior | `make check`, `make sdk-check` and `make workflow-check` pass | Hosted jobs bind the same committed source |
| Version | `1.0.1` in Cargo metadata, lockfile and license | Final gates and tag resolve to these exact bytes |
| First use | Progressive README and getting-started path complete | Hosted archive opens directly to the same journey |
| Connected install | HTTPS redirect fix and regression implemented | Public `v1.0.1` URL installs without `gh`; strict Sigstore path remains available |
| Portable archive | Six standalone files; notices deduplicated by digest | Archive manifest, notices and extracted first run pass |
| Documentation | 19 historical diaries removed; 37 current pages, zero orphans, links pass | Hosted links resolve on the final tag |
| Local hygiene | 187 GiB of build/cache debris purged | Final release artifacts retained; temporary qualification data removed |

## Release Gate

1. Project, SDK and workflow gates pass on the frozen 1.0.1 working tree.
2. Run Windows, macOS Intel/ARM and Linux hosted jobs on the exact commit.
3. Build signed 1.0.1 assets and replay fresh install, update and rollback.
4. Publish 1.0.1, verify the public one-line installer, then remove superseded
   prerelease presentation and local build debris.

The previous `v0.9.0-dev.2` release and its detailed qualification remain in
[Git history](https://github.com/kpihx-labs/kk/commits/main/) and the GitHub release
history. They are evidence, not a second active plan.
