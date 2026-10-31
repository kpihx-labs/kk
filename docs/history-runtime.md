# History Authorization And Outcomes

Runtime contract, not a latency guarantee. [CLI](history-cli.md) / [UI](history-ui.md).

```text
authenticated MLS -> capture + checkpoint COMMIT -> current authorization -> output
                               |                          |
                         durable fact               may withhold
```

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

## Store, Record And Key Ownership

`rooms::store::Provider` owns one connection, lease and transaction. History
enable/status/disable/clear/record/prune/search use a transaction from that same
provider. Commit before exposing reads as well as mutations; successful method
return is not durability. Commit failure retains the poisoned-handle/reopen rule.
Only explicit enable migrates V1 to V2; opening does not enroll or backfill.
Old V1-only binaries refuse V2. Disable retains records; clear retains key,
policy and monotonic IDs. Missing/wrong keys mean locked state, not empty history.

Each encrypted typed row binds expected profile, room, recipient, SQL ID, event,
epoch and exact-wire hash. Caller context supplies expected bindings, not copied
database fields. Bodies/previews/plaintext search indexes never enter SQLite;
policy, counters, clock, bindings, IDs and ciphertext lengths remain visible.
The stored clock floor rejects backwards observations; callers supply trustworthy
time. Search decrypts/validates the bounded retained set, even beyond the returned
page. Pruning between transactions means pagination is not a snapshot.

`envelope::record` uses ordinary binary AGE v1 with bounded X25519 recipient/header/
STREAM work, not custom cryptography. Opening returns zeroizing plaintext only
after full authentication and exact EOF; armor, excess/trailing data and foreign
stanza types are refused. Limits live in [config](../src/config.rs).
AGE integrity is not author authentication or database anti-rollback protection.
Owned plaintext buffers are wiped, but dependency-internal/caller copies, swap,
backups and physical storage are not promised erased.

`HistoryKeyAnchor` explicitly creates/loads `history.agekey` in the retained
room-private directory; SDK callers may supply a typed identity instead. Loading
does not prove SQL enrollment: verify the stored recipient/binding. Descriptor-
relative no-follow opens, owned private single-link files, no-replace staged
publication and an independent initialization lease preserve existing state.
The logical lease explicitly unlocks despite duplicated descriptors. Any existing
key or pending artifact blocks blind initialization. Sync/publication errors can
leave private pending or published bytes; never delete, regenerate or adopt them
implicitly. Orphan recovery requires the expected public recipient and explicit
confirmation. Key rotation is not implemented.

## Verification Scope

Store/codec/key/runtime tests cover binding substitution, corruption, migration,
rollback, concurrent initialization, preserved artifacts, queue admission, policy
veto races, quorum, paging and restart-stable references. These are bounded-work
tests, not preemptible crypto/filesystem or maximum-latency guarantees. Native
execution, physical power loss and local-account compromise remain distinct.
Current bindings: [verification record](progress.md).
