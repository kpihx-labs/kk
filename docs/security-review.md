# Security Boundaries And Review

Engineering review and regression evidence, not an independent audit, formal proof
or promise of absolute perfection. Current qualification: [progress](progress.md).

## Threat Boundary

| Defended boundary | Not covered |
| --- | --- |
| Exact device/scope proofs and current permissions | Compromised endpoint or leaked approved invitation |
| Crash-fault controller quorum | Byzantine safety against a compromised controller majority |
| Private owned files and local IPC credentials | Compromised local account or forensic erasure |
| Private loopback browser session | Malicious browser extensions |
| Bounded native processes and framed media | A sandbox/certification of native parsers or physical devices |

## Invariants And Regressions

| Boundary | Enforced property | Evidence |
| --- | --- | --- |
| SQL/MLS | No output before durable crypto/checkpoint commit | Failed COMMIT, real process kill, ratchet/replay reopen tests |
| Recovery | Quarantined state retains the last acknowledged public view | Mutation failure plus unavailable reload regression |
| Roles/votes | Admission/data rights do not imply a vote | Reader/delegation, nonvoter injection and controller-retirement tests |
| Scope | Certificates, room/purpose and prior configuration bind changes | Wrong-scope/key, delayed pins and signed address-move tests |
| Resource lifetime | A closed owner cannot register late resources | Immediate rebind, terminate/join and owner-SIGKILL tests |
| Media shutdown | Stop completes an active frame before FIN | Saturated IPC and native contention tests |
| Copies | Exact audience, epoch and offer instance govern transfer | Withdrawal/epoch/creator-loss cases |
| Browser | One-use origin-scoped session; text stays text | Host/Origin, expiry, HTML literal and inbox-path tests |
| Archives | Bound decompression before TAR metadata parsing | PAX/gzip expansion, traversal, links and hash tests |

## Findings Closed In Code

| Finding | Correction |
| --- | --- |
| SR-001: late driver future retained a resource after closure | Seal/check/register under the owner lock; drop rejected future synchronously |
| SR-002: failed reload exposed uncommitted public membership | Restore the last durable view and keep crypto quarantined |
| SR-003: PAX parsing preceded member allocation limits | Bound compressed and decoded input before invoking TAR |
| SR-004: vulnerable/unmaintained dependency routes | Explicit source overrides, unchanged primitive guards, vectors and independent interop; no audit ignore |

Dependency details and maintenance obligations have one owner:
[dependency-overrides.md](dependency-overrides.md). Old failed attempts are retained
in Git/tool/lab history, not repeated as current blockers in guides.

## Remaining Qualification

Physical capture/portals, unsupported native OSes, real Wi-Fi/traversal and external
security review remain outside the current evidence. A bounded QUIC close-grace
diagnostic is not automatically a leak: verify joined resources and clean exit.
Likewise, packets queued are not playback, and signed provenance is not a security
assessment. Publication must use the gates in [distribution](distribution.md).
