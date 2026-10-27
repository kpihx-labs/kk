# Reviewed Dependency Overrides

These source overrides are staged for the 0.8.1 security qualification, not an
assertion that the final release gates have passed. Each directory retains its
upstream source and original license. K&K's root license does not relicense it.

| Packages | Upstream Origin | Local Changes |
| --- | --- | --- |
| openmls_basic_credential, openmls_libcrux_crypto, openmls_traits | openmls/openmls at be248d4b1306d87a641ce24ed0ffcab2b27967f6 | Cargo-packaged source; matching synchronous default traits; no cryptographic changes |
| age, age-core | str4d/rage at b5b68c4f4f7ea5c368bc060f6fe0237549c3a13f | Gate the CLI-only accumulator helper on armor plus cli-common |
| i18n-config | kellpossible/cargo-i18n at 10e345470d97c9cffb3bc611182f595ad93251bf | Cargo-packaged upstream maintained TOML dependency |
| hax-lib-macros 0.3.7 | crates.io; hacspec/hax at d8b5b3d3b666fee8943a351445d2b680105e8ea3 | proc-macro-error3 with syn2-error and explicit crate-name migration; normal and cfg(hax) compile checked |
| opaque-ke 4.0.1 | crates.io official source | Remove unused legacy NIST group adapter; explicitly own generic-array/zeroize; gate unused signature helper |
| voprf 0.5.0 | crates.io official source | Remove unused NIST blanket adapter and module; keep original Ristretto and hash calculations; explicit lifetime annotation |
| elliptic-curve XMD subset | crates.io elliptic-curve 0.13.8 | Unchanged expand_msg/XMD/XOF files, with a narrow module wrapper and maintained error type; no legacy curve arithmetic or crypto-bigint 0.5 code |

The expansion files match their upstream source byte-for-byte. Six upstream
XMD/XOF vector groups pass. Independently compiled official and restricted OPAQUE
processes interoperate in both client/server directions, agree on the session and
reject changed passwords or contexts. These are protocol regression evidence,
not a new cryptographic proof or qualification of removed NIST suites.

Do not silently enable the removed NIST adapters. The product's pairing suite
is Ristretto255 with its existing identifiers, context and Argon2 parameters.
Any broader suite change needs a separate dependency and protocol review.

Format and lint K&K's own package separately: do not reformat imported primitive
files and then claim they remain byte-identical to upstream. The root lockfile,
strict advisory gate, full application tests and final VM/artifact replays are
the authoritative release gates. Nested upstream lockfiles are retained as
historical provenance, not the product's resolution.
