# Local room history

History is off by default. Start the foreground room before issuing commands:

```sh
kk --profile ./alice room serve team
kk --profile ./alice room history team status
kk --profile ./alice room history team enable
kk --profile ./alice room send team 'remember this'
kk --profile ./alice room history team search remember --limit 1
```

Enrollment captures only future non-sensitive room text. It does not import old
messages, record files/media, promise delivery, or prove that a human read text.
Each profile opts in independently. UI parity is future work.

## Commands

| Command after `room history team` | Meaning |
| --- | --- |
| `enable` | Explicitly enroll a new key, or re-enable an enrolled existing key |
| `open` | Load only the already enrolled existing key |
| `disable` | Stop future capture; retain existing rows and the key |
| `status` | Cached public summary, not a current quorum or verified row count |
| `status --refresh` | Quorum-authorized public metadata refresh |
| `search [text]` | Bounded literal substring query; omitted text lists a page |
| `clear --confirm` | Delete retained history rows, not the key or room state |
| `recover --expected-recipient age1... --confirm` | Explicit orphan-key recovery |

`enable` and `recover` accept `--rows`, `--encrypted-bytes` and
`--retention-seconds`. Defaults and ceilings come from the backend policy; zero
and over-limit values are rejected. See `src/rooms/store/history.rs`.

## Paging and output

Replies retain the normal `{ok, code, result}` envelope. Search results contain
`rows`, `next_after`, `has_more` and `withheld`. Pass `next_after` as `--after-id`
for the next page, keeping the query unchanged:

```sh
kk --profile ./alice room history team search remember --after-id 12 --limit 1
```

`--response-bytes` bounds the serialized page, including escaped text. The
adapter reserves additional space for the bounded IPC envelope. A small budget
may be unable to fit a row. Withheld rows are not authorized for disclosure;
they do not imply missing network delivery. Send results additionally expose the
runtime's stable `reference`, without changing existing receipt fields. Incoming
live events do not yet expose that reference; display text is never used to
invent one.

## Keys, restart and recovery

The foreground owner configures only the verified room-private directory.
History commands accept neither filesystem paths nor private key bytes.
Never paste private keys into command arguments. The expected recovery recipient
is a **public** age recipient obtained independently from trusted enrollment
metadata.

On restart, enrolled history attempts to load its existing protected key.
Missing, unsafe or mismatched keys produce explicit failure/locked status and
are never silently regenerated. A history failure must not stop a default-off
room. Cached status never opens a private key file.

An orphan is a key published before enrollment committed. Ordinary `enable`
refuses to adopt it. Recovery requires an off store, the exact expected public
recipient and explicit `--confirm`; it cannot replace an enrolled recipient.
Preserve the original state when diagnosing a failure.

Search and key-backed operations run in a separate bounded history lane, not
the reserved cached-status/control lane. Authorization, current membership and
commit-before-exposure checks remain in the runtime/backend. The adapter does
not open SQL or maintain its own message index.
