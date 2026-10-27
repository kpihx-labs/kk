# Persistent Contact Inbox

`serve` keeps exchanges open between already paired devices. It is distinct from
a room: it does not create dynamic membership, roles or votes.
Replace `LOCAL_IP` with the device's reachable LAN IP.

```sh
kk serve --bind LOCAL_IP:45454
# A second terminal using the same profile:
kk send alice "hello"
kk status
```

```text
send/status -> private same-user IPC -> foreground node -> pinned peer links
                                            |
                                   bounded events/inbox
```

## Files And Authorization

To receive files, the foreground node must explicitly choose:

```sh
kk serve --bind LOCAL_IP:45454 --accept-files --inbox PRIVATE_DIRECTORY
```

The receiver selects storage, grants and budgets. A remote alias or filename
never chooses a path. Permission changes are rechecked on live links and at file
checkpoints; restoration does not authorize a different identity.

## Ownership

| Boundary | Behavior |
| --- | --- |
| IPC | Private profile, effective-UID checks and one owner |
| Network | Exact contact certificates; address changes do not replace identity |
| Work | Separate bounded text/file/control lanes; excess work is refused |
| Output | Slow terminals cannot hold the protocol loop indefinitely |
| Exit | Cancel work, join owned resources and remove only the owned socket |

Run it in a terminal and stop with Ctrl-C. No service or automatic startup is
installed. A text receipt is not persistent history; an uncertain send is not
automatically replayed. See [sessions](sessions.md) and [files](files.md).
