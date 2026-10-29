# Native key initialization qualification

Source-test evidence only, not release-byte, power-loss or native-product qualification.

## Hosted snapshot

| Binding | Observation |
| --- | --- |
| Source | `78a911c348bb51a74b28a52137d9b0f7868704cf` |
| Linux Verify | Workflow `40860132085` succeeds |
| Native workflow | `40860132122` fails ARM job `123400969301` |
| ARM failure | `empty_partial_and_complete_pending_block_every_initialization_retry`: 17 key tests passed, one failed at an opaque recovery-result assertion |
| Later ARM gates | Stdio and consent steps were skipped, not passed |
| Other native jobs | Selected Intel macOS and Windows foundation gates pass; this is not complete product support |

The assertion now reports only payload category, attempt, anchor index and the
public error code/message. Unexpected success is reported without formatting the
private identity. The required recovery result and all preservation checks remain.

## Separate logical-lease defect

A deterministic Linux regression retained a duplicated initialization descriptor.
While the logical owner lived, competing initialization correctly refused. After
the owner was dropped, the old implementation still refused because the alias
retained its flock. The regression failed before the corrective guard.

The guard now explicitly unlocks on operation return/unwind, before its file is
closed. The same regression passes, as do all 19 standalone key cases, all 19
library key cases and all-target Clippy. No pending-state, ownership, mode,
no-follow, no-replace or active-writer exclusion checks were relaxed.

## Remaining uncertainty

The original ARM refusal failure has not been reproduced locally. Before the
lease correction, all 18 cases also passed in controlled Linux runs with 128
descriptors, and with 256 descriptors under a deeper project temporary root.
Those observations do not establish the ARM cause. Fresh native execution and
its exact outcome remain required; no native-consent repair is inferred here.

## Hosted follow-up

On `a30ce7f`, Linux Verify `40873064834` and Native `40873064843` succeed.
All three selected native jobs pass, including both macOS key, stdio and consent
gates. This validates that snapshot, not a causal explanation of older failures.

GitLab `3003799544` separately fails browser job `17573579185`: its homelab shell
executor ignores `image:` and reports `make: command not found`. Rust and release
integrity jobs succeed. The project now selects the hosted container-runner tag
`saas-linux-small-amd64`, verified online on runner `54907241`; no global runner
settings or homelab packages were changed. A regression fails without the route
and passes with it, and GitLab's CI lint accepts the configuration. Execution of
the corrected jobs remains a distinct gate.
