# Replies And Authenticated References

Source implementation and local qualification, not a signed-release claim. The backend owns authentication, encoding, authorization and history; CLI and browser adapters submit typed operations, never database writes or authors.

## Use

The browser exposes Reply beside a live message. Reply starts blank: it never copies the original body. Replying to a sensitive message defaults to sensitive. Names remain presentation only. Emoji reactions are no longer active product functionality; they cannot be newly published.

```sh
kk room reply study --to "$MESSAGE_ID" 'See you then.'
```

Select the existing profile using the normal global profile option. Message IDs come from authenticated CLI events or the browser's Message ID details.
An ID is not a read receipt, proof that a referenced message exists locally, or permission to fetch remote history. Unknown/pruned originals stay unresolved.
Opt-in encrypted history and literal search remain useful independently of replies; see [history CLI](history-cli.md) and [history UI](history-ui.md). Search does not fetch missing originals or turn an opaque reference into access authority.

## Protocol And Persistence

- `rooms::conversation` owns the explicit version/type discriminator and bounded codec under a distinct MLS authenticated context. Legacy text, including JSON looking text, retains its existing context and interpretation.
- Before preparing structured ciphertext, every current member must respond to an authenticated capability request for the exact room and epoch. Membership is synchronized and checked again; the backend checks the negotiated audience again at publication. Offline/old peers fail closed; there is no text fallback.
- Targets bind room, original MLS author, epoch and exact wire hash. Operation authors come only from authenticated MLS processing and pinned TLS identity. Cross-room and future-epoch targets are refused.
- Opt-in history records typed relations inside encrypted rows in the same transaction as MLS state. Ordinary rows retain their legacy format; relation rows have an explicit new version. Unknown versions fail closed. Older binaries cannot be promised to read new relation rows; no automatic downgrade migration.
- Sensitive operations stay out of persistent history/search and OS alerts. Live state hides their body/relation until explicit reveal. A public message reference still exposes opaque linkage, not the hidden body.
- Preparation, partial delivery and recipient event-queue acknowledgement are distinct from human reading. Do not blindly resend an uncertain operation.

## Legacy Compatibility And Pending Recovery

Reaction publication is rejected by both the Room boundary, before control/network work, and the Group boundary, before a ratchet/history transaction. CLI/local-IPC
origination is removed; browser controls/API are removed in the coordinated product
batch. A compatibility codec is not a publication API or an advertised feature.

Old encrypted reaction frames and stored typed relation rows remain decodable under
their existing authenticated context and version. Receiving compatible legacy
ciphertext retains its MLS author and typed history relation, subject to the existing
authorization, opt-in and sensitive-exclusion rules. It is never reinterpreted as
ordinary text. No stored rows are erased, no keys regenerated, and no wire/history
version or migration is silently changed. This is legacy data compatibility only,
not a promise to restore reaction controls, counts or a durable reaction ledger.

For membership changes, `Handle::prepared_control(deadline)` reads the exact local
durable proposal after cancellation or restart without requiring a control quorum.
This raw proposal is backend-only and must not be sent to a browser. Adapters use
`pending_mutation(deadline)` for the payload-free operation ID, kind, verified member
fingerprint and prior epoch,
then request explicit confirmation before `retry_prepared(expected_id, deadline)`.
The actor rejects an absent or changed pending ID before network work. Its presence
proves neither public acceptance nor Welcome delivery. The guarded action retries
through `submit_prepared`; it never replaces an uncertain operation with a new nonce,
rejoins a room or automatically resumes invitation/Welcome delivery. Restore
requires the cached pending Welcome to match the verified signed proposal; corrupted
or missing cached bytes fail closed rather than being repaired or regenerated.

## Evidence And Remaining Gates

The following records describe earlier qualification, including the reaction feature
before its removal. They are historical evidence, not current product requirements
or acceptance of the later removal/recovery batch.

On 2026-10-30 the working-tree implementation passed full `make check`: codec,
reaction ownership/capacity, group transaction/rollback/sensitive exclusion,
actual three-profile CLI reply/add/remove/cold-resume, two-profile HTTP and
browser-adapter tests. An additional real QUIC regression exercises capability
legacy/reset/malformed/timeout/oversized responses while preserving the cached
TLS connection and an independent stream. It is a transport fixture, not a
complete mixed-version product qualification.

The retained Alex/Morgan profiles ran in two foreground processes and existing
Edge tabs. Both came online without profile recreation. A reply displayed the
authenticated original author reference; a like increment and authored removal
appeared on both sides. Initial concurrent automation duplicated input text,
so those bodies were concatenated test strings, not a clean single-input replay.
A sensitive original stayed hidden; its Reply dialog was blank and defaulted
sensitive, and the received sensitive reply stayed hidden. No physical capture,
permission override, key inspection or profile deletion was used.

The replay exposed smooth scrolling leaving the newest actions under the
composer; instant scrolling made the target hit-testable. The source now disables
smooth message scrolling, with a source regression. Rebuilt-browser replay after
both processes restarted sent eight real messages through the authenticated UI
API, then submitted one clean reply through the visible dialog. Morgan displayed
nine rows and the exact reply/reference; both pages reported `auto` scrolling
and the newest Reply control passed a center-point hit test despite overflow.
Both authenticated shutdowns succeeded, shell-return notifications arrived, and
only the QA tabs were returned to blank; retained profile data was not removed.
Independent review returned no usable result;
it is not accepted as a passed review. GitHub delivery, exact released-byte QA,
complete mixed-version cancellation/restart UX and legacy-data compatibility remain
separate gates. Durable reaction restoration is no longer a product goal.
See [workstreams](workstreams.md).

GitHub Verify `41042779498` at `f6a559b` failed both Linux compilations on an
enrollment async-layout query-depth overflow, reproduced locally with incremental
compilation disabled. The shared application-operation boundary now returns one
caller-owned type-erased future, without detached work or a recursion-limit
increase. Nonincremental all-target Clippy and both enrollment tests pass locally;
the normal check now disables incremental compilation for Clippy. Hosted
acceptance of this correction remains pending, not implied by the earlier pass.

The correction's commit was blocked by the full pre-commit check: runtime case
`three_private_rooms_use_quorum_order_durable_nonce_and_local_veto_without_global_trust`
failed at `runtime/tests.rs:188` because the raw remote Welcome result was not
`Applied(Some(_))`. The actual alternative variant was not logged, so a leader
change or missing artifact is not established. No hook bypass or deadline change
was used. The future-layout correction remains staged and uncommitted; its
targeted compile/enrollment passes are not an integrated acceptance result.

Subsequent diagnostics retain each exact response category without dumping the
Welcome artifact. One complete concurrent library diagnostic execution passed
289 cases with the existing 10 explicit ignored cases and the HTTP workflow
reserved for its mandatory phase. That passing execution does not identify or
close the earlier intermittent Welcome failure. The compiler correction and
response diagnostics can be evaluated independently; no routing/assertion change
is claimed as its fix.

Correction `1712559` subsequently committed through a successful full hook and
was pushed to GitHub. Verify `41047460918` and Native `41047460933` both succeeded;
Native reported an Apple ARM runner-capacity annotation, not a test failure.
This supersedes the staged/uncommitted checkpoint above, not its failure record
or the unresolved Welcome cause. Later advisory moderation controls are a separate
working-tree slice and are not covered by those hosted results.
