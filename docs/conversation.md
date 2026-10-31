# Replies And Authenticated References

## Reply Without Copying Private Text

```text
loaded message -> Reply -> blank draft -> explicit send
       |                                      |
authenticated reference <-------------- typed relation
```

| Need | Action | Result |
| --- | --- | --- |
| Reply in the browser | Open Reply beside the live message | Blank draft; sensitive original defaults to sensitive |
| Reply in a terminal | `kk room reply study --to "$MESSAGE_ID" 'See you then.'` | Authenticated reference, not a copied original body |
| Find retained text | Opt into [history](history-ui.md) and search | Local authorized rows only; no remote fetch |

IDs come from authenticated events or Message ID details. An unknown/pruned
original stays unresolved; a reference is neither a read receipt nor access authority.

## One Backend Contract

```text
CLI / UI intent -> conversation codec -> capability + room/epoch checks
                                               |
                                     MLS/history transaction
                                               |
                                  explicit per-recipient outcome
```

| Boundary | Rule |
| --- | --- |
| Encoding | Explicit bounded version/type and distinct MLS context; JSON-looking legacy text stays text |
| Compatibility | Every current member must confirm exact-room/epoch capability; old/offline peers fail closed, no text fallback |
| Target | Room, original MLS author, epoch and exact wire hash; cross-room/future-epoch targets refused |
| Author | Authenticated MLS processing and pinned TLS identity, never a UI field or name |
| Persistence | Typed encrypted relation rows commit with MLS state; unknown versions fail closed, no downgrade migration |
| Sensitive content | No persistent history/search/OS alert; explicit reveal applies only to live memory |
| Delivery | Preparation, partial delivery, queue acknowledgement and human reading remain distinct |

## Legacy Data And Uncertain Membership Changes

Reaction origination is removed from Room/Group, CLI/IPC and browser surfaces.
Old encrypted reaction frames/rows remain decodable under their existing context
and authorization. Compatibility does not restore controls, counts or publication.
No keys, rows or versions are silently replaced.

```text
cancel / restart -> inspect pending metadata -> confirm exact operation ID
                                                   |
                                      guarded retry of SAME proposal
```

Adapters use payload-free `pending_mutation`, then `retry_prepared(expected_id)`.
The actor rejects missing/changed IDs before network work. Raw `prepared_control`
bytes stay backend-only. Pending state is not acceptance or Welcome delivery;
retry does not invent a nonce, rejoin or regenerate a missing cached Welcome.
Restore requires the exact cached proof to match the signed proposal.

Current evidence: [verification record](progress.md). Transaction/disclosure
semantics: [history runtime](history-runtime.md). Delivery: [sessions](sessions.md).
