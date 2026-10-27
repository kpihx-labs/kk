# Security Dependency Qualification

## Scope

The 0.8.1 candidate removes the four crypto advisories and two maintenance
warnings found in 0.8.0, without an audit ignore or a change to pairing's wire
suite. The full product lockfile passes cargo-audit 0.22.2 with `--deny warnings`
against database revision `3279c99911354c400e1d41e47c7d60e39258b147`.
This is a known-advisory check, not a claim of perfect or independently certified
cryptography. Final VM replay, hosted CI and artifact qualification remain gates.

## Ownership And Provenance

All overrides live in `third_party/overrides`, not in ignored study clones or
machine-global package edits. `sources.json` records immutable upstream origins,
local changes and exact hashes of the unchanged expansion files. Root Cargo
patches select these sources explicitly, and the root lockfile selects every
registry version. Original licenses remain intact; K&K's license does not cover
the imported code. Notice generation now includes path overrides rather than
incorrectly treating every source-less package as the first-party root.

The OpenMLS credential, Libcrux backend and traits packages are a matching
upstream snapshot. Their default synchronous storage interface is retained;
no private-state repair, key regeneration or database migration is introduced.
The backend uses the corrected Libcrux DRBG/KEM dependencies. The age snapshot
uses current RustCrypto primitives; i18n-config no longer selects basic-toml.

Hax's conditional diagnostic dependency is moved to proc-macro-error3 with its
Syn 2 compatibility feature. Generated diagnostic tokens require the actual new
crate name, not just an alias. Both normal code and the actual `cfg(hax)` macro
implementation compile with upstream's nightly-2025-11-08. This is not an F*/Lean
extraction or proof claim; the product remains pinned to stable 1.99.1.

## Ristretto Boundary

Pairing still uses Ristretto255, TripleDH, SHA-512 and the same Argon2 parameters,
identifiers and context binding. The old crypto-bigint came through the generic
legacy elliptic-curve dependency, although K&K does not offer the NIST adapters.
Simply replacing that dependency with a newer major version would change digest
interfaces and would not establish protocol compatibility.

Instead, the legacy expansion interface, XMD and XOF files are copied unchanged
from the official elliptic-curve 0.13.8 crate. They expose the same generic hash
interface; they are not replaced with a hardcoded SHA-512 specialization or a new
implementation. The wrapper only exports those modules and the maintained error
type. Legacy curve arithmetic and crypto-bigint 0.5 are absent from the product
graph. NIST blanket/group adapters are removed from the internal OPAQUE/VOPRF
subset. They must not be re-enabled implicitly or advertised as supported.

OPAQUE explicitly declares generic-array's zeroize support, which its secret
buffers require and previously received through feature unification. The
Ristretto arithmetic, password processing and protocol calculations are unchanged.
Other small changes are an explicit lifetime and feature gates for unused helpers,
not warning suppression or altered cryptographic computations.

## Evidence

- The three imported expansion files compare byte-for-byte with the registry
  source, and their declared hashes are checked by tests and archive generation.
- All six upstream XMD/XOF vector groups pass from the preserved source files.
- `make sdk-check` compiles separate reference/restricted OPAQUE executables.
  Metadata checks require a registry reference and explicit product overrides;
  empty independent workspaces prevent accidental shared patch inheritance.
- Both client/server directions complete registration/login and agree on hashes
  of the session keys. Wrong password and changed context fail in both directions.
  Keys and private setup stay inside the ephemeral test processes.
- `make hax-check` exercises the conditional macro implementation without
  RUSTC_BOOTSTRAP. The root product and its public CLI do not require nightly.
- The complete 0.8.1 local check passes 155 Rust, six JavaScript and 11 packaging
  tests, with warning-free formatting, Clippy and public documentation.
- The reviewed notice inventory covers all 485 dependencies, including overrides,
  with no missing notice. The authoritative product audit covers 486 packages
  including K&K itself, with no denied finding.

The historical reference test graph intentionally uses the official old SDK for
compatibility comparison. It is not linked into the product or a security waiver.
An offline product-source rebuild does not promise uncached reference downloads.

## Continuing Duties

Follow upstream advisories for the recorded components, review source updates
explicitly and retain original notices. Do not edit copied primitive files and
continue claiming byte identity. Any primitive change, additional suite or feature
expansion needs new source review, vectors and interoperability qualification.
Strict hosted audit, architecture checks, native VM replay and signed artifact
verification remain required before the candidate is described as released.
