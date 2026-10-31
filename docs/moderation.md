# Roles And Administrative Actions

Room membership grants data rights; controller configuration grants votes.
Neither substitutes for the other. See [rooms](rooms.md#rights-are-not-votes).

In the [workspace](ui.md), use the selected room's invitation, controller or
removal action. Invitation role choices cannot exceed the issuer's delegation.
Before removing a voting member, explicitly retire its vote. Confirm the exact
member, not only its display name. Existing received bytes cannot be recalled.

## Advisory Controls, Authoritative Backend

The private group supplies verified own-member rights. Cached capabilities help
the UI disable unavailable actions; unknown, inactive or quarantined membership
enables none. Stale forms recheck room, member and capability state. No HTTP
caller can supply permissions, and every operation still needs fresh backend
authorization, epoch and local-policy checks.

A rejected send must not create a phantom outgoing success. Preparation, partial
recipient acknowledgement and human reading are distinct. After removal, the
device loses current publishing authority but retains its identity and already
received files. Controller retirement alone does not revoke data membership.

## Evidence Boundary

Role/delegation tests and real two-profile HTTP/QUIC cover participant vote
approval/retirement, membership removal, publishing refusal and preserved files.
The integrated graphical qualification also covers roles/revocation; final
bindings are in [the verification record](progress.md). Earlier cancelled-dialog
observations are not proof of a destructive operation. Keyboard accessibility,
every OS browser and external security review are not implied by these results.
