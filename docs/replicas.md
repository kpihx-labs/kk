# Explicit Room Copies

A received file is not automatically shared. A holder must publish an offer to
an exact audience; the requester must still consent to storage.

```text
holder: verified file -> selected audience offer
                                  |
requester: selected holder -> locate hint -> fetch -> verify -> local copy
```

```sh
# Holder:
kk room files team share PRIVATE_FILE --to RECIPIENT_DIGEST
# Intended recipient:
kk room files team locate CONTENT_SHA256 --from HOLDER_DIGEST
kk room files team fetch CONTENT_SHA256 --from HOLDER_DIGEST
# Holder:
kk room files team unshare CONTENT_SHA256
```

## Permission And Lifetime

| Event | Offer | Stored bytes |
| --- | --- | --- |
| Publish | Exact file/audience, bounded foreground registry | Retained by holder |
| Lookup | Authorized hint only, not proof of current storage | Unchanged |
| Fetch | Current file rights, exact offer and receiving consent required | Verified into requester's inbox |
| Withdraw/replace | Retires that offer, including active transfer authority | Not deleted or recalled |
| Restart/epoch change | Offer ends; explicit publication is required again | Not deleted |

`--from` prevents default room-wide disclosure of a potentially sensitive digest.
No remote source path or original filename becomes local path authority.
The original sender need not remain online once another current member publishes
a verified copy; fresh authorization still requires the room's quorum policy.

Lookup reports unavailable and failed peers separately. A successful fetch means
verified durable local content, not a guaranteed replication factor or permanent
availability. See [files](files.md), [rooms](rooms.md) and [evidence](progress.md).
