# Unified CLI And Sensitive Data

> Normal transport is already encrypted. Sensitive mode protects local handling.

## Current 0.8 Additions

`kk ui` is a foreground loopback-only browser workspace with a private one-use
launch. `kk room files NAME share/locate/fetch/unshare` implements the exact-audience
copy protocol in [replicas.md](replicas.md); lookup is a hint and fetch still needs
receiving consent. `kk room media NAME publish/listen` uses explicit local source
and selected recipient/kind consent. `listen --inspect` is transport-only;
`listen --x11` selects actual X11 video output and conflicts with inspection/audio.
Native cancellation completes the active packet before finish. See
[ui.md](ui.md), [media.md](media.md) and [platforms.md](platforms.md). Older
implementation-in-progress descriptions below are retained checkpoint notes.

## Public Rooms

Current file extension (unreleased): explicitly consent on the receiving room
process with `--accept-files --inbox PRIVATE_DIR`, optionally `--max-bytes` and
`--file-budget`. Then send with
`kk room send team ./report.pdf -f --to FULL_RECIPIENT_DIGEST`. Files use direct
pinned TLS in this room scope and current publisher/file permissions; they are
not MLS text payloads and are never silently replicated to every member. `-s`
suppresses the local receipt digest, not bytes deliberately stored in the inbox.
Retry the same stable file to verify/resume its prefix. A successful receipt means
verified durable recipient storage, not automatic replica discovery or future
availability. Host integration passes; the new VM slice is being verified.
Earlier unimplemented-file wording below predates this extension.

Unreleased Unix commands use the same explicit profile identity and backend as
the library. They install no service. Initialize each device separately:

```sh
kk init
kk room create team --title "Our room" --bind LOCAL_IP:45462
kk room serve team
```

While that foreground process stays open, another terminal creates one invitation:

```sh
kk room invite team --bind LOCAL_IP:45463 --invite-out PRIVATE_DIR/invitation.json
```

Deliver that private, single-use capability to the intended device. On that device:

```sh
kk init
kk room join team --invitation PRIVATE_DIR/invitation.json --bind OTHER_IP:45462
```

Compare `CONFIRM` on both devices and type `yes` in each command terminal. Explicit
`--confirm` is available for deliberately approved unattended use; it is not a
default. The invitation never prints its secret capability on a pipe. Matching
`--role reader|participant|inviter|moderator` selections must be explicit at both
ends; the default is participant. Admission creates recipient-owned keys and
starts the joined foreground room, not a global contact or a voter.

```sh
kk room send team "hello everyone"
kk room send team - -s
kk room status team
kk room controllers team FULL_ADMIN_DIGEST_1 FULL_ADMIN_DIGEST_2 FULL_ADMIN_DIGEST_3
kk room remove team FULL_MEMBER_DIGEST
kk room stop team
kk room serve team
```

Text is positional and bounded; sensitive literal argv is refused. A broadcast
reports each recipient's durable-processing/event-queue acknowledgement or failure.
It never means human reading or all-or-nothing delivery. Partial delivery exits
unsuccessfully and retains the per-peer report; blind retries can create a new
message. Ordinary output escapes terminal controls. Sensitive incoming text is
redacted unless the serving command explicitly selected `--reveal`.

Status is a cached durable local view, not a fresh quorum proof. A controller
transition requires admitted administrative peers and completes the consensus
transition; data removal does not silently retire a controller's vote. Retire votes
explicitly before taking controllers offline if continued quorum is required.
`stop` acknowledges shutdown initiation; process exit and socket removal prove
completion. Ctrl-C on an enrollment waits for bounded transaction cleanup, because
cancellation cannot undo an admission already committed by another device.

Room names contain 1..40 lowercase letters, digits or hyphens. Bind addresses must
be concrete and have a nonzero port. Owned profile/room directories are 0700;
the manifest and credential-checked same-user IPC are private. Resume opens the
existing MLS/control stores and reconstructs transport pins from durable committed
state. Missing keys are never regenerated over room history. Interrupted creation
or enrollment retains its directory; explicit incomplete-enrollment recovery and
authenticated address changes remain unfinished, not an automatic reset path.
Room files/replicas and native media are not implemented by these text commands.

## One Send Surface

```sh
kk send IP:PORT "literal text" --identity ID --peer CERT
kk send IP:PORT ./report.pdf -f --identity ID --peer CERT
kk send IP:PORT - -s --identity ID --peer CERT
```

The payload is positional. A path is a file only with `-f`/`--file`; otherwise it
is literal text. `-` reads bounded stdin. With no payload, input is read locally.
`--message`, `listen` and the old `file send/receive` CLI are removed, not aliased.
Named contacts and default profiles remain a later step, not hidden inference.

## Receiving

```sh
kk receive --bind IP:PORT --identity ID --peer CERT
kk receive --bind IP:PORT -f --output ./inbox/report.pdf --accept --identity ID --peer CERT
```

The file inbox must be private (0700) and the output must not overwrite different
content. A sensitive wire marker prevents accidental terminal display even if
the receiver omitted `-s`. `--reveal` explicitly permits display; `--output PATH`
explicitly writes text into a new protected file. Nothing is opened or copied to
the clipboard automatically.

`-s` refuses literal text arguments because their exposure cannot be undone.
Use stdin or hidden terminal input. A cancellation pipe wakes the native input
worker and restores terminal echo rather than leaving runtime shutdown blocked.
Canonical terminal boundary lengths are refused; longer values use bounded stdin.

## Retain Ciphertext At The Destination

```sh
kk keygen ./private/recipient.key
kk send IP:PORT - -s --seal-to AGE_PUBLIC_RECIPIENT --envelope ./private/value.age --identity ID --peer CERT
kk unseal ./inbox/value.age --key-file ./private/recipient.key --output ./private/value
```

Key generation prints only the public age recipient. The key and output parents
must be owned private directories. The recipient receives a file and uses `-f`.
Encryption does not replace or relax pinned TLS authentication.

`--seal` instead prompts for a passphrase locally, twice. No password value is
accepted in argv. This uses age's standard scrypt mode, not ZIP encryption or a
custom cipher. Both recipient and passphrase modes are supported by the library.

The envelope is randomized and must be retained for resume. After interruption,
send that same ciphertext with `-f -s`; do not regenerate another ciphertext over
the earlier partial. Default artifacts live in the user's private kk data folder.
Encryption stages only ciphertext. Explicit decryption stages private plaintext,
publishes only after authentication, and never replaces an existing output.

## Wire Versions And Limits

| Surface | Current format |
| --- | --- |
| Text ALPN | `kk-text/2` |
| Text request | Sensitive flag (0/1), u32 big-endian length, UTF-8, FIN |
| File ALPN | `kk-file/2` |
| File offer | Sensitive flag (0/1), u64 big-endian length, SHA-256 (41 bytes) |
| File metadata | Offer + pinned certificate hash (73 bytes) |
| Receipts/resume | As in files.md; sensitive output omits content hashes |
| Text/stdin | At most 65,536 bytes; diagnostics never include content |
| Binary age header | At most 65,536 bytes before the standard parser |

0.4 wire/CLI compatibility is not implied. Older partial metadata is refused and
preserved, not silently rewritten. A local 0.4 binary backup is retained under
`/tmp/opencode/kk-0.4.0.bak` for the earlier lab artifacts.

Owned content buffers are zeroized where possible. This is not forensic erasure,
swap protection, immunity to compromised endpoints or automatic clipboard hygiene.
The age format is standardized; this composition is not independently audited.
