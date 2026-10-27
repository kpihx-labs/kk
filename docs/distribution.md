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

Each currently supported Linux target also produces a reproducible `.zip` binary
archive alongside `.tar.gz`. Both carry the same executable, notices and manifest;
the per-target `.sha256` file covers both archive byte streams. ZIP uses sorted
ordinary files, explicit Unix permissions and a UTC commit timestamp clamped to
the ZIP date range. Reproducibility means identical inputs with the same packaging
toolchain, not byte equality across different compression library versions.

The existing `bundle --target ... --output ...` and `verify ARCHIVE [--sha256 ...]`
commands are unchanged. `verify` accepts `.zip` as well as `.tar.gz`, checks without
extracting, and applies the same manifest/digest gates. It rejects traversal,
noncanonical paths, duplicate or case-colliding members, file/directory conflicts,
links and unsafe file types. Compressed input, expanded data, member counts and
manifest sizes are bounded in [release.py](../scripts/release.py). ZIP directory
metadata is bounded before parsing; encryption, multipart and ZIP64 archives are
not supported. Offline source remains `.tar.gz` and is not a binary-manifest
archive. A new container format does not qualify a new operating system or CPU.

Verification opens the input once with no-follow/nonblocking flags, validates the
opened descriptor as an ordinary file, and reads a bounded immutable snapshot.
Trusted-digest hashing, ZIP directory preflight and archive parsing all consume
that same snapshot. Replacing the pathname or rewriting the file after hashing
cannot substitute different bytes for parsing.

```sh
uv run --no-project scripts/release.py verify DOWNLOADED_BINARY_ARCHIVE.zip
```

The release workflow's provenance `subject-path` includes `target/dist/*.zip`
alongside `target/dist/*.tar.gz`. Before public ZIP distribution, exact-byte
replay must authenticate and verify the ZIP selected for installation. Current
matrix uploads, provenance-artifact uploads and draft asset creation already use
`target/dist/*`, which includes ZIP; keep that coverage when changing upload
filters. Creating ZIP locally does not itself provide a CI attestation or replay
evidence. Managed installation remains immutable and unchanged below.

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
