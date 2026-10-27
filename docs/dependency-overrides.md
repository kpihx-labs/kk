# Dependency Overrides

Explicit internal sources remove the advisory/maintenance routes found in the
earlier graph. This is not a general cryptographic fork or an independent audit.

```text
Cargo.toml + Cargo.lock
          |
third_party/overrides/sources.json -> exact origins + allowed modifications
          |
source hashes + strict audit + vectors + independent peers
```

## Why Each Override Exists

| Family | Purpose and limit |
| --- | --- |
| OpenMLS credential / Libcrux / traits | Matching upstream snapshot with corrected dependencies; default storage stays synchronous |
| age / age-core | Updated RustCrypto dependencies; CLI-only helper gated to its actual features |
| i18n-config | Maintained TOML dependency instead of basic-toml |
| Hax macros | Maintained diagnostic crate with explicit Syn 2 binding and real crate-name migration |
| OPAQUE / VOPRF | Existing Ristretto protocol; remove unused legacy NIST adapters and declare required zeroization explicitly |
| Expansion subset | Preserve original generic XMD/XOF files; exclude legacy curve arithmetic/crypto-bigint 0.5 |

Exact revisions and changes are in the [source ledger](../third_party/overrides/sources.json),
not duplicated here. Original licenses apply; K&K's license does not relicense them.

## What Is Preserved

Ristretto255, TripleDH, SHA-512, Argon2 settings, identifiers and context binding
remain the existing pairing suite. The three RustCrypto expansion files are
byte-identical to their official source. The wrapper exports those modules and
a maintained error type; it does not specialize a generic hash parameter to an
unrelated algorithm or implement new arithmetic.

NIST adapters in this internal subset are not supported and must not be silently
re-enabled. Generic-array zeroization is declared by the owner of secret buffers,
not accidentally supplied by another dependency's enabled features.

## Qualification Gates

| Gate | Checks |
| --- | --- |
| `make check` | Product behavior, source-hash guards and notice/packaging invariants |
| Strict CI audit | Entire root locked graph, no advisory ignore or denied-warning suppression |
| `make sdk-check` | Preserved expansion vectors plus independent official/restricted wire peers |
| `make hax-check` | Actual conditional macro implementation with the upstream nightly |
| Final VM/artifacts | Exact executable, lifecycle/fault behavior and authenticated origin |

The reference SDK is intentionally historical, is independently resolved, and is
never linked into the product. Its test/cache requirements do not expand the
offline product-build promise. Hax macro compilation is not an F*/Lean proof run;
the application itself uses the repository-pinned stable toolchain.

Wire-test design: [independent interoperability gate](../lab/opaque-interop/README.md).

## Maintenance

Follow advisories for recorded upstream components. Any primitive edit, suite
expansion or source update needs explicit review, refreshed notices, vectors and
interoperability checks. Do not reformat copied primitive files and continue
claiming byte identity. Do not infer qualification from Rust or a zero-audit result.

Current results: [verification record](progress.md). Source layout:
[overrides](../third_party/overrides/README.md).
