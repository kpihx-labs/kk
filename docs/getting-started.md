# From a first conversation to everyday use

**Start here:** [install and open your first room](../README.md#install-101), then [exchange a message and a reply](../README.md#first-shared-success-a-message-and-a-reply). All choices below are optional continuations of that working exchange.

Choose one interface for the first success: follow the README's browser path, or
use the [same room journey in the terminal](cli.md#rooms). Both use the same
profile and room state, so switching interface does not require pairing again.

```text
Need to exchange something -> Already have a room invitation?
  YES -> Join with an invitation -> Compare CONFIRM --+
  NO  -> Create a room -> Invite -> Compare CONFIRM --+
                                                    v
                                  Both approve -> Send message <-> Get reply
```

## Meet again: contacts or the same room?

```text
Need direct terminal sends to a regular contact?
  YES -> Pair devices -> Keep contact inbox open (kk serve) -> kk send NAME "hello"
  NO  -> Keep the existing room -> Start room -> Same shared conversation

Need another person in that room?
  YES -> Invite a member -> Single-use JSON -> Compare CONFIRM -> Both approve
  NO  -> Keep talking; no extra contact setup
```

Room membership does not create a global contact or grant an administrative vote. Follow [contact pairing](pairing.md) and the [contact inbox](node.md) for direct sends, or [rooms](rooms.md) and the [CLI room walkthrough](cli.md#rooms) for shared conversations.

## Send the file: consent, transfer, outcome

```text
Document to send -> Receiver: Consent to incoming files
                -> Sender: File -> Select document
                -> Room actions / File transfers
                     |
                     +-- completed + verified receipt -> Private inbox: stored
                     +-- interrupted -> Keep partial + original -> Explicit retry
                     +-- uncertain   -> Inspect / ask receiver -> Decide to retry
                     +-- unwanted    -> Request cancellation -> Inspect outcome
```

Progress is not a storage receipt; cancellation cannot recall bytes already delivered. See [file transfers](files.md) for recovery and [explicit room copies](replicas.md) when a holder should offer the received file to selected people.

## Find text later: keep history only if needed

```text
Need a local record of future room text?
  NO  -> Leave history OFF -> Live conversation only
  YES -> Room actions / Local history -> Enable future capture -> Confirm
                                           |
             Future non-sensitive text -> Stored records -> Search when needed
                                           |
             Disable capture -> No new records; existing records + key retained
```

History starts **off per profile and room**: no earlier-message import, sensitive text, files or media. A locked history is not an empty history; use the [history workspace guide](history-ui.md) or [history CLI](history-cli.md), and [Reply](conversation.md) when you only need to answer a particular message.

## Talk or show: choose a source, then stop it

```text
Text + file not enough?
  NO  -> Continue the conversation; no capture starts
  YES -> Room actions / Share media -> Choose source -> Browser/OS permission
         Other person: Accept media -> Choose peer + source kind -> Playback
                                           |
                                      Finished sharing?
                                        YES -> Stop my media -> Capture ends
                                        NO  -> Continue; add sources as needed
```

Start with one source and check playback together; headphones help avoid feedback. [Browser capture](browser-capture.md) explains device choices, while [media](media.md) covers prerequisites and the native path; browser/OS availability varies.

## Leave and return without starting over

```text
RUNNING BACKEND -> Start room -> ONLINE -> Exchange -> Stop -> ROOM OFFLINE
                                  ^                              |
                                  +---------- Start room --------+

Ctrl-C in terminal -> BACKEND STOPPED -> kk ui -> Select existing room -> Start
Close browser tab  -> BACKEND STILL RUNNING (not a shutdown)

Wrong profile? -> Profiles -> Select for next launch -> Restart UI
                            Existing room identities do not silently switch
```

Profiles own independent identities and local state; a display name is only a label. See [profiles and settings](profiles.md) before separating work and personal use, [notifications](notifications.md) when you want content-free local alerts, and [the workspace guide](ui.md) for controls.

## Only when access needs change

```text
Need someone to invite or administer?
  NO  -> Keep current roles
  YES -> Review data rights vs controller votes -> Explicit authorized change
```

[Room roles](rooms.md#rights-are-not-votes) explain that decision;
[administrative actions](moderation.md) show the safe sequence for controller
retirement and member removal. The [contract](../CONTRACT.md) defines delivery,
storage and revocation. Continue to [advanced security boundaries](security-review.md)
only for deeper trust assumptions, or the [documentation map](index.md) for the
next current page.

## Continue From Need, Not From A Manual

```text
Daily browser/CLI -> ui.md / cli.md
Identity + alerts -> profiles.md / notifications.md
Files + copies    -> files.md / replicas.md
History           -> history-ui.md / history-cli.md
Media             -> media.md / browser-capture.md
Install + trust   -> installers.md / offline-install.md / platforms.md
Contribute        -> backend-map.md / distribution.md / progress.md
```

The [complete documentation map](index.md) keeps advanced architecture and evidence
reachable without forcing them into the first conversation.
