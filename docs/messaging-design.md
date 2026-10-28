# Member Names And Conversation State

Implementation plan for the authorized product expansion, not a support claim.
Owners and progress: [workstreams](workstreams.md), [delivery state](../TODO.md).

## Names Without Changing Trust

```text
profile name -> authenticated self-announcement -> room member presentation
local alias ------------------------------------> presentation override
certificate / grants ---------------------------> action authority, unchanged
```

One backend resolver must serve member rows, message authors, recipient lists,
moderation and media selectors. Actions continue to submit full member IDs.
Resolution order: explicit local alias, accepted self-announced name, unnamed
member fallback. Fingerprints remain available in technical/security details.
Duplicate names never merge identities; replacement keys never inherit labels.

The first slice stores bounded, versioned local aliases keyed by profile, room
and exact member ID. These are not verified remote profile announcements, global
contacts, certificate changes or permission grants.

The announcement slice must bind labels to the authenticated sender, not a
payload-supplied fingerprint. Validate international names, control/bidi spoofing,
cardinality and aggregate bytes. Readers need a self-label operation independent
of chat publication rights; that must not grant them chat or reaction rights.

Refresh on activation, rename and membership change, with bounded/coalesced work.
Existing online members reannounce after a join because new members cannot read
old MLS epochs. Offline, never-observed labels remain explicitly unknown.
Retained labels are filtered against current membership without rewriting history.

## Typed Application Payloads And Compatibility

Current MLS application bytes are a sensitivity byte plus UTF-8 under the v1 text
context. Preserve that decoder; JSON-looking old messages remain ordinary text.
Names, replies and reactions need an explicit bounded version/type discriminator,
not magic text prefixes. Existing TLS-peer/MLS-author binding remains mandatory.

Old peers reject unfamiliar contexts. Authenticated capability signaling or an
explicit compatible-room upgrade is therefore required before sending structured
operations. A decoder alone does not establish mixed-version compatibility.
Never fall back by showing structured JSON as a chat message.

## Opt-In History And Search

The room backend owns recording/query policy; the private store owns persistence.
When retention is enabled, insert incoming history in the same transaction as
MLS processing/checkpointing, before an event or receipt escapes. Saving from the
UI observer afterwards leaves a crash gap and is not an acceptable substitute.

Use typed history rows and an explicit transactional schema migration from the
existing private-store version. Preserve keys/checkpoints on migration failure;
unknown versions fail closed. Document old-binary downgrade refusal honestly.

- History is off by default and enabling records future messages only.
- Sensitive bodies/previews stay out of persistent history/search by default.
- Disabling future recording and clearing existing history are separate actions.
- Retention, query size, result count, pagination and execution work are bounded.
- Parameterized local search never fetches remote history implicitly.
- Private SQLite permissions are not encryption at rest or forensic erasure.
- Nothing private enters public Raft logs, snapshots or release diagnostics.

## Stable References, Replies And Reactions

Use a room-scoped stable reference derived from exact authenticated ciphertext,
not UI process-local counters. Confirm identical references across all adapters.
Replies carry a target reference; optional preview text is not proof of authorship.
Reaction authors come from authenticated senders, with bounded values and
per-author add/remove semantics. Unknown/pruned targets stay bounded/unresolved;
cross-room references cannot become remote-fetch permission.

Outgoing preparation, partial delivery, uncertainty, peer acceptance and human
reading are distinct states. Preserve the existing per-recipient outcomes.

## Required Gates

Author substitution; reader restrictions; revoked members; duplicate/spoofed
names; late join/restart/rename; old-version interoperability; failed commit and
failed migration; full event queues; profile/room isolation; history-off and
sensitive exclusion; replay deduplication; reaction ownership; Unicode search,
pagination and injection strings. Browser tests must use ordinary names and
demonstrate the actual member presentation, not decorative wording.

## Implemented Announcement Boundary

The current name slice uses a private authenticated Profile metadata request,
not a new MLS chat payload. The typed-payload/history sections above remain future
design requirements, not implemented support. Each foreground serve/join binds its
explicit selected profile root to the control service before signaling readiness;
the backend verifies that root's public certificate against its own identity.

Sharing is default-off and read from the selected profile's atomic settings.
Local status obtains one complete room membership/epoch snapshot and supplies it
unchanged to the synchronous cache API. The cache owner handles bounded background
refresh, source binding and invalidation; adapters neither await remote names nor
invent their own name cache. JSON exposes `profile_names` and an independent
`profile_names_error`. CLI and UI call `present_with_announcements` for identical
local-override precedence. Remote metadata failures do not invalidate basic status.

`self_announcement` is provenance, not verification of a human. Equal or confusable
text cannot merge peer IDs or authorize an action. The own member row is always
local-profile presentation even while sharing is off. Disabling disclosure does
not erase prior knowledge, refresh-delayed cache entries or explicit local aliases.
