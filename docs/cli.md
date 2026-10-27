# CLI

Use `kk COMMAND --help` for every supported flag. These examples use one private
profile per device; add global `--profile PRIVATE_DIR` to select another.

Examples use placeholders: replace `LOCAL_IP`/`OTHER_IP` with each device's actual
LAN IP, and `PRIVATE_DIR` with an existing owned directory of mode 0700. Peer
digests are the full values reported by the room, not nicknames or IP addresses.

## Rooms

```text
A: create + serve -> invite -> compare CONFIRM <- join :B
       |                                          |
       +------------ encrypted room --------------+
```

On A, keep the room process open:

```sh
kk init
kk room create team --title "Our room" --bind LOCAL_IP:45462
kk room serve team
```

In a second terminal on A, export a single-use invitation into an owned private
directory. Deliver that file privately to B:

```sh
kk room invite team --bind LOCAL_IP:45463 --invite-out PRIVATE_DIR/invitation.json
```

On B:

```sh
kk init
kk room join team --invitation PRIVATE_DIR/invitation.json --bind OTHER_IP:45462
```

Compare CONFIRM and approve on both devices. Joining starts B's foreground room;
it does not create a global contact or grant a vote. Roles must match explicitly
at both ends; the default is `participant`.

| Action | Command |
| --- | --- |
| Broadcast text | `kk room send team "hello"` |
| Hidden input | `kk room send team - -s` |
| Inspect cached state | `kk room status team` |
| Set controllers | `kk room controllers team ADMIN_DIGEST_1 ADMIN_DIGEST_2 ADMIN_DIGEST_3` |
| Remove a data member | `kk room remove team MEMBER_DIGEST` |
| Stop / reopen | `kk room stop team` / `kk room serve team` |

Room peer arguments are full certificate digests. Retire votes explicitly before
taking controllers offline. Cached status is not a fresh quorum proof.

## Contacts And Direct Sends

Pair contacts using [pairing.md](pairing.md), then:

```sh
kk serve --bind LOCAL_IP:45454
# Another terminal, same profile:
kk send alice "hello"
kk send alice ./report.pdf -f
kk status
```

`serve` keeps a node open; `receive` handles one operation. Receiving a file needs
explicit consent and destination selection; see [files](files.md).
Expert operations can supply `--identity DIR --peer CERT` without bypassing known
profile restrictions.

Lifetime and receipt distinctions: [persistent inbox](node.md), [sessions](sessions.md).

## Input And Local Encryption

| Input | Meaning |
| --- | --- |
| `"text"` | Literal text, even if a file with that name exists |
| `PATH -f` | Explicit file |
| `-` | Bounded stdin |
| No payload with `-s` | Hidden local input; sensitive literal argv is refused |

```sh
kk keygen PRIVATE_DIR/recipient.key
kk send alice - -s --seal-to AGE_PUBLIC_RECIPIENT --envelope PRIVATE_DIR/value.age
kk unseal PRIVATE_DIR/value.age --key-file PRIVATE_DIR/recipient.key --output PRIVATE_DIR/value
```

`--seal` prompts for a passphrase instead. Retain the same randomized ciphertext
for a later `-f -s` resume. Sensitive mode protects local handling; ordinary
transport is already encrypted. There is no `--message` or `listen` command.

## Other Surfaces

| Need | Entry point |
| --- | --- |
| Browser workspace | `kk ui` |
| Untrusted discovery hints | `kk peers` |
| File digest without identity | `kk file-info PATH` |
| Room copies | `kk room files NAME --help` |
| Native media prerequisites | `kk media doctor` |
| Room media | `kk room media NAME --help` |

Delivery reports distinguish per-peer success, refusal and uncertainty. Do not
blindly retry an uncertain send. See the [contract](../CONTRACT.md).
