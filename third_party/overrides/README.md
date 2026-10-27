# Source Overrides

These internal sources are selected explicitly by root Cargo patches. They retain
their original licenses; K&K's license does not relicense them.

| Source group | Scoped change |
| --- | --- |
| OpenMLS credential / Libcrux / traits | Matching upstream snapshot; synchronous default storage |
| age / age-core | Updated primitives; CLI-only helper feature gate |
| i18n-config | Maintained TOML dependency |
| Hax macros | Maintained diagnostic binding with Syn 2 compatibility |
| OPAQUE / VOPRF | Ristretto-only internal subset; no legacy NIST adapters |
| Expansion subset | Unchanged XMD/XOF sources; no legacy curve arithmetic |

**Exact origins, local changes and primitive hashes:** [sources.json](sources.json).
**Rationale and gates:** [dependency review](../../docs/dependency-overrides.md).

Do not silently enable removed suites, reformat copied primitives or promote a
source update on an audit result alone. Root lockfile, source guards, vectors,
independent protocol peers and actual application/artifact tests are separate gates.
Nested upstream lockfiles are historical reference, not the product resolution.

Imported READMEs/changelogs below these directories are upstream documentation,
not the current K&K feature/status authority. They remain intact for provenance.
