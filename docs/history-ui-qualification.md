# Interactive History Evidence

This records the configured Edge UI, not mocked DOM or native-mobile execution.
Contract: [history runtime](history-runtime.md). Usage: [history UI](history-ui.md).

Executable SHA-256:
`bd3616ba98f718534014cb34cc49c01d1de2514a1b65e5681ccfc1df44cf18f7`.
Disposable profiles Alex and Morgan used independent foreground processes and
their existing owned Edge tabs. No operator profile or active installation changed.

| Case | Executed observation |
| --- | --- |
| Initial state | Both Local history panels report off/not enrolled |
| Explicit enrollment | Enable opens its own confirmation; both profiles then report capture on/unlocked |
| Incoming reference | A live received message has the authenticated room/member/epoch/event/wire digest reference |
| Repeated text | Two equal bodies arrive as two live entries and two stored records, not one text-matched entry |
| Literal Unicode search | `[literal]` finds both `History GUI: café [literal] same body` records |
| Live/store merge | The archive shows exactly two stored matching records, not four duplicated live-plus-stored entries |
| Sensitive text | Received in hidden live memory; its synthetic body is absent from stored search/results |
| Room restart | Alex stops and resumes through the UI; blank search still returns the two stored records |
| Honest lifecycle | Records are labelled Incoming processed, not delivered/read receipts |
| Layout | Search confirmation remains inside the viewport while results are bounded/scrollable |

Reports under `/tmp/browser-proxy-autosave` include
`page-evaluate_20261028_144025_672600.json` (enrollment),
`page-evaluate_20261028_144100_590262.json` (authenticated live reference),
`page-evaluate_20261028_144306_148573.json` (two literal matches),
`page-evaluate_20261028_144433_579972.json` (sensitive hidden), and
`page-evaluate_20261028_144622_092304.json` (stored rows after room restart).

The early immediate read before a live update returned no message; the run waited
for actual state. Click reports at zero coordinates were not counted as successful
actions; menu state was checked before retrying. No false pass is inferred from them.

Whole-UI-process restart, real-GUI disable/clear/recovery, final history layouts
and cross-platform execution remain separate gates. HTTP/IPC fixtures cover more
paths but are not substituted for these outstanding interactive cases.
