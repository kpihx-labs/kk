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

## Shared Integration Contracts

Parallel module owners preserve these boundaries; the coordinator integrates
changes to the shared CLI, IPC command/event roots, configuration and Cargo graph.
An unavailable platform capability is explicit, not a successful empty operation.

| Interface | Stable integration rule |
| --- | --- |
| Profile/settings | Preserve `Identity`/`Peer` and explicit create/load, one protected identity and revisioned owner; never regenerate missing/unsafe state or resubmit an unrelated stale opt-in |
| Room commands | Thin CLI/HTTP adapters submit the same typed command; backend actors own authorization/transactions. Reject backend refusal before recording or announcing success; adapters cannot provide their own authority |
| Cached status | Local, bounded, noncreating observation; capabilities are advisory and never fresh quorum authorization |
| Messages/history | Exact authenticated room/author/epoch/wire references and typed relations belong to the MLS/transaction owner; no UI-authored identities, text-equality identity, magic JSON or direct database reads |
| Delivery | Preparation, committed membership/control, per-recipient queue acknowledgement, durable storage, playback, completed shutdown and human reading are distinct outcomes; partial results remain explicit |
| File/replica operations | Exact peer/content/audience binding, explicit live consent, cancellation barrier and no-replace publication remain at the backend owner |
| Transfer jobs | HTTP upload/forward opt into the existing foreground-owned start commands; GET list/status and POST cancel are guarded by the same session/origin policy. The UI polls only its exact owner ID, fences retired views and never equates source counters, admission or cancellation requests with a storage receipt. Closing a view does not cancel work; restart retires job IDs |
| Invitation clipboard | Copy/Paste happen only through explicit UI controls; stale operation/profile/dialog/input responses cannot copy or replace edits. Native clipboard permissions remain browser-owned. Clipboard data never enters diagnostics, and pending permission or pasted JSON is not pairing approval |
| Media | Source selection stays local; publication/subscription leases bind room, epoch, peer and kind; adapters cannot silently start or replace capture |
| Async UI/media | Bound concurrent work and invalidate retired room/dialog/permission generations; late callbacks cannot revive capture, consent or a closed workflow |
| Shared wiring | Coordinator owns Cargo metadata/lock, `lib.rs`, `config.rs`, shared CLI/session/network roots; preserve existing callers and request signature/DTO changes |
| Native implementations | Share the contract, not Unix path assumptions; missing protection/lifecycle support fails closed without replacing data. Distinguish skeleton, implementation and native execution; cross-build is not product acceptance |
| Packaging/install | Cargo version and actual artifact digest are authorities; compatibility, attestation, activation and rollback must concern the same bytes |

Cross-owner changes start with the requested signature or schema delta and its
failure/cancellation meaning. Owners retain their module knowledge across layers;
the coordinator reviews the changed boundary rather than rediscovering the tree.
