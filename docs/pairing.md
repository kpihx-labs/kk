# Pairing And Local Authority

> Code possession authenticates an invitation holder, not a display name.
> Ordinary data always returns to strict pinned mutual TLS.

## Two Devices

```sh
# A: create one invitation for Bob and choose the rights granted to him.
kk pair --listen 192.168.1.10:45453 --as bob --allow text,files

# B: enter A's six-word code through hidden input, not an argument.
kk pair 192.168.1.10:45453 --as alice --allow text,files
```

The default private identity/profile is initialized if necessary. Compare the
same 12-hex-digit CONFIRM value on both screens and type yes on both sides.
The displayed comparison is 48 bits; the PAKE invitation is independently 66
random bits. No long-term private key is printed or included in an invitation.

After pairing:

```sh
kk receive --bind 192.168.1.10:45454 --from bob
kk send alice "hello"
kk send alice ./report.pdf -f
```

File receivers still need `-f --output PRIVATE_PATH --accept`. A paired identity
does not authorize arbitrary filesystem writes. The current inbox is one-shot;
persistent sessions and automatic authenticated address refresh are later work.

`--profile DIR` selects a separate private identity/trust context. Explicit
`--identity`/`--peer` certificates remain available for low-level operation, but
cannot bypass known grants or fingerprint revocation in the selected profile.

## Ceremony

```text
 A                                            B
 |-- public version + random invitation ID --->|
 |<--------------- OPAQUE -------------------->|
 |      one global attempt, monotonic expiry   |
 |                                            |
 |<-- encrypted certificates + chosen grants ->|
 |<------ both confirm the bound document ---->|
 |                                            |
 |<----- separate strict mutual TLS proof ---->|
 |       both private keys + TLS exporter      |
 |                                            |
 |-- persist locally + encrypted receipt ----->|
 |<------ persist locally + receipt -----------|
```

The TCP bootstrap contains only public PAKE handshake material before key
confirmation. It never sends the code as a password. Certificates/grants and
receipts are encrypted with directional HKDF-derived keys and monotonic AEAD
nonces. Roles, invitation ID and the exact OPAQUE transcript are bound; both
certificate/grant documents are confirmed and bound to the TLS exporter proof.

Only then does contact persistence run. No permissive TLS certificate verifier
is used. Registry, UI and approval callbacks do not get private PAKE session keys.

## Code, QR And Automation

- Default invitation lifetime: 120 seconds; maximum 300, measured by the server's
  monotonic clock from invitation creation, not from a client's claimed timestamp.
- One accepted connection/attempt consumes the invitation. Malformed, wrong and
  racing requests do not receive a new budget by changing addresses.
- This permits denial of service by burning an invitation; it is not LAN DoS protection.
- Use a new invitation after failure. A stale exported code cannot restart one.

For an explicit private out-of-band bundle or QR image:

```sh
kk pair --listen 192.168.1.10:45453 --as bob --invite-out ./private/invite.json --qr-out ./private/invite.png
kk pair --invitation ./private/invite.json --as alice
kk pair --qr-image ./private/invite.png --as alice
```

These outputs contain the disposable invitation secret. Their parents must be
owned private directories and their files are mode 0600. QR data is decoded with
file/pixel/allocation limits. It also pins the expected server fingerprint and
invitation ID before persistence. The current QR path is image export/import,
not a claim of camera scanning on every OS.

`--confirm` pre-authorizes the code holder and selected grants for controlled
automation. It bypasses the human comparison, not the cryptographic key proofs.
Do not use it with an invitation delivered through an untrusted channel. A leaked
or photographed invitation is a live capability until consumed or expired.

Optional mDNS advertising uses `_kk-pair._tcp.local.`. Its metadata stays UNVERIFIED.
It never imports a certificate, creates an alias or replaces the PAKE/confirmation.

## Permissions And Revocation

```sh
kk trust list
kk trust allow bob --allow text
kk trust revoke bob
kk trust restore bob
```

`allow` replaces local incoming rights, not the other device's policy. Upgrading
the peer's remembered outgoing rights currently requires explicit re-pairing;
live policy synchronization belongs to persistent sessions. Restriction/revocation
is checked at receiver admission and before effects, with file checkpoints at
most 64 KiB apart. In-flight bytes cannot be retroactively erased.

An alias never silently changes identity. Revocation follows the fingerprint,
including old explicit-certificate commands. Restore is an explicit local action
for the same key, not automatic re-admission. Prior registry states are archived
privately before atomic updates. Deleting the current registry while history
exists fails closed rather than resetting trust.

## Evidence And Boundaries

Local real TCP/QUIC tests cover successful key proof, wrong code/ID, rejected
consent, QR roundtrip and storage failure. Cross-device VM evidence is recorded
in lab.md only after execution. Exported test invitations are synthetic and their
codes are not retained in result logs.

OPAQUE uses opaque-ke 4.0.1, Ristretto255/TripleDH and Argon2id 19456 KiB/t2/p1,
not the fast Identity KSF from examples. A historical library audit does not make
this version/composition independently audited. A compromised code channel or
endpoint is not cured by calling the ceremony PAKE. Storage is not atomically
committed across two machines: a failed final receipt can leave one legitimate
local contact saved. Inspect state and retry explicitly; do not erase history.
