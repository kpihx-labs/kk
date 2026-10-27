# Sessions And Delivery

Choose the smallest lifetime that fits the task:

| Surface | Lifetime | Trust context |
| --- | --- | --- |
| `send` / `receive` | One operation | Selected paired contact or exact certificate |
| `serve` | Foreground persistent inbox | Already paired contacts |
| `room serve` / joined room | Foreground room | Current room membership and policy |

```text
connect -> verify peer -> exchange bounded data -> close
               |                    |
               +---- refuse --------+---- timeout/cancel
```

Persistent owners reuse authenticated links and shared stream handlers. A broken
link is retired; the next explicit operation reauthenticates rather than silently
replaying a possibly committed payload. Discovery/address caches are only hints.

## Read The Receipt Correctly

| Receipt | Meaning |
| --- | --- |
| Direct/node text | Validated, bounded application acceptance |
| Room text | Durable replay processing and local event-queue acceptance |
| File | Verified durable destination publication |
| Media queue report | Queued datagrams, not playback |

None proves human reading. Multi-peer delivery can be partial; concurrent streams
do not imply a single display order or exactly-once replay across reconnects.
Sensitive messages do not acquire persistent chat history by default.

Use [nodes](node.md) for paired inboxes and [rooms](rooms.md) for dynamic admission,
delegation and control state. Guarantees are defined in the [contract](../CONTRACT.md).
