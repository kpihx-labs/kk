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
| Foreground room readiness | Signal only after its bounded startup announcement succeeds; Darwin SDK hosts explicitly configure a compatible [stdio worker](stdio.md) when non-file output requires one |
| Exit | Cancel work, join owned resources and remove only the owned socket |

Run it in a terminal and stop with Ctrl-C. No service or automatic startup is
installed. A text receipt is not persistent history; an uncertain send is not
automatically replayed. See [sessions](sessions.md) and [files](files.md).

## Shared Local IPC Filesystem Contract

Contact nodes and foreground rooms use the same transport. Profile directories
must be private and owned by the effective UID. Ancestors must be owned by root
or that UID, without group/other write access except trusted sticky directories.
Root-owned system aliases are supported; user-controlled ancestor aliases are
refused. K&K does not automatically change workspace sharing permissions.

Short profiles retain their ordinary socket location for legacy clients. Long
profiles use a private, per-UID temporary namespace with a binding to the exact
canonical profile path, device and inode. A public locator hash is not authority.
The held ownership lock and socket identities are checked throughout the lifetime.
Interrupted initialization has bounded recovery; ambiguous data is preserved and
refused rather than deleted to force progress.
Crashes before a lock/socket receives its exact private mode can require explicit
inspection: wrong-mode runtime files are preserved and refused, not automatically
normalized. Do not remove locks or endpoints while another owner might be alive.

Linux addresses and chmod operations use pinned descriptors through
`/proc/self/fd`; unavailable support fails closed. Other Unix targets rely on the
trusted-ancestor policy and native no-follow operations and need native execution
before qualification. Root and the current UID are trusted filesystem principals:
this is not isolation from arbitrary same-UID malware, nor a promise to detect
every ancestor replacement-and-restoration race.
