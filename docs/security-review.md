# Post-Integration Boundary Review

## SR-004: Dependency Audit Blocks Publication (2026-10-27)

`cargo-audit 0.22.2`, with advisory database revision
`3279c99911354c400e1d41e47c7d60e39258b147`, reports the following current product
lockfile findings. No audit ignore or warning suppression is added.

| Advisory | Locked Package | Required Action |
| --- | --- | --- |
| RUSTSEC-2026-0387 | crypto-bigint 0.5.5 | Patched release is >=0.7.6; current age, credential and OPAQUE/VOPRF routes need coordinated upstream updates |
| RUSTSEC-2026-0329 | libcrux-hmac-drbg 0.0.1 | Upgrade to >=0.0.2; affected nonzero output lengths are multiples of 65,536 |
| RUSTSEC-2026-0331 | libcrux-kem 0.0.9 | Upgrade to >=0.0.10; reject short hybrid-key encodings without panic |
| RUSTSEC-2026-0330 | libcrux-kem 0.0.9 | Upgrade to >=0.0.10; reject short hybrid encapsulation seeds without panic |
| RUSTSEC-2026-0374 | basic-toml 0.1.10 | Maintenance warning through i18n-config; evaluate upstream TOML replacement |
| RUSTSEC-2026-0173 | proc-macro-error2 2.0.1 | Maintenance warning through conditional hax tooling; remains visible in the full locked graph |

These are package-level findings, not a claim that all affected functions are
remotely reachable in the configured application. Conversely, the fixed
Ristretto/MLS configuration is not taken as an unproved exemption. Signing and
release publication now depend on a dedicated audit job with `--deny warnings`.
There is no release tag; all earlier functional/CI/provenance results remain
candidate evidence, not a waiver of these findings.

An isolated `target/dependency-probe` uses Cargo-packaged upstream snapshots:
OpenMLS `be248d4b1306d87a641ce24ed0ffcab2b27967f6`, rage
`b5b68c4f4f7ea5c368bc060f6fe0237549c3a13f` and cargo-i18n
`10e345470d97c9cffb3bc611182f595ad93251bf`. Its resolved graph removes the three
Libcrux findings and basic-toml warning, but still reports crypto-bigint 0.5.5
through OPAQUE/VOPRF and proc-macro-error2. Its first compile also exposes a
credential/traits snapshot mismatch in the synchronous/asynchronous storage
adapter. This probe is ignored experiment state, not a product dependency change.

## Release Archive Boundary (2026-10-27)

### SR-003: Extended TAR Metadata Preceded Member Resource Checks

The added release-tool review reproduces an earlier limit boundary than the
ordinary file checks: Python's TAR parser reads PAX metadata before yielding a
member. A 513-byte decoded fixture advertising a 16-MiB extended header made the
old verifier request a 16-MiB gzip read despite a test budget of 1 KiB. The
instrumented regression failed before the fix without allocating that large read.

Verification now refuses non-ordinary or oversized compressed inputs, streams an
optional trusted digest, and caps the complete decompressed input before TAR
metadata parsing. The existing ordinary-file, member-count, path, manifest and
hash checks remain in force. The PAX regression and a separate small gzip
expansion fixture pass. This is resource/integrity validation, not a substitute
for authenticating the CI attestation.

The exact downloaded CI executable
`116a7e4dd40e21aba7c063761be2e7a8fe102f6b2f6c2a0cb28a05022ecc135f`
passes the 25-case public CLI gate and native contention replay recorded in
`lab/tmp/native-media-11dd74d8aa97.json`: 1,300 native packets, fresh decoded pixels,
nine concurrent acknowledged texts with 1.5281-second maximum latency and the
same verified 1,094,160-byte bulk fixture. All cleanup reports are empty. Archive
selection and deployment guards now use the same explicit `KK_TEST_BINARY`.

## Current Closure Snapshot (2026-10-26)

The dedicated second pass is complete for the implemented software boundaries.
Both reproduced findings below are fixed with failing-before/passing-after
regressions. The latest exact `make check` passes 155 Rust tests and six JavaScript
tests, with warning-free format/Clippy/documentation checks. All nine explicitly
selected media tests pass, including the four dependency-gated tests ignored by
default. Thirty public-control adversarial tests and nine scope/delegation/voter
tests were also replayed after the focused code review. No confirmed blocking
finding remains open in this reviewed scope.

The private X server's keysym warnings were traced to its runtime X11 library,
not hidden or filtered. A compatible library extracted into the test's private
directory makes the isolated startup/exit probe return empty stdout/stderr.
`MEDIA_XVFB_LIBS` scopes that library path to the test X server only; the application's
native-child environment whitelist is unchanged. QUIC close-grace messages are
the explicit forced-drain path; the strengthened sealed-owner test proves late
tasks cannot retain resources after return.

The review-in-progress wording and pending table cells below are retained dated
working notes superseded by this snapshot. Remaining work is release/platform
qualification and final frozen-artifact replay, not a claim of independent audit,
Byzantine safety, physical-device qualification or mathematical perfection.

The operator explicitly required this second pass after functional integration,
even where ordinary tests already pass. It is an engineering/adversarial review,
not a formal proof, an external security audit or a promise of absolute perfection.
Final closure remains pending while the findings and targeted challenges below
are being completed.

## Threat And Evidence Boundary

