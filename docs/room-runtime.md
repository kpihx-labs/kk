# Room Runtime

## State Flow

```text
authorized intent -> public ordered record
                            |
                     private actor applies
                            |
                  MLS + applied cursor COMMIT
                            |
                  output / Welcome / event
```

| State | Owner | Never substitutes for |
| --- | --- | --- |
| Signed public control | OpenRaft service and public store | Private MLS keys or Byzantine consensus |
| Private epochs/ratchets/replay | Device-local MLS/SQLite owner | A public membership snapshot |
| Cached status | Local actor snapshot | A fresh quorum authorization |
| UI/CLI state | Adapter | Current membership or permission |

## Admission And Recovery

1. Prove the invitation's exact room, purpose, device and granted role.
2. Order the authorized admission; a new learner is not implicitly a voter.
3. Retain recipient-encrypted Welcome for authenticated delivery.
4. Activate only with matching private candidate state and accepted record.

Interrupted enrollment distinguishes an unactivated candidate from damaged
member state. Verified recovery reuses existing keys; it does not reset trust.
Existing private members catch up through retained accepted records. A compacted
gap requires rejoin; copying another device's database is never recovery.

## Commit And Lifetime Rules

- MLS mutations and their local replay/applied checkpoints share a transaction.
- Failed commit/reload quarantines crypto and retains the last durable public view.
- Quorum work is bounded; cached status/cancellation do not wait behind it.
- Files/media have separate bounded lanes and fresh scoped authorization.
- Shutdown seals task registration, terminates/joins owned drivers and releases
  storage/sockets. A bounded close-grace diagnostic identifies that teardown path;
  process exit and resource release, not the diagnostic alone, establish completion.

The runtime uses crash-fault OpenRaft ordering, not Byzantine safety. Numeric
retention/work limits remain in code. See [owners](backend-map.md),
[security review](security-review.md) and [verification](progress.md).
