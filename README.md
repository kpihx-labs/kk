# K&K

```text
Send someone a document -> Open a room -> Exchange text + files -> Talk or show
                          your devices, no mandatory cloud account or host
```

## Install 1.0.0

**Linux x86_64 / aarch64**, with `curl` and `sha256sum` available:

```sh
curl --proto '=https' --tlsv1.2 -fsS https://github.com/kpihx-labs/kk/releases/download/v1.0.0/install.sh | sh
"$HOME/.local/bin/kk" ui
```

Installs for your user without sudo, services or PATH changes; refuses to replace an existing `kk`. This simple path checks HTTPS release checksums, not Sigstore provenance; explicit version/commit verification is a separate [advanced installation path](https://github.com/kpihx-labs/kk/blob/main/docs/installers.md).

**Already have the binary archive?** Open a terminal in its extracted directory:

```sh
./kk ui
```

Use `kk ui` if it is on PATH. The browser interface is bundled; no Node, Python or separate web server is needed.

## Choose the path that feels natural
```text
Browser -> kk ui                  -> guided controls and visible room state
Terminal -> kk init / kk room ... -> the same protected core
```
Both use the same profile. Start visually or follow the [complete CLI path](https://github.com/kpihx-labs/kk/blob/main/docs/cli.md);
switching interface does not create another identity.

## First minute: a room running on your device

```text
kk ui -> Initialize device -> Create room -> local name + title
                                             |
                     alone -> loopback ------+
              another PC -> reachable IP:port
                                             v
                           Create and start -> ONLINE
```
A local room starts no file reception, history or media capture.

## First shared success: a message and a reply

Start on the same LAN, with K&K running on both devices. Replace these example IPs with your own; the browser address and `127.0.0.1` are local only, and `0.0.0.0` is not an address to share.

```text
CREATOR A                                   DEVICE B
Room on 192.168.1.10:45462                  Open K&K; initialize
Invite participant on :45463                |
Download invitation JSON ----------------> Import JSON; own address :45462
             Compare CONFIRM -> both approve
Send message ----------------------------> Reply
Read reply <------------------------------- FIRST SHARED SUCCESS
```

Joining fails? Check both reachable addresses, network access and the separate pairing port; replace an expired or consumed invitation, not your profile. A LAN address alone does not provide internet reachability.

### The same first room from the CLI

```text
A: kk init; kk room create team --title "Our room" --bind 192.168.1.10:45462
A: kk room serve team                    # keep this terminal open
A: kk room invite team --bind 192.168.1.10:45463 --invite-out PRIVATE_DIR/invitation.json
B: kk init; kk room join team --invitation PRIVATE_DIR/invitation.json --bind 192.168.1.11:45462
Both: compare CONFIRM, approve, then kk room send team "hello"
```
`PRIVATE_DIR` is an existing private directory. Keep `room serve` open while
using another terminal. The [CLI walkthrough](https://github.com/kpihx-labs/kk/blob/main/docs/cli.md#rooms) continues into status,
files, contacts, history and media.

## Send the document, then choose what comes next

```text
RECEIVER: Consent to incoming files -> SENDER: File -> Select document
                                                        |
Room actions / File transfers <-------------------------+
  completed + verified receipt -> Stored in receiver's Private inbox
  uncertain / interrupted      -> Inspect before explicitly retrying
```

## Grow only when the need appears
| Your next need | Continue with |
| --- | --- |
| Use the browser or terminal every day | [Browser workspace](https://github.com/kpihx-labs/kk/blob/main/docs/ui.md) / [CLI](https://github.com/kpihx-labs/kk/blob/main/docs/cli.md) |
| Identities, contacts or delivery | [Profiles](https://github.com/kpihx-labs/kk/blob/main/docs/profiles.md) / [pairing](https://github.com/kpihx-labs/kk/blob/main/docs/pairing.md) / [sessions](https://github.com/kpihx-labs/kk/blob/main/docs/sessions.md) |
| Add members, reply or change access | [Rooms](https://github.com/kpihx-labs/kk/blob/main/docs/rooms.md) / [replies](https://github.com/kpihx-labs/kk/blob/main/docs/conversation.md) / [roles](https://github.com/kpihx-labs/kk/blob/main/docs/moderation.md) |
| Files, copies or future text | [Transfers](https://github.com/kpihx-labs/kk/blob/main/docs/files.md) / [copies](https://github.com/kpihx-labs/kk/blob/main/docs/replicas.md) / [history](https://github.com/kpihx-labs/kk/blob/main/docs/history-ui.md) |
| Talk, show a window or choose a device | [Media](https://github.com/kpihx-labs/kk/blob/main/docs/media.md) / [browser capture](https://github.com/kpihx-labs/kk/blob/main/docs/browser-capture.md) |
| Install offline or verify provenance | [Installers](https://github.com/kpihx-labs/kk/blob/main/docs/installers.md) / [offline USB](https://github.com/kpihx-labs/kk/blob/main/docs/offline-install.md) |
| Understand guarantees or contribute | [Security boundaries](https://github.com/kpihx-labs/kk/blob/main/docs/security-review.md) / [full documentation map](https://github.com/kpihx-labs/kk/blob/main/docs/index.md) |

## Stop now, return later
```text
ONLINE -- Stop --> ROOM OFFLINE -- Start room --> ONLINE (same room + identity)
Ctrl-C -> backend stops -> kk ui -> select existing room -> Start
Close browser tab -> backend keeps running
```

Live messages are not an automatic archive, and delivery is not a human read receipt. This README works offline in a binary archive; web guides follow current source, so use the build manifest's source commit and `./kk COMMAND --help` for an older binary.

## Working from source
```sh
cargo fetch --locked
make build
./target/release/kk ui
```
Use `make help`; contributors continue with [distribution](https://github.com/kpihx-labs/kk/blob/main/docs/distribution.md).
Guarantees: [contract](CONTRACT.md). License: [BUSL-1.1](LICENSE), then AGPL-3.0-only
on its stated terms. Imported code keeps its [attribution](https://github.com/kpihx-labs/kk/blob/main/third_party/README.md).
