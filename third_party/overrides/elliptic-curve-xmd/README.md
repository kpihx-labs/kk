# Experimental RustCrypto Expansion Subset

Not a general replacement for elliptic-curve and not approved for product use.
The three expand_msg source files and their upstream vectors are imported
unchanged from the elliptic-curve 0.13.8 registry archive. Original MIT/Apache
licenses are retained. Only the expansion interface is exposed; no legacy curve
arithmetic or legacy crypto-bigint implementation is included. The error type is
re-exported from maintained elliptic-curve 0.14.

This is an explicitly restricted internal experiment for the existing
Ristretto255 OPAQUE suite. NIST curve adapters must not be exposed by its callers.
Source equality, upstream RFC vectors and old/new OPAQUE interoperability are
required before any promotion. A zero audit report alone is not qualification.