The core authenticates device certificates, signed room authority, scoped PAKE
proofs and MLS membership. IP addresses, public packages and voter configuration
alone do not grant private data access. It is a crash-fault quorum design, not
Byzantine consensus: a compromised authorized controller majority, compromised
local account or malicious browser extension is outside the claimed boundary.

Tests use independently generated synthetic identities; no real long-term
operator key is exported or read for the review. Physical microphone/camera,
mobile keyboard and OS portal validation remain distinct from software fixtures.

## Critical Property Matrix

| Boundary | Invariant And Owner | Evidence And Extra Challenge | Current Outcome |
| --- | --- | --- | --- |
| Crypto and checkpoint | `Group::transactional` exposes output only after private SQL COMMIT; failed/uncertain state cannot continue ratchets | Real deferred COMMIT failure, process kill, restart, staged operations; added simultaneous mutation-error/recovery-quarantine challenge | Finding SR-002 fixed; final regression pending |
| Data rights and votes | Actor/group authorization owns data; signed explicit prior-configuration voter intent owns control membership | Reader/delegation tests, pinned-nonvoter vote/lead/snapshot refusal, explicit controller retirement and creator-VM loss | Existing evidence reviewed; targeted replay pending |
| Enrollment recovery | Candidate/activation evidence, private keys and applied position agree; missing state never silently regenerates | Actual recipient CLI SIGKILL after replicated admission, durable marker and lost-reply recovery | Targeted kill/recovery replay passes |
| Certificate and address scope | Exact proved device, room, purpose and prior configuration bind the operation | Cross-room/contact replay refusal, private-key possession, delayed transport-scope rollback and actual voter-address movement in three VMs | Existing scope checks reviewed; targeted replay pending |
| QUIC ownership | Closing seals the task owner; no late task can retain a released socket | Existing tracked driver termination plus new immediate-rebind late-spawn counterexample | Finding SR-001 fixed; targeted owner and CLI regressions pass |
| Media framing | Graceful stop finishes a framed IPC write before FIN | Constrained socket stopped after one frame byte; native capture/decode under shaped-link bulk traffic | Fixed during integration; targeted and native gates pass |
| Replica authority | Exact audience and membership epoch bind one offer instance; stale publications cannot replace newer policy | Excluded original sender, withdrawal, changed source, epoch/restart retirement, monotonic registry and original-VM-off verified pull | Public 25-case VM gate and later targeted/UI checks pass |
| Browser boundary | Private launch is one-use; bearer sessions stay origin/port-scoped; untrusted data remains text and inbox IDs | Exact Host/Origin tests, cross-port credential refusal, session retirement, literal HTML display and traversal refusal | Targeted HTTP/JS checks pass |
| Native child lifetime | Explicit local source only, bounded pipes/resources, no provider environment, owner-death termination | Native codec/private-display tests, actual owner SIGKILL and explicit X11 output diagnostics | Software gates pass; physical/platform limits remain |

## Closed Counterexamples

### SR-001: Driver Spawn After Owner Closure

The old runtime still created and asynchronously aborted a task when its owner
was already closing. After an earlier `released()` returned, a late future could
therefore retain a socket until the scheduler ran it. The new counterexample
failed an immediate bind of that exact address before the fix.

The closing check and registration now share the registry lock. A sealed owner
drops the incoming future before spawn returns and never registers a new task.
Both the new synchronous resource-release case and the earlier abort/join case
pass; the public room CLI/restart scenario also passes.

### SR-002: Failed Recovery Exposed Uncommitted Public Membership

The ordinary failed-COMMIT branch retained the last acknowledged public members,
but the mutation-error branch did not do so when verified reload also failed.
The group was correctly inactive/quarantined, yet its public member list could
reflect the rejected mutation. The added test performs a real staged MLS removal,
injects provider recovery quarantine and initially observed one member instead
of the last acknowledged two.

The failed-reload branch now restores the prior public members, committed epoch,
outgoing view and applied position before retaining quarantine. Private operations
remain refused. The test then drops the poisoned owner, verifies reopen without
key regeneration and completes the same staged operation. All six durable-group
tests plus actual process-kill and enrollment-recovery regressions pass.

## Output Review

The native shaped-link gate passed on binary SHA-256
`49ae0797608d1ab0ccb8a8441298185747a0ca7f72fb9e3789a97adbe0396a54`,
`lab/tmp/native-media-653e0518c99a.json`. It recorded 1,304 received native packets,
fresh decoded pixels while bulk transfer was active, nine concurrent acknowledged
text samples, maximum acknowledgement latency 1.5124 seconds at 125 KiB/s, and a
verified 1,094,160-byte file. Publisher finish and guest/network/QoS cleanup passed.

The report also contains XKB keysym warnings from the extracted test X server;
they are still being investigated, not discarded. Host Xwayland warnings were
removed by using an explicit free private display and the supported `-nokeymap`
option. QUIC close-grace diagnostics identify the deliberate bounded-drain path:
the code subsequently terminates and joins tracked drivers and waits for storage
release. They are not proof of a leak, but SR-001 showed why the post-close late
spawn boundary needed an additional regression.

Inbox listing errors are no longer silently presented as an empty inbox. The UI
retains its last view, exposes the failure and states that stored bytes are
unchanged. This does not turn the bounded inbox view into an unlimited archive.
