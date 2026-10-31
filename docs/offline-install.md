# Signed Local Bootstrap / USB First Install

The Linux/macOS shell installer supports first installation without an already trusted
`kk` executable or network downloads. The script, capable `gh` and public roots
still need independent provisioning/authentication. Windows remains refused.

For the reviewed version/platform/architecture, place these ordinary public files
in an absolute directory on the USB device or local disk:

```text
kk-vVERSION-PLATFORM-ARCH-bootstrap.bin
kk-vVERSION-PLATFORM-ARCH-bootstrap.bin.jsonl
kk-vVERSION-PLATFORM-ARCH-bootstrap-notices.json
kk-vVERSION-PLATFORM-ARCH-bootstrap-notices.json.jsonl
```

`PLATFORM` is `linux` or `macos`; `ARCH` is `x86_64` or `aarch64`.
The `.jsonl` sidecars are exported gh attestation bundles for the corresponding
subjects, not handwritten hashes. Rename gh's Unix `sha256:DIGEST.jsonl` downloads
to these portable sidecar basenames before copying to FAT/exFAT media. Authenticate
the installer script and provision public roots separately; a roots file bundled
by an untrusted USB provider is not an independent trust anchor.

```sh
sh /absolute/reviewed/install.sh --version VERSION --commit FULL_COMMIT_SHA \
  --bootstrap-dir /absolute/USB/bootstrap \
  --archive /absolute/USB/binary.zip --bundle /absolute/USB/archive.jsonl \
  --trusted-root /absolute/provisioned/trusted-root.jsonl \
  --prefix "$HOME/.local"
```

No curl is required for this mode. The script copies bounded helper, notices,
subject bundles and roots into its private retained distribution directory. It
checks both subject attestations with pinned repository/workflow/tag/commit and
explicit local bundle/roots, then checks their authenticated selector/SHA-256
association before enabling the copied helper. The USB executable is never
chmodded or executed directly. The helper still authenticates the local archive
and performs the full [native transaction](installers.md); shell archive extraction
is not used. Missing, linked, empty, oversized or unauthenticated bootstrap inputs
fail closed. Same-UID tampering/root/account compromise remains excluded.

The bootstrap directory is mutually exclusive with `--helper` and `--rollback`;
later rollback uses the installed trusted helper with the selected version's
bundle/roots. No secrets, compiler, identity or persistent service are needed.

The public kit preparer can assemble these inputs, including authenticated
`install.sh` and its sidecar, with `--with-bootstrap`; see the
[lab preparation guide](../lab/README.md). Preparation and mocked boundary tests
do not establish real air-gapped execution or physical USB qualification. The
[macOS implementation boundary](macos-install.md) remains distinct from native acceptance.
