# Portable Prebuilt Installer Boundary

## Status

The same-package native helper lives in `src/install.rs` and submodules, with
CLI wiring and normal archive dependencies. End users need no Rust/Node/Python.
`scripts/install.sh` and `scripts/install.ps1` accept an explicit already-trusted
native `kk` via `--helper`/`-Helper`. Without it they exit 3 before installation.
Help exits 0; bad input/tool failures exit nonzero. No profile, PATH, service or
identity changes occur. This is not release or native-platform qualification.

The archive format and platform authority remain [release.py](../scripts/release.py),
not a second release registry. Linux host/CPU detection selects a candidate using
the existing release filename convention; its authenticated manifest must match.
Only Linux transactions are enabled. PowerShell on Linux can invoke the helper;
Windows/macOS are refused. ZIP availability does not imply Windows support.

## Inspect Before Running

After a public release has passed its gates, download its bootstrap over HTTPS,
inspect it, then invoke it explicitly. Do not pipe network content into a shell.
Replace the example selectors with the reviewed release tag and full source commit.
The bootstrap and raw helper assets below are a release integration design, not
an assertion that public assets or their attestations exist today.

```sh
curl --disable --fail --show-error --location --max-redirs 5 --proto '=https' --proto-redir '=https' \
  --connect-timeout 15 --max-time 60 --max-filesize 131072 \
  --output install.sh \
  https://github.com/kpihx-labs/kk/releases/download/vVERSION/install.sh
less install.sh
sh install.sh --version VERSION --commit FULL_COMMIT_SHA --helper /absolute/trusted/kk
```

Use fresh destinations and retain inspected bytes. PowerShell users can use
the same curl flags with `curl.exe`, inspect `install.ps1` with `Get-Content`, then
run `pwsh -NoProfile -File ./install.ps1 -Version VERSION -Commit FULL_COMMIT_SHA
-Helper /absolute/trusted/kk` on Linux.
Do not bypass execution policy. Neither bootstrap auto-installs dependencies.

## Native Helper And Bootstrap Trust

Shell archive listings and extraction tools do not provide the existing verifier's
bounded immutable-snapshot guarantees. Python is not an end-user prerequisite.
A separately provisioned, explicitly trusted native `kk` binary is required.
Do not execute a helper extracted from an unverified archive to verify itself.
The release workflow must publish a standalone raw host-specific `kk` asset and
include those exact bytes in its existing GitHub provenance subject list. Download
it with HTTPS-only redirects, fresh output, time/redirect bounds and a byte cap.
Authenticate that raw asset with the gh command below BEFORE chmod/execution;
bind it to the reviewed helper tag/commit. Never use an archive's candidate as its
own verifier. Raw-helper asset publication/bootstrap automation remains Main's gate.

```text
kk install --version VERSION --commit FULL_SHA --prefix ABSOLUTE
  [--archive ABSOLUTE [--bundle ABSOLUTE]]
kk rollback --prefix ABSOLUTE --version VERSION [--bundle ABSOLUTE]
```

The helper owns the complete transaction, including these gates:
1. Public GitHub Releases only, repository `kpihx-labs/kk`. No moving latest
   selector or alternate URL. Explicit version and full commit bind the reviewed
   tag. Metadata parsing is bounded; authenticated build manifests remain the sole
   target/version/files authority. No helper-authentication bypass flag exists.
2. HTTPS-only initial requests and redirects, bounded count, connection and total
   time, compressed bytes and metadata bytes. Reject oversized chunked bodies too.
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

   A capability check runs before download. Missing flags/tool, timeout or failure
   is fatal. gh reads kernel-sealed memfd bytes via procfs, not replaceable input paths.
4. USB/local `--archive` uses exactly the same private immutable snapshot and
   provenance/content path as download. Offline verification additionally requires
   an exported GitHub attestation `--bundle` passed to gh's supported bundle option,
   plus its separately provisioned trust material. Local input paths must be absolute.
   Offline trust-root setup must be separately qualified. Rollback reauthenticates
   its retained original archive through gh; mutable local receipts are not provenance.
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

`uv run --no-project tests/test_installers.py` tests bootstrap quoting and dispatch
with mocked discovery/helper tools. `cargo test --test install` exercises actual
TAR/ZIP parsing, hostile archives and managed filesystem/rollback transactions with
explicitly MOCKED gh/curl. It is not real GitHub provenance or download evidence.
PowerShell execution tests run only if `pwsh` is already available; skips are explicit.
Public raw assets, actual gh offline setup, replay and full crash injection remain gates.
Dependencies: tar 0.4.46 (MIT/Apache, MSRV 1.63), flate2 1.1.10 (MIT/Apache, 1.67),
zip 9.1.3 (MIT, 1.88). RustSec fixes reviewed; cargo-audit/pwsh are unavailable locally.
