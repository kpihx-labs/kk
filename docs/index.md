# Documentation

Start with the task you want to finish. Each row reveals the next useful layer;
implementation and evidence stay available without blocking ordinary use.

```text
need -> daily action -> observed result
            |                 |
       concepts if stuck      +-> evidence, not assumptions
            |
       advanced install / contribution
```

## Use K&K

| Step | Read | What becomes possible |
| --- | --- | --- |
| 1. First exchange | [Getting started](getting-started.md) | Open a room, invite another device, send a message and a file |
| 2. Choose a surface | [Browser workspace](ui.md) or [CLI](cli.md) | Continue the same workflows visually or from terminals |
| 3. Shape your workspace | [Profiles](profiles.md) and [notifications](notifications.md) | Separate identities, labels, appearance, name sharing and local alerts |
| 4. Meet people | [Contact pairing](pairing.md), [contact inbox](node.md), [sessions](sessions.md) | Pair regular contacts, keep a foreground inbox and interpret outcomes |
| 5. Grow a room | [Rooms](rooms.md), [roles and administration](moderation.md), [replies](conversation.md) | Add members, separate rights from votes, reply with authenticated references |
| 6. Move documents | [File transfers](files.md) and [room copies](replicas.md) | Consent, resume, cancel, inspect receipts and offer verified copies |
| 7. Remember selectively | [History in the workspace](history-ui.md) or [history CLI](history-cli.md) | Opt into future local text, search it, disable or recover safely |
| 8. Talk or show | [Media](media.md) and [browser capture](browser-capture.md) | Choose a local source/device, publish, play and stop it explicitly |

## Install And Trust The Bytes

| Need | Read | Boundary |
| --- | --- | --- |
| Ordinary prebuilt install | [Installer paths](installers.md) | Connected install, authenticated helper and managed rollback |
| USB or disconnected machine | [Offline installation](offline-install.md) | Portable bundles and independently provisioned trust roots |
| What each OS really supports | [Platform scope](platforms.md) | Linux, Windows, macOS, WSL and mobile evidence without false equivalence |
| macOS implementation details | [macOS installer boundary](macos-install.md) | Mach-O, activation and remaining host evidence |
| Exact installation evidence | [Installation qualification](install-qualification.md) | Origin, content, update, rollback and offline proof |

## Understand What Happens Underneath

| Question | Read |
| --- | --- |
| Who owns each invariant? | [Backend owners](backend-map.md) and [workstream ownership](workstreams.md) |
| How does a room recover and commit? | [Room runtime](room-runtime.md) |
| Why is history not a read receipt? | [History runtime](history-runtime.md) |
| How are SQLite leases kept distinct? | [Database ownership](database-ownership.md) |
| How is a shared profile name bounded? | [Profile publication](profile-publication.md) |
| Why does macOS use a supervised worker? | [Supervised stdio](stdio.md) |
| Which threat boundaries are defended? | [Security review](security-review.md) |
| Why are dependencies locally overridden? | [Dependency overrides](dependency-overrides.md) |

## Build, Verify Or Contribute

| Goal | Read |
| --- | --- |
| Build and package a delivery | [Distribution](distribution.md) |
| Run isolated multi-guest scenarios | [Verification lab](lab.md) |
| Interpret native contention evidence | [Native candidate qualification](native-candidate-qualification.md) |
| See current source, host and artifact evidence | [Verification record](progress.md) |

The [contract](../CONTRACT.md) owns guarantees; [TODO.md](../TODO.md) owns delivery
status. Limits and version numbers belong to code and Cargo metadata, not another
documentation matrix. Every current page in `docs/` is reachable from this map;
Git preserves retired diaries without mixing them into the user journey.

## Historical Records

Earlier qualification diaries, superseded designs and peer handoffs remain in
[the pre-simplification Git tree](https://github.com/kpihx-labs/kk/tree/44f1a88c976eb44c7886df9233c980ce96ab6383/docs).
Locally, use `git log -- docs/PAGE.md` and `git show COMMIT:docs/PAGE.md`.
Old diaries are removed, not mixed into current guides. Two paths still referenced
by the unchanged lab README retain only current evidence requirements, not logs.
