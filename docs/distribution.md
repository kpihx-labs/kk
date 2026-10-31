# Build, Verify And Distribute

Installing a prebuilt binary? Start with [installers](installers.md),
[offline/USB](offline-install.md) or [platform scope](platforms.md).
This page is for contributors producing a delivery.

```text
clean commit -> checks + audit + SDK + native gates -> signed draft
                                                          |
                                             authenticate EXACT bytes
                                                          |
                                              install/update/rollback
                                                          |
                                             publish UNCHANGED assets
```

## Build And Inspect

```sh
cargo fetch --locked
make check
make sdk-check
make bundle TARGET=x86_64-unknown-linux-gnu
make verify-bundle ARCHIVE=DOWNLOADED_BINARY_ARCHIVE.tar.gz
uv run --no-project scripts/release.py verify DOWNLOADED_BINARY_ARCHIVE.zip
```

| Need | Action | Result / limit |
| --- | --- | --- |
| Build a candidate | Bundle clean committed source | Executable, contract/license, notices and manifests |
| Check archive integrity | Verify TAR/ZIP without extraction | Exact bounded members/digests; not origin authentication |
| Authenticate origin | Verify CI attestation below | Repository/workflow/tag/commit binding |
| Qualify delivery | Replay the authenticated artifact | Separate builds are not assumed byte-identical |
| Install from source | `make install` or `make install PREFIX="$HOME/custom-prefix"` | Immutable version tree and managed link; no identity/service/PATH change |

## Authenticate Before Publication

```sh
gh attestation verify "$ARCHIVE" --repo kpihx-labs/kk \
  --signer-workflow kpihx-labs/kk/.github/workflows/check.yml \
  --source-ref "refs/tags/$TAG" --source-digest "$COMMIT" \
  --deny-self-hosted-runners
```

Use separately trusted capable `gh` and reviewed selectors. Keyless CI provenance
is not an operator signing key or security audit. A tag/draft is not publication;
never rebuild assets, replace bytes or move tags to hide failed qualification.
Only authorized promotion publishes the same accepted draft.

## Packaging Invariants

| Owner | Invariant |
| --- | --- |
| [release.py](../scripts/release.py) | Authoritative targets, format/size/member bounds and deterministic packaging inputs |
| TAR/ZIP verifier | One bounded immutable snapshot for hashing/parsing; reject traversal, links, collisions, unsafe types and extra/missing bytes |
| ZIP | Sorted ordinary files, explicit permissions and UTC commit timestamp; no encryption/multipart/ZIP64; compression toolchain affects reproducibility |
| Bootstrap | Archived executable is unchanged; authenticated companion notices bind selectors, helper name/hash and collected texts |
| CI uploads/attestations | Cover TAR, ZIP, bootstrap helper/notices and reviewed installer entrypoints; local packaging is not provenance |
| Partial bundle publication | Retain private stages; identical retry fills missing companions, foreign bytes/links fail closed; asset sets are not atomic |
| Offline source | Include tracked local overrides plus locked registry sources; Rust/C tools and pinned toolchain remain prerequisites |

`Cargo.toml`/`Cargo.lock` own version/dependencies. Upstream licenses remain intact;
inventory is not an independent legal-completeness review. SDK reference tests may
need separate caches. See [overrides](dependency-overrides.md),
[third-party sources](../third_party/README.md) and [current evidence](progress.md).
