# Explicit Room Copies

This unreleased addendum defines the room-aware availability layer on top of
consented, hash-verified file transfers. An existing copy is never silently
published to other room members. Sensitive text is not part of this layer.

## Publication And Lookup

```sh
# On a device that owns a verified copy:
kk room files team share PRIVATE_FILE --to RECIPIENT_CERTIFICATE_DIGEST

# On that intended recipient, using the known content digest:
kk room files team locate CONTENT_SHA256 --from COPY_HOLDER_CERTIFICATE_DIGEST
kk room files team fetch CONTENT_SHA256 --from COPY_HOLDER_CERTIFICATE_DIGEST

# On the copy holder:
kk room files team unshare CONTENT_SHA256
```

Each recipient runs its own foreground room service. Fetch requires its existing
explicit incoming-file consent, inbox limits and reservations. Locating a hint
does not grant storage consent. `--from` is mandatory: the client never broadcasts
a potentially sensitive content digest to every room member by default.

Publication hashes the selected regular local file and chooses an exact audience.
No directory-wide inventory, original filename or private source path is exposed
to other devices. The original sender is not implicitly an allowed recipient of
another holder's advertisement. Lookup returns an authorized metadata hint, not
proof of current or future bytes on disk.

## Authority And Lifetime

Offers belong to one foreground service and one private room membership epoch.
Restart or any epoch change requires explicit publication again. This avoids
resurrecting an old file authorization when a member is removed and rejoins with
the same long-term certificate. Voter changes do not themselves authorize files.

Every advertisement and transfer uses the exact room's pinned mutual-TLS peer
identity, current file permissions and fresh room authorization. Active outgoing
transfers recheck both the exact offer instance and its membership epoch per
chunk. Withdrawal or replacement of an offer retires that instance, including
withdraw-then-republish of the same digest. Already delivered bytes cannot be
recalled; no retroactive deletion is promised.

An offer does not survive its actor. Stored copies do: stopping or withdrawing
never deletes the inbox object. The bounded offer registry and transfer lanes
must not consume the control-plane admission permits while waiting for quorum.

## Verification And Failures

The holder reopens and rehashes the selected source before sending. The requester
requires the exact requested digest before reserving an inbox destination, then
uses the existing verified-prefix and final SHA-256 protocol. An unchanged full
copy can be reverified without rewriting it. Corrupt/changed sources, wrong
digests, unsafe paths, denied audiences and missing receiving consent cannot
publish a successful file receipt.

Receipts mean verified durable local storage from the selected holder, not human
reading, a replication factor or a permanent availability commitment. A holder
may stop, revoke permission or lose quorum. Partial lookup distinguishes successful
hints, unavailable offers and failed peers; it is not reported as a complete
observation when a selected peer fails.

The original sender need not remain online once another current member owns and
explicitly publishes a verified copy. No private MLS database, signer or identity
key is copied for that purpose. A usable controller majority is still required
for fresh authorization; this layer does not weaken the room's partition policy.

## Evidence Boundary

The public three-process CLI integration covers exact audience, no receiving
consent, verified first pull and resume, withdrawal, a changed source preserving
the requester's existing copy, service restart, membership-epoch retirement and
retrieval after the original process stops. A fresh cross-VM gate is running;
its recorded binary and result, not this source description, determine VM status.
Automatic replication and a guaranteed future replica count are not claimed.

Later verification (2026-10-26): the public CLI gate passes 25 scenarios across
three independent guests, including audience exclusion, withdrawal, epoch
retirement, first verified pull after original-VM shutdown, complete-prefix
reverification and refusal without replacing an existing copy. Evidence:
`lab/tmp/public-room-cli-943e5eff1a70.json`, binary SHA-256
`d3a99b5e1bc55727696c54c72747cb7a514d0605283c0bd4b6f62db8d5387bf6`.
Guests and isolated network stopped cleanly. Subsequent source adds a monotonic
registry epoch so a delayed old publication cannot erase newer offers, including
after withdrawal; its focused unit regression passes.

The browser now offers received inbox objects to exact checked peers, withdraws
offers, looks up a known content ID only on checked peers, and fetches/verifies
the selected advertised copy. HTTP integration verifies bytes and path refusal.
The configured-profile browser run also passes publication, lookup, verified
receipt and refusal after withdrawal while retaining its stored file. No browser
adapter gains arbitrary-path publication or bypasses receiving consent.
