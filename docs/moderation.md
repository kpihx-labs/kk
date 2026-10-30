# Moderation Qualification

Evidence for advisory action controls, not full moderation or release acceptance.
Usage and trust boundaries: [browser workspace](ui.md).

## Implemented Boundary

The private group supplies verified own-member rights. The runtime derives a
bounded action/issuance view; cached status exposes it without opening a second
database owner or awaiting remote name/quorum work. Missing, quarantined or
inactive membership produces no enabled capabilities. These are presentation
hints: every operation retains existing fresh backend authorization.

The browser disables unavailable publication, reply/reaction, invitation,
controller and removal controls. Invitation role choices respect delegation;
stale forms recheck the selected room and current capability snapshot. A voter
must be retired separately before removal. Controller selection explicitly
distinguishes the approver's administrative authority from the selected devices'
data grants. No capability field is accepted as permission from an HTTP caller.

## Automated Evidence

- Full `make check` passes, including nonincremental all-target Clippy, the
  mandatory two-profile HTTP phase and all 65 JavaScript cases.
- Backend role mapping covers reader, participant, inviter, moderator and missing
  state. Inviter issuance stops below delegated inviter/moderator rights.
- Real enrollment checks a reader's advisory view and actual publishing refusal;
  the owner retains its administrative view.
- Actual HTTP checks participant capabilities, refused removal and rejection of
  an injected capability field. JavaScript checks unknown/offline/nonboolean
  values, stale roles, changed rooms/members/votes and controller/data separation.

## Real Browser Slice / 2026-10-30

Executable SHA-256:
`1490c77548181c33303f18761f21755021e982fb7f049de7420cc690a0338fe2`.
Source: working tree above `1712559`, not signed distributed bytes.
The retained Alex/Morgan profiles, separate foreground processes and existing
owned Edge tabs were reused; no identity or room was recreated.

Alex's verified moderator view enabled publication, invitations, controllers and
removal. Visible clicks opened its controller dialog: two current members and
the explicit votes-versus-data explanation. The invitation dialog offered four
backend-approved roles. Both dialogs were cancelled without issuing invitations
or changing controller membership.

Morgan's participant view enabled publication/media sharing but disabled invite,
controller and removal actions. A message submitted through its actual composer
arrived in Alex's rendered message body. Morgan's initial browser serve call
exceeded the tool wait; it was not blindly resubmitted. A CLI status observation
confirmed the original owner was running, and an explicit UI refresh then showed
online state and the expected controls.

Both authenticated shutdowns returned `stopping: true`, shell-return notifications
arrived, and the QA tabs returned to blank. Existing profiles, keys, votes and
membership were preserved. No physical capture or approval override was used.

## Still Open

Real rendered reader/inviter controls, actual controller retirement/removal and
revocation feedback, full keyboard/accessibility/error recovery, fresh hosted
verification and exact released-byte qualification remain required. Cancelled
dialogs, cached capabilities and automated denial tests do not close those gates.

## Destructive HTTP Regression

The mandatory generated-profile HTTP workflow now exercises an actual two-voter
configuration, retires the participant's vote, confirms continued publication,
then removes its data membership and checks the new epoch. The removed device's
publishing request must fail, its cached access/capabilities become inactive, its
identity remains unchanged and previously received bytes remain intact.

This exposed a missing IPC refusal check in the UI's plain-text Send adapter:
denied publication was previously recorded as an outgoing message with HTTP 200.
The adapter now checks the backend result before recording an event. Partial
recipient delivery still succeeds as preparation, but the shared UI completion
predicate requires explicit all-recipient acknowledgement, never human reading.
The regression first failed on the refusal assertion and passes after correction.
Proposal digests are byte arrays; tests now match the actual backend response.
This is real authenticated HTTP/QUIC execution, not an Edge destructive-flow claim.
