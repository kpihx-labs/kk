# Portable Prebuilt Installer Boundary

## Status
The same-package native helper lives in `src/install.rs` and submodules with CLI wiring.
`scripts/install.sh` bootstraps Linux with authenticated raw helper and notices
assets; `--helper` instead accepts an explicitly already-trusted native `kk`.
PowerShell on Linux supports only explicit `-Helper`; without it it exits 3.
Help exits 0; input/tool failures exit nonzero. No profile/PATH/service/identity changes occur.
This is not release or native-platform qualification.

The format/platform authority remains [release.py](../scripts/release.py), not a second registry.
Linux host/CPU selects a candidate; its authenticated manifest must match.
Only Linux transactions are enabled. Windows/macOS are refused; ZIP is not Windows
support. Separately provision capable `gh`, `curl`, `jq`, `sha256sum`, `timeout` and Linux tools;
the native helper also needs `ldconfig`. No Rust/Node/Python is needed by end users.

## Inspect Before Running
After release gates, retrieve its entrypoint with separately trusted tooling,
authenticate and inspect it, then invoke it. Never pipe network content into a shell.
Replace the example selectors with the reviewed release tag and full source commit.
This is not published-asset evidence; publication requires all existing release/exact-byte replay gates.

```sh
gh release download vVERSION --repo kpihx-labs/kk --pattern install.sh --dir FRESH_DIRECTORY
gh attestation verify FRESH_DIRECTORY/install.sh --repo kpihx-labs/kk \
  --signer-workflow kpihx-labs/kk/.github/workflows/check.yml --source-ref refs/tags/vVERSION \
  --source-digest FULL_COMMIT_SHA --deny-self-hosted-runners
less FRESH_DIRECTORY/install.sh
sh FRESH_DIRECTORY/install.sh --version VERSION --commit FULL_COMMIT_SHA
```

Use fresh destinations and retain inspected bytes. For PowerShell retrieve and authenticate
`install.ps1` with the same pinned policy, inspect it with `Get-Content`, then
run `pwsh -NoProfile -File ./install.ps1 -Version VERSION -Commit FULL_COMMIT_SHA
-Helper /absolute/trusted/kk` on Linux.
Do not bypass execution policy. Neither bootstrap auto-installs dependencies.

## Native Helper And Bootstrap Trust
Shell archive tools do not provide the native verifier's bounded immutable-snapshot guarantees.
Never execute a helper extracted from an unverified archive to verify itself.
`bundle` emits `kk-vVERSION-linux-ARCH-bootstrap.bin` (unchanged archive executable)
and `kk-vVERSION-linux-ARCH-bootstrap-notices.json`, where ARCH is `x86_64` or `aarch64`.
Both require CI attestation. JSON retains all non-executable archive files and collected
upstream text/inventory, target/platform/CPU/format, version/commit and both asset names.
The shell parses authenticated JSON with jq and matches selectors, helper basename and
SHA-256 BEFORE chmod/execution. Two independently authenticated but swapped subjects fail.
This inventory is not independent legal-completeness review. Never ship the helper alone.
Downloads retain a private directory, bounded bodies/metadata, time and five redirects.
Each hop is checked BEFORE following: exact HTTPS github.com, release-assets.githubusercontent.com
or objects.githubusercontent.com only; userinfo/ports/other hosts fail. Signed URLs are not printed.
Missing jq is fatal, never auto-installed. The retained notices path is printed on success.

```text
kk install --version VERSION --commit FULL_SHA --prefix ABSOLUTE
  [--archive ABSOLUTE [--bundle ABSOLUTE [--trusted-root ABSOLUTE]]]
kk rollback --prefix ABSOLUTE --version VERSION [--bundle ABSOLUTE [--trusted-root ABSOLUTE]]
```

The helper owns the complete transaction, including these gates:
1. Public GitHub Releases only, repository `kpihx-labs/kk`. No moving latest
   selector or alternate URL. Explicit version and full commit bind the reviewed
   tag. Metadata parsing is bounded; authenticated build manifests remain the sole
   target/version/files authority. No helper-authentication bypass flag exists.
