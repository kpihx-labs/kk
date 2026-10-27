# Verifiable Distribution

## 0.8.1 Candidate Update

The local dependency hold recorded below for 0.8.0 is resolved by the explicit
overrides and qualification in `dependency-overrides.md`. All strict audit and
local software/SDK checks pass. Source overrides and their original licenses are
included in offline source; their unchanged primitive hashes are checked before
notice/archive generation. The production graph and the historical reference
test graph are separate. Final hosted and VM/artifact gates are still pending;
no final release is asserted by these local results.

## Current Security Hold (2026-10-27)

The current source is a candidate with unresolved dependency advisories. Its
functional tests and signed provenance do not establish security qualification.
The release workflow requires the separate unsuppressed dependency audit before
signing or publishing a release. No release tag is created during this hold.
Details and the isolated remediation experiment are recorded in
`security-review.md` (SR-004).

The system's `gh 2.46.0` lacks attestation verification. The candidate verification
was exercised with a temporary, checksum-checked `gh 2.103.3`, without changing
the installed CLI. A verifier must support `gh attestation verify`. In addition
to `--repo`, enforce `--signer-workflow kpihx-labs/kk/.github/workflows/check.yml`,
the expected `--source-ref` and `--source-digest`, and
`--deny-self-hosted-runners`. The tested manual candidate uses its exact branch
and commit; it is not presented as a signed final version tag.

Release archives are generated only from clean committed source and an exact
qualified target. The package version and contract version live in Cargo metadata.
No profile, private identity, browser bootstrap, test state or agent context belongs
in an archive. Existing archive names are never silently overwritten.

```sh
cargo fetch --locked
make check
make bundle TARGET=x86_64-unknown-linux-gnu
make verify-bundle ARCHIVE=DOWNLOADED_BINARY_ARCHIVE.tar.gz
```

Each binary archive contains the executable, public contract/license, locked
dependency inventory, upstream notices and a build manifest. The verifier checks
every ordinary file, rejects extra/missing members, traversal, duplicate members,
links and archive resource overruns. SHA-256 checksums and a separate build record
are published alongside it. Checksums establish integrity only when their source
is independently trusted.

The tagged GitHub release job runs only after the required native Linux matrix and
static lab jobs succeed. It generates keyless build-provenance attestations through
the CI OIDC identity, includes the public verification bundle, and publishes no
operator private signing key. Authenticate a downloaded archive against the exact
repository/release workflow identity, not just a self-contained hash manifest:

```sh
gh attestation verify DOWNLOADED_BINARY_ARCHIVE.tar.gz --repo kpihx-labs/kk
```

Attestation availability is a CI result, not something a local script claims to
have produced. Public source remains mirrored over the two SSH git remotes. A
release is not reported complete until required workflows and published assets
have actually been inspected.

## Offline Source And Notices

The companion source archive includes all tracked source and every locked Cargo
dependency, with a relative vendored-source Cargo configuration. After trusted
archive authentication, Rust/C build tools and the pinned toolchain are still
prerequisites; a Cargo registry connection is not required for the rebuild.

`THIRD-PARTY.json` preserves declared license expressions. Missing packaged notices
are copied without rewriting from upstream crate VCS revisions and recorded by
repository, revision, git blob and SHA-256. REUSE license trees are preserved.
The collector never replaces this project's BUSL license or upstream licenses.
The full vendored sources accompany the notices; GStreamer remains an explicit
system dependency, not an silently relicensed binary bundle.

## Local Installation

```sh
make install
# Optional explicit destination:
make install PREFIX="$HOME/custom-prefix"
```

This is a per-user, non-privileged application installation. It keeps each version's
bytes immutable under the selected prefix, refuses to replace an unrelated `kk`,
and updates only its own managed entry link. It creates no identity, service,
firewall rule or shell alias. It is not a root-owned immutable security-appliance
deployment; local account compromise remains outside the documented threat model.
