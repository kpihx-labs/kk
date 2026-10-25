# Unified CLI And Sensitive Data

> Normal transport is already encrypted. Sensitive mode protects local handling.

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
