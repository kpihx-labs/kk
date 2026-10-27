# Backend Owners

```text
CLI / browser
      |
typed requests + bounded events
      |
profile / node / room owners
      |
authorization + durable crypto + ordered control
      |
shared authenticated transport and file/media handlers
```

| Owner | Responsibility |
| --- | --- |
| `src/cli/`, `src/ui/` | Input, presentation and explicit local consent; not trust decisions |
| `src/profile.rs`, `src/contacts.rs` | Device/trust context, grants, local revocation and protected history |
| `src/pairing/` | Scoped PAKE ceremony, confirmation, certificate/private-key proof |
| `src/node.rs`, `src/node/ipc.rs` | Foreground pre-paired links, same-user IPC and resource lanes |
| `src/rooms/local/` | Profile-scoped room creation/recovery and foreground lifecycle |
| `src/rooms/runtime/` | Thread-confined private actor, bounded commands, freshness and events |
| `src/rooms/group/`, `src/rooms/store.rs` | MLS effects, replay/nonce state and atomic private persistence |
| `src/rooms/control/raft/` | Signed public ordering, membership, controllers and recovery |
| `src/transport/` | Exact-peer TLS/QUIC policy and owned driver teardown |
| `src/files/`, room file/replica modules | Shared verified transfer plus scoped copy authority |
| `src/media/`, room media modules | Explicit sources/sinks, framing, consent leases and child ownership |
| `src/config.rs`, `src/error.rs` | Typed budgets and canonical failure codes |

## Invariants

- Fix the owner, not each adapter. CLI and UI share the same authorization path.
- Public replication never contains private MLS databases or identity keys.
- Durable crypto commits before corresponding output becomes visible.
- File/media work cannot turn into unbounded control-plane tasks.
- A stopped owner cannot leave late transport/capture resources alive.

The implemented surfaces are described in their guides. Delivery work belongs
only in [TODO.md](../TODO.md), and measured qualification in
[progress.md](progress.md). Earlier source inventories are Git history, not a
second current feature matrix.
