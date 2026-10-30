# Replies And Reactions

Source implementation and local qualification, not a signed-release claim.
The backend owns authentication, encoding, authorization and history; CLI and
browser adapters submit typed operations, never database writes or authors.

## Use

The browser exposes Reply and six reaction choices beside a live message.
Reply starts blank: it never copies the original body. Replying to a sensitive
message defaults to sensitive. A sensitive reaction has a separate dialog and
never contributes to public reaction counts. Names remain presentation only.

```sh
kk room reply study --to "$MESSAGE_ID" 'See you then.'
kk room react study --to "$MESSAGE_ID" like
kk room react study --to "$MESSAGE_ID" like --remove
```

Select the existing profile using the normal global profile option. Message IDs
come from authenticated CLI events or the browser's Message ID details.
An ID is not a read receipt, proof that a referenced message exists locally,
or permission to fetch remote history. Unknown/pruned originals stay unresolved.

## Protocol And Persistence

- `rooms::conversation` owns the explicit version/type discriminator and bounded
  codec under a distinct MLS authenticated context. Legacy text, including JSON
  looking text, retains its existing context and interpretation.
- Before preparing structured ciphertext, every current member must respond to
  an authenticated capability request for the exact room and epoch. Membership
  is synchronized and checked again; the backend checks the negotiated audience
  again at publication. Offline/old peers fail closed; there is no text fallback.
- Targets bind room, original MLS author, epoch and exact wire hash. Operation
  authors come only from authenticated MLS processing and pinned TLS identity.
  Cross-room and future-epoch targets are refused.
- Reactions use a finite enum. Add/remove affects only the operation author's
  entry. Projection is bounded, deduplicated and scoped to buffered nonsensitive
  activity; it is not a durable complete reaction ledger or convergence claim.
- Opt-in history records typed relations inside encrypted rows in the same
  transaction as MLS state. Ordinary rows retain their legacy format; relation
  rows have an explicit new version. Unknown versions fail closed. Older binaries
  cannot be promised to read new relation rows; no automatic downgrade migration.
- Sensitive operations stay out of persistent history/search and OS alerts.
  Live state hides their body/relation until explicit reveal. A public message
  reference still exposes opaque linkage, not the hidden body.
- Preparation, partial delivery and recipient event-queue acknowledgement are
  distinct from human reading. Do not blindly resend an uncertain operation.

## Evidence And Remaining Gates

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
complete mixed-version cancellation/restart UX and durable reaction restoration
remain separate gates. See [workstreams](workstreams.md).

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
