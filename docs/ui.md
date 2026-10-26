# Foreground Device Workspace

The unreleased `kk ui` serves a bundled light/dark interface on loopback. Each
device runs its own backend and keeps its own private identity. The browser is
an adapter to the same room, permission, file and media owners as the CLI; it
does not become a room authority or a public web server.

```sh
kk ui
# Select an existing private K&K profile explicitly when needed:
kk --profile "$HOME/path-to-private-profile" ui
```

The command stays in the foreground. Stopping it drains its managed rooms and
media. The service binds loopback only; invite remote devices through a room's
explicit reachable address, not through the loopback UI URL.

## Private Launch

- The backend opens an owner-private, single-use local bootstrap file through the
  selected opener. Its capability is not printed or passed as a secret URL argument.
- The page consumes and removes the bootstrap fragment, then keeps its ephemeral
  bearer session in tab-local, origin/port-scoped session storage. No localhost
  authentication cookie is shared with other local ports.
- Host and Origin are checked; write requests and WebSocket upgrades require the
  expected Origin. WebSocket authentication uses its negotiated subprotocol.
- A backend restart invalidates the old session. The interface retires that
  credential, stops media and asks for a new private launch, without automatic trust.

Local account compromise and malicious browser extensions are outside this boundary.

## Workflows

Initialize a device once, create a room or import a single-use invitation, and
compare CONFIRM on both devices before approving. Admission does not grant a vote.
The interface also exposes explicit controller selection, member removal and
authenticated local address movement without changing identity keys.

Text is displayed as text, not HTML. Sensitive messages stay hidden until an
explicit reveal. Foreground memory is bounded and is not a persistent chat archive.
File reception needs explicit consent before starting the room. Sending and
forwarding choose an exact recipient; a verified stored copy is not a promise of
future replica availability.

Native browser microphone, camera and screen requests run from the user's local
action and still require browser/OS permission. A remote request cannot start
capture. Receiving selects an exact member and source kind. Stop, room switching,
session loss and page exit terminate owned tracks and buffers. The native CLI
remains available when the browser lacks the required codecs or capture APIs.

## Appearance And Evidence

The native themes use graphite and neutral light surfaces. `darkreader-lock`
prevents a second automatic recoloring of this app, without changing extension
settings for the operator's other pages. Pairing notices stay in document flow;
terminal errors can be dismissed without deleting backend history. Room content
scrolls independently of the composer.

On 2026-10-26 the operator's configured Edge profile and one dedicated owned window
passed independent-profile pairing, cold room resume, literal HTML text display,
consented file receipt and live browser VP8/Opus transport. Video decoded 588 frames
and reproduced the expected grey pixel; audio decoded 748 blocks with the test
playback muted. These streams used owned synthetic sources, not real hardware.

A script-free layout fixture rendered the actual bundled markup/CSS at 390 x 844
in both themes, including three retained pairing errors: no horizontal overflow,
composer visible and Send unobstructed. Native computed surfaces were 25/25/25
and 36/36/36 in dark mode, and 243/243/242 and 255/255/255 in light mode; the modal
backdrop was neutral black. This is responsive layout evidence, not a physical
phone, touch keyboard or OS permission-portal qualification.

See [progress](progress.md) and [pending work](../TODO.md) for remaining native
contention, replica availability, platform, adversarial review and release gates.

## Explicit Copy Availability Update

The subsequent copy layer is now integrated: `Offer copy` selects an exact checked
audience, `Withdraw offer` removes permission without deleting stored bytes, and
`Find copies` sends a known content digest only to selected peers. Results are
labelled advertisements, not guarantees; `Fetch and verify` still requires the
room's incoming-file consent and verifies the requested hash. Restart or membership
epoch change ends an offer. These browser/HTTP workflows pass on their recorded
slice; see [the replica contract](replicas.md) for authority and evidence boundaries.
