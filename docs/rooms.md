# Rooms

A room introduces authorized devices without making every participant a global
contact. Its creator is an identity anchor, not a mandatory data relay.

```text
approved room anchor
        |
bounded signed delegation + certificate-bound MLS package
        |
current authorized admission
        |
member ---- encrypted peer data ---- member
        \--- separate controller quorum ---/
```

## Rights Are Not Votes

| Item | Meaning |
| --- | --- |
| Room identity | Approved creator certificate, title and random nonce |
| Application role | `reader`, `participant`, `inviter` or `moderator` rights |
| Controller configuration | Explicit admitted administrative peers allowed to vote |
| MLS signer | Device-bound per-room signing material, not the device's private key |
| Local profile revocation | Additional local veto, even with a valid room proof |

An admission is not a vote. Removing a data member does not silently retire its
controller vote. Controller changes require the separate signed authorization path.

## Lifecycle

| Event | Result |
| --- | --- |
| Join | Candidate creates its own keys and activates the accepted Welcome |
| Restart | Reopen protected private/control state; never regenerate missing keys |
| Creator loss | Surviving links and delegated rights remain; quorum policy still applies |
| Permission/epoch change | Old data authorizations and copy/media leases retire |
| Unbridgeable history gap | Refuse and require verified rejoin |

A valid historical delegation is an authority ceiling, not proof of current
membership. Silent or partitioned peers are not automatically declared departed.
An uninformed partition cannot receive instantaneous revocation knowledge.

Create/join commands: [CLI](cli.md#rooms). Ordering and storage:
[room runtime](room-runtime.md). Threat boundaries: [contract](../CONTRACT.md).
