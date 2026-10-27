# Product Workstreams

Coordinator record for the operator-authorized product expansion. Actual code,
tests and delivery status remain in their owners; this is not a release claim.

| ID | Owner / peer | Files / boundary | Acceptance |
| --- | --- | --- | --- |
| W1 cleanup | Main, done | Managed installations only | Obsolete 0.8.0 trashed; active 0.8.2 and all profiles intact |
| W2 file consent | Main, working | Room receive policy, IPC, CLI/UI | Live opt-in/out; active-transfer revocation; actual backend state shown |
| W3 profiles | Main integration; profile executor | `src/profile/`, CLI/UI settings | Human names, explicit catalog, safe selection, no key regeneration |
| W4 UX workflows | UI executor | Invitation/join forms and feedback | Editable errors, retained non-secret drafts, clear private invitation actions |
| W5 appearance | UI executor | Profile settings and rendered theme | Quantified hue/clarity; default neutral grey exactly at midpoint |
| W6 media | Backend-test and UI-media executors | Shared media core and adapters | Concurrent participant screens, device controls, lifecycle/load tests |
| W7 platforms | Main, native foundation returned | Storage/IPC/process/capture ports | Actual native Windows/macOS/WSL execution and packaging |
| W8 distribution | Packaging executor, ZIP slice | Portable archives/installers | Verified tar/zip and reproducible installation across supported targets |
| W9 product review | Unassigned | Profiles/history/search/calls/accessibility | Review useful comparable-app patterns; define and test end-to-end cases |

## Peer Rules

- Only non-blocking `help_*`, with explicit OpenAI models. No native Task/Cursor.
- Prefer `openai/gpt-6.1-sol-fast` for bounded implementation and
  `openai/gpt-6.1-terra-fast` for independent review. Keep ownership disjoint.
- Before handing off, record peer ID, files, expected result and verification here.
- Main continues a separate task and reviews each result before integration.
- No delegated access to secrets, kernel issue creation or unrelated cleanup.

## Runtime Activation

Both fast models are configured canonically and executed read-only tasks successfully after restart.
Do not silently substitute Nvidia/Ollama or use a forbidden delegation tool.

| Peer | Current ownership | Verification / result |
| --- | --- | --- |
| `ses_e8190452bffe5Wg9qRmeiFT2Ei` (Terra fast, checker) | Read-only independent review | Wrong implementation assignment withdrawn, no edits; follow-up found publication/admission/archive races |
| `ses_e818fa5a8ffesTjZxwmtaA47DE` (Sol fast, explore, returned) | Read-only CLI/UI/media scout | Integration map and existing multi-peer semantics returned; no edits |
| `ses_e8188f7e6ffesHsfAjUS4Mv0cI` (Sol fast, explore) | Read-only member-name/history architecture map | Native platform blocker map returned; now maps sender-bound labels and opt-in messaging |
| `ses_e81846e76ffeT9lASkR7Gp0d8U` (Sol fast, executor) | Only `network/media/tests.rs`; profile ownership released | Concurrent peer-screen isolation/capacity regressions; profile concurrency/callback work returned |
| `ses_e8183ed38ffePpWpchnCiSVA4B` (Sol fast, executor) | Only `scripts/release.py`, `tests/test_release.py`, `tests/packaging.rs`, `docs/distribution.md` | Reproducible ZIP and bounded hostile-archive verification; preserve tar/install gates; report workflow integration |
| `ses_e8182b88affeHfPnXbftPqnFTa` (Terra fast, executor, returned) | `ui.rs`, `ui/{driver,tests}.rs`, assets app/index/style, `tests/ui_workflows.cjs` | Profile/settings/themes/editable join/issuance lifecycle; media files released |
| `ses_e817d843dfferuMi1u5xbyJk8L` (Sol fast, executor) | `files/{stream,storage}.rs`, `files.rs` caller, network file/replica, service tests | Publication/revocation barrier and regression; first five real-service consent tests returned |
| `ses_e81798465ffew0wctP5N67uJ5g` (Sol fast, executor) | Only new `.github/workflows/native.yml`, `tests/platform_foundation.rs`, `docs/platforms.md` | Nonpublishing native Windows/macOS foundation lanes; local test checks; no unexecuted native-support claim |
| `ses_e816cf899ffe3TUELK2k32c65c` (Sol fast, executor) | Only `src/ui/assets/media.js`, `src/ui/media.rs`, new `tests/ui_media.mjs` | Keyed multi-peer sessions, independent stop/cleanup, duplicate/capacity feedback; synthetic tests, no real capture |
| `ses_e81637a99ffeexoxNqI6OsapKf` (Sol fast, executor) | `profile.rs`, shared name validator in settings, new `profile/labels.rs` and tests | Private exact-room/member local aliases; bounded/concurrent/corrupt-state tests; no trust or remote-name claim |
| `ses_e815c09dcffen412a0MimzD28f` (Sol fast, executor, returned) | Only transport lifetime tests; production cleanup unchanged | Quinn drain exceeds grace; deterministic drain/socket-release regressions pass, no demonstrated leak |
| `ses_e8150c10bffe861gsFIxoGaFfo` (Sol fast, executor) | Only canonical `Proxies/desk-proxy` code/tests/docs | Repair confirmed AT-SPI-to-XID focus bug; stable window targeting and editable-install verification; no HITL weakening |

