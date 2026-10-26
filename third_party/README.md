# Supplemental Upstream Notices

These are exact public upstream notice bytes, not a relicensing of dependencies.
Some published Cargo packages declare SPDX alternatives but omit their workspace
license files. `notices.json` maps each affected locked package version to copied
notice paths and records immutable repository revision, git blob and SHA-256.
Root, ancestor/crate, LICENSES and REUSE notice trees are retained where supplied.

When an upstream README explicitly supplies an Apache-2.0 alternative but no full
license text, that exact pinned README and any referenced AUTHORS file are kept
alongside the canonical Apache-2.0 text from a pinned SPDX license-data revision.
The selected alternative and its basis are recorded. The CC0-only package lacking
VCS metadata receives the canonical public-domain instrument; its published Cargo
declaration and locked package checksum remain the grant/source reference.

`scripts/collect-notices.py` performs this optional maintainer-time collection
through the native GitHub CLI, without handling or printing authentication values.
Normal release builds read only the committed public notices and verify hashes;
they do not fetch mutable license text. Existing collected bytes are not replaced.
`scripts/release.py licenses` inventories all resolved dependency licenses and
notices. An incomplete inventory blocks release bundle generation.

Dependencies that ship their own root notices are copied from their cached locked
Cargo sources during packaging. Full locked vendored source accompanies the
release. GStreamer system runtime plugins are not bundled or reclassified under
this project's BUSL license; their own installed licensing remains applicable.
