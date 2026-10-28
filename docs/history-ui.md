# Local encrypted history in the workspace

Start a room, then open **Room actions / Local history**. The bounded modal leaves
the chat and composer in place. Close it with its close button or Escape; native
dialog focus trapping and focus return apply. On small or short screens, controls
and records scroll inside the modal rather than expanding the workspace.

## Explicit actions

History is **off by default**, independently for every profile and room. Opening
the panel reads cached public status only. It does not enroll, migrate, open a key
or query stored messages. Loading the workspace does not enable history.

| Control | Effect |
| --- | --- |
| Enable future capture | Confirmation enrolls or re-enables local capture |
| Disable capture | Stops future capture, retaining records and the existing key |
| Refresh current status | Asks the actor for currently authorized public metadata |
| Unlock existing key | Opens only the already enrolled key; never regenerates it |
| Search stored records | Explicit bounded literal Unicode substring search |
| Next stored page | Keeps the submitted query and uses the returned cursor |
| Clear stored records | Separate confirmation deletes records, not keys or room state |
| Recover orphan key | Separate confirmation requires the exact expected public recipient |

Capture includes only **future non-sensitive room text**. No old-message import,
files or media are included. Sensitive live text remains hidden until explicitly
revealed in live chat and is never revealed from persisted history. The panel
never accepts a private key or filesystem path. Obtain a recovery recipient
independently from trusted enrollment metadata; it is a public `age1...` value.
Recovery cannot replace an enrolled recipient or repair a missing key.

## Reading records without inventing receipts

Search replaces the current bounded stored page, not an accumulating archive in
browser storage. Blank search lists a page. Search treats `%`, `_` and Unicode
literally, not as SQL wildcards. Editing the field does not change Next's query
until Search is submitted again. The page reports its actual cursor, withheld
count and whether more scanning remains. Withheld is an authorization result,
not evidence of failed network delivery. Empty successful pages and unavailable
or locked storage are different states.

The panel combines that stored page with the room's bounded live messages.
Only the full authenticated stable reference joins the two views: room, member,
epoch, event and exact-wire digest. Equal bodies remain distinct publications.
Legacy live events without a reference remain explicitly live-only; no reference
is reconstructed from display text, author names or a transient UI sequence.

- **Outgoing prepared / outcome uncertain** means local preparation committed.
- **Incoming processed** means authenticated MLS processing committed.
- Neither means delivery, human reading, or a "Sent" or "Read" receipt.

These are **local encrypted records**, not read receipts. A timeout or lost reply
may occur after an enable, disable or clear committed. Refresh status and inspect
the durable result before deciding to explicitly retry. Missing or mismatched
keys mean locked/unavailable storage, never an empty archive.

## Boundary and qualification

The authenticated same-origin `/api/action` adapter accepts a path-free `history`
action and a typed command. It dispatches through `local::request` to the bounded
actor history lane, with a bounded UI task admission limit. No HTTP handler opens
a database, exports key bytes, or implements authorization or a second index.
Search input and response bounds remain owned by the runtime/store. Session
retirement clears the panel, and closed/switched panels ignore late responses.
Queries and pages are not written to browser storage.

`src/ui/history_tests.rs` exercises the actual authenticated HTTP adapter, local
IPC and actor using two disposable profiles: enrollment, repeated-body distinct
references, incoming-live/stored reference equality, literal search, paging,
sensitive exclusion, restart, disable, clear and missing-key denial. It also
checks authorization/origin and malformed path-bearing command rejection.
An additional disposable orphan fixture proves exact public-recipient recovery,
refusal without confirmation, wrong-recipient refusal and existing-key unlock.
`tests/ui_history.mjs` separately checks client merging, explicit confirmations,
paging and stale/error handling with mocks. Neither is a configured-browser GUI
qualification; that remains a separate operator-owned gate.

Backend details: [CLI](history-cli.md), [runtime](history-runtime.md),
[key lifecycle](history-keys.md), [store](history-store.md).