UI executor released both media implementation files before that handoff; its
remaining test ownership is `tests/ui_workflows.cjs`, not a test asset in the UI.
Profile executor completed locked callback and damaged-certificate distinction,
then moved exclusively to backend media tests. Its interrupted media patch and
the consent executor's interrupted publication fix were explicitly resumed after
the operator reported accidentally stopping peers; inspect partial edits first.
Only unfinished assignments were restarted, not completed tasks.
Completed UI/profile/media/packaging/foundation peers are archived; main owns returned files.

Second checker review found a final-authorize/publication race, ordinary IPC
admission starvation, and archive path replacement after trusted hashing. Consent
executor owns the publication barrier (`src/files/{stream,storage}.rs`, minimal
`src/files.rs` caller migration, network file/replica and service tests). Main owns
reserved control IPC admission (`session.rs`, `tests/room_cli.rs`). Packaging
executor returned immutable-snapshot parsing with 22 passing release tests.
These fixes require fresh independent review and integrated tests.

Platform foundation executor returned seven passing local Linux tests and a
nonpublishing Windows/macOS workflow. Native jobs have NOT yet run. Read-only
platform scout now maps sender-bound member names and opt-in conversation/history
integration; no implementation ownership. Backend media scout found existing
multi-peer support and explicit small capacity budgets, now being regression-tested.

Targeted and intermediate check evidence is in [progress](progress.md).
Settings updates use optional fields or the validated locked callback; CLI/UI
must never resubmit stale settings snapshots. Main integrated ZIP provenance,
non-creating profile/status and workflow-test discovery; final replay remains due.

Main browser QA uses the real `default` Edge profile and an owned test window
`144025616`, target `BA8764503AB102AE7EBBE060E136CD0C`; disposable data root
`/tmp/opencode/kk-product-20261027`, foreground UI in temporary tmux pane `%195`
of `kpihx-labs:15`. No operator profiles/capture used. Profile name is primary,
default background is exactly rgb(25,25,25), and no initial horizontal overflow
or console error was observed. Theme brightness only covered dark shades; UI
executor is correcting the complete dark-to-light range with contrast tests.
Close only this owned browser window and temporary pane after QA.
The disposable room title was a QA fixture, not product copy; use ordinary
examples such as Study group. Human-facing means profile names, understandable
actions and secondary technical details, never adding the word Human to labels.

Main owns live-consent fixes: withdrawal must bypass saturated work lanes, and
consent toggles must not replenish lifetime inbox budgets. Add regressions before
resuming CLI/UI integration. Independent review found both defects; they are not
yet fully verified fixed. Focused consent-budget and saturated-lane unit tests
pass; active-stream and full-product regression remain required. No new release
is authorized by these intermediate checks.

## Product Acceptance Map

- Human identity: current profile visible; names first, fingerprints in details;
  profile creation/selection/renaming/settings from CLI and UI without key loss.
- Room workflows: obvious single-use private invitation export; editable join
  errors and retained non-secret drafts/suggestions; no stored invitation secrets.
- Appearance: bounded quantized hue/clarity choices; neutral grey at clarity 50
  reproduces the current dark default; settings persisted per profile.
- Meeting UX: simultaneous participant screens, explicit subscriptions, device
  selection and stop controls; lifecycle, contention and real end-to-end tests.
- Messaging: [names/history design](messaging-design.md), replies/reactions,
  notifications, moderation and accessibility; no UI-only persistence shortcuts.
- Platforms: native Windows/macOS and WSL require storage, authenticated local
  IPC, foreground I/O, process cleanup and media implementations plus actual
  execution. Cross-builds alone never qualify support.
- Distribution: verified portable tar/zip, USB use, authenticated shell and
  PowerShell installation, safe update/rollback; runtime needs no build toolchain.
- Qualification: [two-profile/two-pane/two-tab UI gate](ui-qualification.md),
  error/cancel/retry/restart, permissions, accessibility and exact released bytes.

## Role And Integration Discipline

Verified configured profiles: `executor` performs edits/builds/tests; `explore`
maps code read-only; `pragmatic-checker` has edit/bash denied and reviews only.
Never assign implementation to a checker. Archive mistaken/finished peers after
recording results. Each executor has disjoint files; main reviews diffs and runs
integration gates before accepting work. No worker commits or publishes alone.
