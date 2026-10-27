# Browser Workspace

```sh
kk ui
# Select a different private application profile:
kk --profile PRIVATE_DIR ui
```

The bundled interface and its backend stay in the foreground. No Node/Python
runtime or installed web service is required to use the binary.

```text
local browser -> authenticated loopback API -> same room/core owners as CLI
                                                   |
                                            encrypted remote peers
```

## Everyday Actions

| Action | Required choice |
| --- | --- |
| Initialize / join | Own identity; private invitation and two-sided CONFIRM |
| Send text | Room and content; sensitive text is hidden until reveal |
| Receive a file | Consent and inbox before starting the room |
| Send / offer / fetch a copy | Exact file or content ID and selected peers |
| Capture / play media | Local browser/OS permission and exact peer/source kind |
| Change controllers / remove member | Current administrative authority |

Admission is not a vote. Room identities do not become global contacts. Untrusted
message text is not rendered as HTML, and the workspace is not durable chat history.

## Private Launch

| Boundary | Behavior |
| --- | --- |
| Listener | Loopback only; remote devices use the room's reachable address |
| Bootstrap | Owner-private, single-use launch file; capability not printed in argv |
| Session | Tab-local origin/port-scoped bearer, not a cross-port localhost cookie |
| Requests | Exact Host/Origin checks and authenticated writes/WebSockets |
| Backend restart | Old credential retires; media stops; new private launch required |

Use `kk ui --help` for an explicit opener. Local account compromise and malicious
browser extensions are outside this boundary.

## Appearance

Neutral graphite and light-grey themes share one layout. App-local Dark Reader
opt-out prevents double recoloring without changing other pages' settings.
Errors stay visible in document flow; content scrolls independently of the composer.
Responsive layout checks are not physical-phone or permission-portal qualification.

Current delivery: [TODO.md](../TODO.md). Evidence: [progress.md](progress.md).
