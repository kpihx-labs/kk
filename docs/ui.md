# Browser Workspace

```sh
kk ui
# Select a different private application profile:
kk --profile PRIVATE_DIR ui
# Or select an existing registered profile:
kk --profile-name work ui
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
| Profiles / Settings | Visible current profile; creation, registration and selection for a future launch |
| Local member nickname | Exact room/member binding; local presentation only, not verified remote identity |
| Send text | Room and content; sensitive text is hidden until reveal |
| Receive a file | Explicit consent, changeable while online; revoked transfers cannot publish later |
| Send / offer / fetch a copy | Exact file or content ID and selected peers |
| Capture / play media | Local browser/OS permission and exact peer/source kind |
| Change controllers | Current backend administrative authority, separate from data membership |

Admission is not a vote. Room identities do not become global contacts. Untrusted
message text is not rendered as HTML, and the workspace is not durable chat history.

After starting a room, use **Invite a member** and **Download invitation**. Share
that single-use JSON privately, then import it through **Join with an invitation**
on the other profile/device. A fingerprint is not an invitation. Both sides compare
CONFIRM before approving. Failed joining details can be edited without retyping
the local room/address/role; consumed or expired attempts need a fresh invitation.

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

Settings offer quantified hue, clarity and saturation rather than a binary toggle.
Neutral hue, clarity 50 and saturation 20 restore the original dark-grey midpoint.
Names come first; fingerprints and room IDs remain in technical details. See
[profiles](profiles.md) for CLI equivalents and local-label provenance.
App-local Dark Reader opt-out prevents recoloring without changing other pages.
Errors stay visible in document flow; content scrolls independently of the composer.
Responsive layout checks are not physical-phone or permission-portal qualification.

Current delivery: [TODO.md](../TODO.md). Evidence: [progress.md](progress.md).

## Sharing A Display Name

Settings includes **Share this display name with room members**, off by default.
Your own name remains visible locally when it is off. Remote names appear only
after explicit opt-in and a background metadata refresh. Member cards distinguish
self-announced names from local nicknames; neither verifies a human identity.
Fingerprints remain available, and every action still uses the exact member ID.

Turning sharing off cannot erase names already learned by another member and does
not clear their local nicknames. Settings updates change only submitted fields
atomically. Optional name-cache errors are shown separately; they do not stop the
room or make its basic status offline. The UI status path never waits for remote
name requests. See [profiles](profiles.md#explicit-name-sharing) for CLI controls.