2. Authorize each HTTPS hop against the exact bootstrap authorities above before
   connecting. Bound hop count, total time, metadata and even chunked body bytes.
   A content-length header alone is not a bound. Tool process groups are stopped
   before leader reaping; cleanup survives caller cancellation. No dependency installs.
3. Require separately installed `gh` with attestation verification support. Invoke
   argv, not shell strings, equivalent to the existing documented verifier:

   ```sh
   gh attestation verify "$SNAPSHOT" --repo kpihx-labs/kk \
     --signer-workflow kpihx-labs/kk/.github/workflows/check.yml \
     --source-ref "refs/tags/v$VERSION" --source-digest "$COMMIT" \
     --deny-self-hosted-runners
   ```

   A bounded capability check runs before bootstrap download. Missing flags/tool,
   timeout or failure is fatal. Bootstrap uses private retained paths (same-UID
   tampering excluded); native archive gh reads sealed memfd bytes via procfs.
4. USB/local `--archive` uses the same private immutable snapshot and provenance/content path as download.
   Offline use needs exported `--bundle` AND separately provisioned `--trusted-root` (gh `--custom-trusted-root`).
   Both trust inputs are bounded kernel-sealed snapshots; all local input paths must be absolute.
   Authenticate public roots out of band; true air-gapped replay remains a separate qualification gate.
   Rollback reauthenticates the retained original archive through gh; mutable receipts are not provenance.
   Shell explicit-root mode requires `--helper`; PowerShell always requires `-Helper`. No helper bootstrap in that mode.
5. Authenticate the snapshot before parsing archives. Verify manifest format,
   target, version, commit, exact member set and every digest before extraction.
   Match release.py bounds on compressed/expanded/member/manifest bytes and count;
   preflight ZIP directory allocations and bound gzip/TAR header processing.
   Reject traversal, absolute/noncanonical paths, duplicates, case collisions,
   file/directory conflicts, links, special files, unsafe modes, encryption,
   unsupported ZIP64/multipart data and inconsistent lengths. No `tar -x` shortcut.
6. Stage only authenticated declared ordinary files under an exclusively created
   private directory. Linux operations use no-follow descriptor-relative traversal.
   Never execute candidates. Derive ELF DT_NEEDED/version needs, compare the manifest,
   and check supported system libraries' ELF version definitions via bounded ldconfig.
7. Keep immutable `PREFIX/lib/kk/VERSION` bytes. Reject same-version different
   bytes, modified managed binaries and unrelated `PREFIX/bin/kk`, including foreign
   symlinks into unowned trees. Same-version/rollback compares all retained files to
   fresh authenticated bytes, not receipt claims. A nonblocking flock serializes
   writers; elevated execution and foreign/writable prefix ancestors are refused.
8. Activate using an atomic same-filesystem managed-link replacement, retain prior
   bytes and the previous activation record for explicit rollback. Refuse foreign
   state rather than repair it. No cleanup/deletion of old installations. Crashes
   leave either the old active version or a fully verified new version, never partial
   active bytes. Interrupted stages/activation records are retained, never auto-cleaned.
   Stable private paths and cooperating writers holding the same lock are REQUIRED.
   Uncoordinated same-UID replacement/root/account compromise is excluded. No profiles.

## Evidence And Remaining Gates

`uv run --no-project tests/test_installers.py` tests quoting, dispatch and failed
helper/notice verification with MOCKED gh/curl. `cargo test --test install` exercises actual
TAR/ZIP parsing, hostile archives and managed filesystem/rollback transactions with
explicitly MOCKED gh/curl. It is not real GitHub provenance or download evidence.
PowerShell execution tests run only if `pwsh` is already available; skips are explicit.
Native [evidence](install-qualification.md) is not raw bootstrap replay; that and offline trust remain gates.
Release pairs are NOT atomic: private stages are retained; retry identical companion inputs to fill
partial publication. Existing differing bytes/links fail closed. See [distribution](distribution.md).
