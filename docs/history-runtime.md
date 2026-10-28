# History Authorization And Outcomes

This is the runtime contract, not a universal cancellation or latency guarantee.
Storage: [history store](history-store.md). Commands: [history CLI](history-cli.md).

## Durable Facts Versus Disclosure

MLS processing, eligible capture and the private checkpoint commit together.
The runtime checks current policy, membership, epoch and quorum before processing
and again before releasing its result. Incoming history authors are also checked
before disclosure. Public names never confer authority.

| State | Meaning | Does not establish |
| --- | --- | --- |
| Outgoing prepared | Local encryption/capture committed | Network delivery or human reading |
| Incoming processed | Authenticated MLS processing/capture committed | Final runtime authorization or event delivery |
| Acknowledged | The named transport receipt was checked | Human reading |
| Withheld | Current runtime policy prevents disclosure | Deletion or rollback of already committed facts |

A local veto, epoch change, quorum loss, timeout or caller cancellation after the
private commit can prevent wire/plaintext/page/event release. It cannot recall
that commit. Encrypted prepared/processed history may remain; subsequent reads
must pass current authorization. Sensitive bodies and companion history rows are
excluded even when recording is enabled.

History enable/disable/clear can likewise commit before a caller loses its reply.
A cancellation or timeout is not proof that nothing changed. Refresh status and
verify the durable result before deciding whether another explicit action is
needed. Missing or incorrect keys are locked/unavailable state, not empty history.

Command cancellation after commit has an **outcome-uncertain** API contract, not
a rollback contract. This applies to text processing and history mutations alike;
no failed reply promises that private state was not mutated.

## Live References

Both authenticated actor accept paths emit
`Event::ReferencedText { text, reference }`. The reference comes from authenticated
decryption in the same operation, never from a lookup by body or displayed author.
`Event::Text(Text)` remains a legacy producer payload. The additional enum variant
requires exhaustive SDK consumers to update their matches in the 0.9 development
API; it does not change the existing Text payload's fields.

The local UI event adds `reference`, serialized directly as the canonical
`StableReference`: `room`, `member` and `wire_sha256` are arrays of 32 byte
integers, `epoch` is an unsigned integer, and `event` is `IncomingProcessed`.
The object equals the incoming history row's reference, including across private
state restart. Equal bodies from different authenticated wires remain distinct.
Sensitive live content remains memory-only; references do not permit hydration,
create a companion history row, or bypass private-session authentication.

## Scheduling And Bounds

The dedicated history queue bounds waiting requests; the actor executes one
selected operation at a time. Priority control admission wins between operations,
not during synchronous crypto or filesystem work. Cached status reads public
summary data without opening keys, but cannot promise scheduling on a blocked
single-thread executor. Input, retained rows/bytes, page size and work are bounded
by their typed policy and code limits, not by a universal wall-clock claim.

Search keeps the underlying cursor advancing over withheld rows and reports their
count. Every successful response must fit its complete serialized frame, including
references and runtime fields. A size refusal is not an empty result or permission
to skip records silently.

When the runtime framing exceeds the budget, search returns a shorter fitting
prefix where possible. Its cursor stops at the last processed row, its withheld
count describes only that prefix, and `has_more` preserves the deferred authorized
row for the next request. If no advancing prefix fits, the operation returns a
size error instead of an empty nonadvancing success.

## Qualification Gates

Runtime tests cover queue-before-decrypt, policy veto races, absent quorum,
authorization-filtered paging, sensitivity, restart-stable live references and
shorter-page behavior at the final escaped-JSON budget. A paused synchronous-work
regression establishes that a priority control request can time out while active
history work still owns the actor. Cached reads on another executor worker do not
prove responsiveness on a blocked single-thread executor. These are bounded-work
and ordering tests, not a maximum production latency measurement or a preemptible
filesystem/crypto guarantee. Key/file assumptions and upstream AGE internal-buffer limits
remain in their respective contracts; these tests do not prove physical power-loss
durability or protection against compromise of the local account.
