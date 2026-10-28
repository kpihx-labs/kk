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

## Qualification Gates

Runtime tests cover queue-before-decrypt, policy veto races, absent quorum,
authorization-filtered paging, sensitivity and stable references. Active-work
latency and shorter-page behavior at the final escaped-JSON budget remain explicit
integration gates. Key/file assumptions and upstream AGE internal-buffer limits
remain in their respective contracts; these tests do not prove physical power-loss
durability or protection against compromise of the local account.
