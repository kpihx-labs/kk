# Build, Verify And Install

Current publication state lives only in [TODO.md](../TODO.md). Source version and
resolved dependencies are owned by `Cargo.toml` and `Cargo.lock`.

## Release Path

```text
clean commit -> checks + strict audit + SDK + native architecture builds
                                              |
                                 signed draft archives
                                              |
                             authenticate + exact-byte VM replay
                                              |
                              publish same assets -> install
```

A tag is not a published release. Separate builds are not assumed byte-identical.
The exact draft executable is qualified before promotion; assets are not rebuilt,
silently replaced or force-retagged to hide a failure.

## Build And Check

```sh
cargo fetch --locked
make check
make sdk-check
make bundle TARGET=x86_64-unknown-linux-gnu
make verify-bundle ARCHIVE=DOWNLOADED_BINARY_ARCHIVE.tar.gz
```

Bundles require clean committed source. Binary archives contain the executable,
contract/license, complete dependency notices and file/build manifests. The verifier
rejects unsafe members, changed/missing/extra files and bounded gzip/TAR overruns.
Integrity checks alone do not authenticate an archive.

## Authenticate Origin

Use a `gh` version supporting attestation verification, with expected values from
the release/tag under review:

```sh
gh attestation verify "$ARCHIVE" --repo kpihx-labs/kk \
  --signer-workflow kpihx-labs/kk/.github/workflows/check.yml \
  --source-ref "refs/tags/$TAG" --source-digest "$COMMIT" \
  --deny-self-hosted-runners
```

The workflow uses keyless CI provenance, not an exported operator signing key.
After exact-artifact replay and cleanup, the authorized publication step promotes
the same draft with `gh release edit "$TAG" --draft=false --repo kpihx-labs/kk`.
That is not authorization to ignore an operator's explicit publication pause.

## Offline Source And Installation

The authenticated source archive includes tracked source, explicit local overrides
and vendored locked registry sources. Rust/C tools and the pinned toolchain remain
prerequisites. The product can rebuild without registry access; historical SDK
reference tests may still need their separate cache. Public upstream test vectors
are not operator identities or runtime state.

```sh
make install
# Optional destination:
make install PREFIX="$HOME/custom-prefix"
```

This builds/installs a per-user binary, retains immutable version directories and
updates only its managed link. It refuses an unrelated `kk` and creates no identity,
service, firewall rule or alias. It is not a root-owned security appliance.
Licensing/provenance owners: [third-party sources](../third_party/README.md) and
[overrides](dependency-overrides.md).
