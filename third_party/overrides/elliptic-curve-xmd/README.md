# RustCrypto Expansion Subset

Internal Ristretto dependency component, **not a general elliptic-curve SDK**.

```text
official elliptic-curve 0.13.8 expansion files, unchanged
                         |
              narrow module/error wrapper
                         |
              existing Ristretto OPAQUE path
```

The three expansion files and their original vectors/licenses are preserved.
Only XMD/XOF expansion is exposed; no legacy NIST arithmetic or crypto-bigint 0.5
implementation is included. The error type is re-exported from maintained
elliptic-curve. No new expansion algorithm or hash specialization is implemented.

Source hashes are enforced by [the ledger](../sources.json), and upstream vectors
plus independent OPAQUE wire peers run through `make sdk-check`. Scope and update
duties: [dependency review](../../../docs/dependency-overrides.md).
