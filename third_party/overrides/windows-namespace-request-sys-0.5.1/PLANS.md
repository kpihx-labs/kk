# Plans: windows-namespace-request-sys

Design decisions are in [DESIGN-NOTES.md](DESIGN-NOTES.md).

This crate is planned in two files, and the split is deliberate. Its *creation*
lives in a feature-scoped checklist at the workspace root, because it lands
alongside a sibling crate and a set of workspace-level corrections whose lowest
common source-component is the workspace root; that file is deleted when its
feature completes. *Durable follow-up work* therefore cannot live there, and has
a local checklist instead. Neither is the workspace
[CHECKLIST.md](../../CHECKLIST.md), which holds unrelated deferred work.

The local [CHECKLIST.md](CHECKLIST.md) is a durable queue: its first plan, `NR-1`, is finished and
recorded in [COMPLETED-PLANS.md](COMPLETED-PLANS.md), and the rows below are for the work queued after
it, one row per plan.

| Path to CHECKLIST.md | Status | Brief description | Design Notes |
|---|---|---|---|
| [../../CHECKLIST-thread-ambient.md](../../CHECKLIST-thread-ambient.md) | in progress | **This crate's part (M24-M26) is complete**: the foundations (owned handle duplication, security attributes, path preparation, the faithful-execution contract), the four handle-producing entries, the five query entries, a test seam, and an acceptance pass over both operation and scenario coverage. The checklist itself stays open for M27 (`windows-platform-probes`) and the `M26+` items gated on this branch merging with `main`. | [DESIGN-NOTES.md](DESIGN-NOTES.md) |
