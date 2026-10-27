# Pairing And Contact Rights

Pairing proves an invitation holder and both device keys. A display name, address
or discovery announcement is not an identity proof.
Replace `LOCAL_IP` with A's actual LAN IP; `PRIVATE_DIR` means an existing owned
directory of mode 0700.

```sh
# A grants Bob selected incoming rights:
kk pair --listen LOCAL_IP:45453 --as bob --allow text,files
# B enters A's code through hidden input:
kk pair LOCAL_IP:45453 --as alice --allow text,files
```

Compare CONFIRM on both screens and approve on both sides. Each device owns its
private profile. Pairing initializes one when appropriate, but never regenerates
a missing identity over existing trust/history.

```text
single-use invitation -> OPAQUE + context confirmation
                                  |
                       encrypted certificates/grants
                                  |
                       strict TLS private-key proof
                                  |
                         local trust persistence
```

## Invitation Handling

| Choice | Effect |
| --- | --- |
| Code | Hidden input; expiring global attempt budget |
| `--invite-out PRIVATE_DIR/invite.json` | Explicit private disposable capability |
| `--qr-out PRIVATE_DIR/invite.png` / `--qr-image ...` | Bounded image export/import, not physical camera scanning |
| `--confirm` | Deliberate unattended approval; not the default or a key-proof bypass |

A failed, expired or consumed invitation needs replacement. A photograph/leak is
a live capability until then. No permanent private key is included. Two machines
cannot atomically commit their independent contact stores; an interrupted final
receipt may leave one legitimate local contact saved. Inspect, do not erase history.

## Rights After Pairing

```sh
kk trust list
kk trust allow bob --allow text
kk trust revoke bob
kk trust restore bob
```

`allow` replaces local incoming rights, not the peer's policy. Revocation follows
the fingerprint, including expert explicit-certificate operations. Restore is an
explicit action for the same key, never a silent identity replacement. Long file
transfers recheck authorization; already delivered bytes cannot be recalled.

Room invitations use the same scoped proof discipline but create room admission,
not global contacts or implicit voters. See [rooms](rooms.md),
[CLI](cli.md#rooms) and [dependency scope](dependency-overrides.md).
