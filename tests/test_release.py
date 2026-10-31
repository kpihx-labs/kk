import contextlib
import hashlib
import importlib.util
import io
import json
import stat
import struct
import tarfile
import tempfile
import unittest
import warnings
import zipfile
from pathlib import Path
from unittest import mock

spec = importlib.util.spec_from_file_location("kk_release", Path(__file__).resolve().parents[1] / "scripts/release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class OverrideProvenanceTests(unittest.TestCase):
    @contextlib.contextmanager
    def recorded_sdk(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            base = root / "third_party/overrides"
            directory = base / "openmls-0.9.2"
            (directory / "src").mkdir(parents=True)
            (directory / "Cargo.toml").write_text('[package]\nname="openmls"\nversion="0.9.2"\nlicense="MIT"\nrepository="https://example.test/openmls"\n')
            (directory / ".cargo_vcs_info.json").write_text(json.dumps({"git": {"sha1": "a" * 40}, "path_in_vcs": "openmls"}))
            (directory / "src/replay.rs").write_bytes(b"reviewed local fixture")
            (directory / "src/primitive.rs").write_bytes(b"unchanged upstream fixture")
            (root / "Cargo.toml").write_text('[dependencies]\nopenmls={version="=0.9.2",path="third_party/overrides/openmls-0.9.2"}\n')
            (root / "Cargo.lock").write_bytes(b"fixture locked graph")
            relative = "licenses/upstream/LICENSE"
            notice = root / "third_party" / relative
            notice.parent.mkdir(parents=True)
            notice.write_bytes(b"original upstream fixture notice")
            extra = {"packages": {"openmls@0.9.2": [relative]}, "sources": {
                relative: {"revision": "a" * 40, "sha256": release.digest(notice.read_bytes())}}}
            (root / "third_party/notices.json").write_text(json.dumps(extra))
            origin = {"directory": "openmls-0.9.2", "sdk_dependency": "openmls",
                      "repository": "https://example.test/openmls", "revision": "a" * 40,
                      "path_in_vcs": "openmls", "required_notices": [relative],
                      "reviewed_files": {"src/replay.rs": release.digest(b"reviewed local fixture")}}
            origin["upstream_tree_sha256"] = release.override_tree_digest(directory, origin["reviewed_files"])
            (base / "sources.json").write_text(json.dumps({"packages": {"openmls@0.9.2": origin}, "unchanged_primitives": {}}))
            metadata = {"resolve": {"root": "root"}, "packages": [
                {"id": "root", "name": "root", "version": "1", "source": None},
                {"id": "dependency", "name": "openmls", "version": "0.9.2", "source": None,
                 "license": "MIT", "repository": origin["repository"], "manifest_path": str(directory / "Cargo.toml")}]}
            with mock.patch.object(release, "ROOT", root), mock.patch.object(
                    release, "command", side_effect=lambda *args: json.dumps(metadata)):
                yield root, directory, metadata, extra

    def test_current_openmls_origin_and_sdk_source_are_statically_guarded(self):
        # Reads public source/metadata only, never invokes Cargo.
        origins = release.check_override_sources()
        origin = origins["openmls@0.9.2"]
        self.assertEqual(origin["sdk_dependency"], "openmls")
        self.assertIn("src/key_packages/key_package_in.rs", origin["reviewed_files"])

    def test_recorded_local_origin_and_original_notice_survive_collection(self):
        with self.recorded_sdk() as (_, directory, _, _):
            (directory / ".cargo-ok").write_bytes(b"cache marker is not upstream source")
            notices, files = release.licenses()
            self.assertEqual(notices[0]["revision"], "a" * 40)
            self.assertEqual(notices[0]["license"], "MIT")
            self.assertEqual(files[notices[0]["notices"][0]][0], b"original upstream fixture notice")

    def test_reviewed_and_unreviewed_source_drift_are_distinct_failures(self):
        for name, expected in (("replay.rs", "Reviewed override"), ("primitive.rs", "Unreviewed imported")):
            with self.subTest(name=name), self.recorded_sdk() as (_, directory, _, _):
                (directory / "src" / name).write_bytes(b"unexpected drift")
                with self.assertRaisesRegex(ValueError, expected):
                    release.check_override_sources()

    def test_vcs_revision_or_manifest_license_cannot_drift(self):
        with self.recorded_sdk() as (_, directory, _, _):
            (directory / ".cargo_vcs_info.json").write_text(json.dumps({"git": {"sha1": "b" * 40}, "path_in_vcs": "openmls"}))
            with self.assertRaisesRegex(ValueError, "identity/revision"):
                release.check_override_sources()
        with self.recorded_sdk() as (_, directory, _, _):
            path = directory / "Cargo.toml"
            path.write_text(path.read_text().replace('license="MIT"', 'license="Apache-2.0"'))
            with self.assertRaisesRegex(ValueError, "Unreviewed imported"):
                release.check_override_sources()

    def test_symlinks_and_extra_upstream_sources_are_not_silently_ignored(self):
        for mutation in ("link", "extra"):
            with self.subTest(mutation=mutation), self.recorded_sdk() as (_, directory, _, _):
                path = directory / "src/unreviewed.rs"
                if mutation == "link":
                    path.symlink_to(directory / "src/primitive.rs")
                else:
                    path.write_bytes(b"unexpected source")
                with self.assertRaisesRegex(ValueError, "Symlink|Unreviewed imported"):
                    release.check_override_sources()
    def test_sdk_inheritance_must_be_direct_path_not_root_patch_or_registry(self):
        for declaration in ('[dependencies]\nopenmls="=0.9.2"\n',
                            '[patch.crates-io]\nopenmls={path="third_party/overrides/openmls-0.9.2"}\n'):
            with self.subTest(declaration=declaration), self.recorded_sdk() as (root, _, _, _):
                (root / "Cargo.toml").write_text(declaration)
                with self.assertRaisesRegex(ValueError, "SDK dependency must directly"):
                    release.check_override_sources()

    def test_resolved_graph_cannot_relabel_registry_or_foreign_path_as_override(self):
        for field, value in (("source", "registry+https://example.test/index"),
                             ("manifest_path", "/foreign/openmls/Cargo.toml"), ("license", "Apache-2.0")):
            with self.subTest(field=field), self.recorded_sdk() as (_, _, metadata, _):
                metadata["packages"][1][field] = value
                with self.assertRaisesRegex(ValueError, "bypasses|license differs"):
                    release.licenses()

    def test_original_notice_must_be_included_and_bound_to_recorded_revision(self):
        for mutation in ("missing", "revision"):
            with self.subTest(mutation=mutation), self.recorded_sdk() as (root, _, _, extra):
                if mutation == "missing":
                    extra["packages"]["openmls@0.9.2"] = []
                else:
                    extra["sources"]["licenses/upstream/LICENSE"]["revision"] = "b" * 40
                (root / "third_party/notices.json").write_text(json.dumps(extra))
                with self.assertRaisesRegex(ValueError, "original override notice"):
                    release.licenses()

    def test_offline_sdk_source_requires_complete_path_dependency_and_legal_inputs(self):
        with self.recorded_sdk() as (root, _, _, _):
            notices, _ = release.licenses()
            sources = {path.relative_to(root).as_posix(): (path.read_bytes(), 0o644)
                       for path in root.rglob("*") if path.is_file()}
            release.check_override_source_files(sources, notices)
            for missing in ("third_party/overrides/openmls-0.9.2/src/replay.rs", "third_party/licenses/upstream/LICENSE"):
                with self.subTest(missing=missing), self.assertRaisesRegex(ValueError, "Offline SDK source missing"):
                    release.check_override_source_files({key: value for key, value in sources.items() if key != missing}, notices)
            replay = root / "third_party/overrides/openmls-0.9.2/src/replay.rs"
            replay.write_bytes(b"changed after initial notice collection")
            sources[replay.relative_to(root).as_posix()] = (replay.read_bytes(), 0o644)
            with self.assertRaisesRegex(ValueError, "Reviewed override file changed"):
                release.check_override_source_files(sources, notices)

    def test_target_specific_sdk_and_package_root_license_preserve_original_blob(self):
        with self.recorded_sdk() as (root, directory, _, _):
            ledger_path = root / "third_party/overrides/sources.json"
            ledger = json.loads(ledger_path.read_text())
            origin = ledger["packages"]["openmls@0.9.2"]
            origin["sdk_target"] = "cfg(windows)"
            (root / "Cargo.toml").write_text('[target.\'cfg(windows)\'.dependencies]\nopenmls={version="=0.9.2",path="third_party/overrides/openmls-0.9.2"}\n')
            data = b"original upstream fixture notice"
            (directory / "LICENSE").write_bytes(data)
            origin["required_notices"] = ["LICENSE"]
            origin["reviewed_files"]["LICENSE"] = release.digest(data)
            origin["license_git_blob"] = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data, usedforsecurity=False).hexdigest()
            origin["upstream_tree_sha256"] = release.override_tree_digest(directory, origin["reviewed_files"])
            ledger_path.write_text(json.dumps(ledger))
            notices, files = release.licenses()
            self.assertIn("licenses/openmls-0.9.2/LICENSE", notices[0]["notices"])
            self.assertEqual(files["licenses/openmls-0.9.2/LICENSE"][0], data)
            sources = {path.relative_to(root).as_posix(): (path.read_bytes(), 0o644)
                       for path in root.rglob("*") if path.is_file()}
            release.check_override_source_files(sources, notices)
            origin["license_git_blob"] = "b" * 40
            ledger_path.write_text(json.dumps(ledger))
            with self.assertRaisesRegex(ValueError, "upstream Git blob"):
                release.licenses()


class ReleaseTests(unittest.TestCase):
    def test_release_tag_pushes_only_selected_authorized_ssh_remotes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text('[package]\nversion="1.2.3"\n')
            answers = {
                ("git", "status", "--porcelain"): "",
                ("git", "rev-list", "-n", "1", "v1.2.3"): "commit",
                ("git", "rev-parse", "HEAD"): "commit",
                ("git", "remote", "get-url", "github"): "git@github.com:kpihx-labs/kk.git",
            }
            with mock.patch.object(release, "ROOT", root), mock.patch.object(
                release, "command", side_effect=lambda *args: answers[args]
            ), mock.patch.object(release.subprocess, "run") as run:
                release.release_tag(push=True, remotes=["github"])
            run.assert_called_once_with(
                ["git", "push", "github", "refs/tags/v1.2.3"], cwd=root, check=True
            )

        with self.assertRaisesRegex(ValueError, "selected authorized remotes"):
            release.release_tag(push=True, remotes=["origin"])

    def test_imported_primitive_changes_are_rejected_before_packaging(self):
        release.check_override_sources()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            base = root / "third_party/overrides"
            base.mkdir(parents=True)
            (base / "primitive.rs").write_bytes(b"changed")
            (base / "sources.json").write_text(json.dumps({
                "packages": {}, "unchanged_primitives": {"primitive.rs": release.digest(b"original")},
            }))
            with mock.patch.object(release, "ROOT", root), self.assertRaisesRegex(ValueError, "primitive changed"):
                release.check_override_sources()

    def test_local_dependency_overrides_are_not_omitted_from_notices(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            base = root / "third_party/overrides"
            dependency = base / "fixture"
            dependency.mkdir(parents=True)
            (dependency / "LICENSE").write_bytes(b"public fixture notice")
            expected = {"fixture@1.0.0": {"changes": ["test source override"]}}
            manifest = base / "sources.json"
            manifest.write_text(json.dumps({"packages": expected, "unchanged_primitives": {}}))
            metadata = {"resolve": {"root": "root"}, "packages": [
                {"id": "root", "name": "root", "version": "1.0.0", "source": None},
                {"id": "fixture", "name": "fixture", "version": "1.0.0", "source": None,
                 "license": "MIT", "manifest_path": str(dependency / "Cargo.toml")},
            ]}
            with mock.patch.object(release, "ROOT", root), mock.patch.object(
                release, "command", return_value=json.dumps(metadata),
            ):
                notices, files = release.licenses()
                self.assertEqual(len(notices), 1)
                self.assertEqual(notices[0]["name"], "fixture")
                self.assertEqual(notices[0]["override"], expected["fixture@1.0.0"])
                self.assertEqual(files[notices[0]["notices"][0]][0], b"public fixture notice")
                manifest.write_text(json.dumps({"packages": {}, "unchanged_primitives": {}}))
                with self.assertRaisesRegex(ValueError, "Unrecorded local dependency"):
                    release.licenses()

    def test_installation_keeps_foreign_cli_and_prior_version_bytes_intact(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "binary"
            binary.write_bytes(b"owned binary")
            prefix = root / "prefix"
            (prefix / "bin").mkdir(parents=True)
            foreign = prefix / "bin/kk"
            foreign.write_bytes(b"foreign")
            with self.assertRaisesRegex(ValueError, "unrelated"):
                release.install_binary(binary, prefix, "0.8.0")
            self.assertEqual(foreign.read_bytes(), b"foreign")
            foreign.unlink()
            link = release.install_binary(binary, prefix, "0.8.0")
            self.assertEqual(link.read_bytes(), b"owned binary")
            binary.write_bytes(b"new bytes")
            with self.assertRaisesRegex(ValueError, "different bytes"):
                release.install_binary(binary, prefix, "0.8.0")
            self.assertEqual(link.read_bytes(), b"owned binary")

    def test_matrix_files_cannot_silently_replace_a_different_source_archive(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ["one", "two", "output"]:
                (root / name).mkdir()
            (root / "one/source.tar.gz").write_bytes(b"source")
            (root / "two/source.tar.gz").write_bytes(b"source")
            release.collect(root / "one", root / "output")
            release.collect(root / "two", root / "output")
            (root / "two/source.tar.gz").write_bytes(b"different")
            with self.assertRaisesRegex(ValueError, "conflict"):
                release.collect(root / "two", root / "output")
            self.assertEqual((root / "output/source.tar.gz").read_bytes(), b"source")

    def fixture(self, path, changed=False):
        data = b"owned binary fixture"
        manifest = {"format": 1, "version": "0.8.0", "target": "x86_64-unknown-linux-gnu", "files": {"kk": release.digest(data)}}
        release.archive(path, "kk-fixture", {"kk": (data + b"changed" if changed else data, 0o755),
                                             "build-manifest.json": (json.dumps(manifest).encode(), 0o644)}, 1)

    def test_manifest_checks_every_file_and_does_not_claim_signer_authenticity(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "release.tar.gz"
            self.fixture(path)
            result = release.verify(path, release.digest(path.read_bytes()))
            self.assertTrue(result["ok"])
            self.assertIn("attestation", result["authenticity"])
            with self.assertRaises(ValueError):
                release.verify(path, "0" * 64)
            with self.assertRaises(FileExistsError):
                self.fixture(path)

    def test_changed_bytes_never_pass_the_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "changed.tar.gz"
            self.fixture(path, changed=True)
            with self.assertRaises(ValueError):
                release.verify(path)

    def test_traversal_links_and_duplicate_members_are_refused(self):
        for names, link in [(["root/../escape"], False), (["root/a"], True), (["root/a", "root/a"], False), (["root/./a"], False)]:
            with self.subTest(names=names, link=link), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "unsafe.tar.gz"
                with tarfile.open(path, "w:gz") as archive:
                    for name in names:
                        member = tarfile.TarInfo(name)
                        if link:
                            member.type, member.linkname = tarfile.SYMTYPE, "/outside"
                            archive.addfile(member)
                        else:
                            member.size = 1
                            archive.addfile(member, io.BytesIO(b"x"))
                with self.assertRaises(ValueError):
                    release.verify(path)

    def test_archive_limits_apply_before_payload_allocation(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "bounded.tar.gz"
            self.fixture(path)
            with mock.patch.object(release, "MAX_UNPACKED_BYTES", 1), self.assertRaises(ValueError):
                release.verify(path)

    def test_pax_metadata_cannot_request_unbounded_gzip_allocation(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "oversized-pax.tar.gz"
            header = tarfile.TarInfo("root/metadata")
            header.type, header.size = tarfile.XHDTYPE, 16 * 1024 * 1024
            path.write_bytes(release.gzip.compress(header.tobuf() + b"x"))
            original = release.gzip.GzipFile.read

            def bounded_read(stream, size=-1):
                self.assertGreaterEqual(size, 0)
                self.assertLessEqual(size, 1025)
                return original(stream, size)

            with (
                mock.patch.object(release, "MAX_UNPACKED_BYTES", 1024),
                mock.patch.object(release.gzip.GzipFile, "read", bounded_read),
                self.assertRaises((ValueError, tarfile.ReadError)),
            ):
                release.verify(path)

    def test_gzip_expansion_is_bounded_before_tar_metadata_parse(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "expansion.tar.gz"
            path.write_bytes(release.gzip.compress(b"x" * 2048))
            with mock.patch.object(release, "MAX_UNPACKED_BYTES", 1024), self.assertRaisesRegex(ValueError, "Unpacked"):
                release.verify(path)

    def test_dirty_sources_and_unqualified_targets_cannot_create_release_archives(self):
        with mock.patch.object(release, "command", return_value=" M src/owned.rs"), self.assertRaisesRegex(ValueError, "clean committed"):
            release.bundle("x86_64-unknown-linux-gnu", Path("unused"))
        with self.assertRaisesRegex(ValueError, "Unsupported native"):
            release.bundle("x86_64-pc-windows-msvc", Path("unused"))

    def test_zip_is_reproducible_and_preserves_tar_contents_and_modes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tar, first, second = [root / name for name in ("one.tar.gz", "one.zip", "two.zip")]
            for path in (tar, first, second):
                self.fixture(path)
            self.assertEqual(first.read_bytes(), second.read_bytes())
            self.assertEqual(release.archive_entries(tar), release.archive_entries(first))
            self.assertEqual(release.verify(tar), release.verify(first))
            with zipfile.ZipFile(first) as archive:
                binary = archive.getinfo("kk-fixture/kk")
                self.assertEqual(binary.external_attr >> 16, stat.S_IFREG | 0o755)
                self.assertEqual(binary.date_time, (1980, 1, 1, 0, 0, 0))
            self.assertTrue(release.verify(first, release.digest(first.read_bytes()))["ok"])
            with self.assertRaisesRegex(ValueError, "trusted digest"):
                release.verify(first, "0" * 64)
            with self.assertRaises(FileExistsError):
                self.fixture(first)
            changed = root / "changed.zip"
            self.fixture(changed, changed=True)
            with self.assertRaisesRegex(ValueError, "differ from"):
                release.verify(changed)

    def raw_archive(self, path, names, mode=stat.S_IFREG | 0o644, data=b"x"):
        if path.suffix == ".zip":
            with warnings.catch_warnings(), zipfile.ZipFile(path, "w", compression=zipfile.ZIP_DEFLATED) as archive:
                warnings.simplefilter("ignore", UserWarning)
                for name in names:
                    info = zipfile.ZipInfo(name)
                    info.create_system = 3
                    info.external_attr = mode << 16
                    info.compress_type = zipfile.ZIP_DEFLATED
                    archive.writestr(info, data)
        else:
            with tarfile.open(path, "w:gz") as archive:
                for name in names:
                    member = tarfile.TarInfo(name)
                    member.size = len(data)
                    archive.addfile(member, io.BytesIO(data))

    def test_both_formats_reject_ambiguous_paths_before_manifest_checks(self):
        cases = [
            ["root/../escape"], ["/root/a"], ["root/./a"], ["root//a"],
            ["root/\\escape"], ["root/C:escape"], ["root/a\x01"], ["root/a."],
            ["root/a", "root/a"], ["root/A", "root/a"],
            ["root/A/x", "root/a/y"], ["root/a", "root/a/b"],
            ["root/a/b", "root/a"], ["root/a", "other/b"],
            ["root/" + "x" * release.MAX_PATH_BYTES],
            ["root/" + "a/" * release.MAX_PATH_DEPTH + "x"],
        ]
        for extension in (".tar.gz", ".zip"):
            for names in cases:
                with self.subTest(extension=extension, names=names), tempfile.TemporaryDirectory() as directory:
                    path = Path(directory) / ("unsafe" + extension)
                    self.raw_archive(path, names)
                    with self.assertRaises(ValueError):
                        release.archive_entries(path)

    def test_zip_rejects_links_special_files_and_directories(self):
        for mode in (stat.S_IFLNK, stat.S_IFIFO, stat.S_IFCHR, stat.S_IFBLK, stat.S_IFSOCK, stat.S_IFDIR, stat.S_IFREG | 0o4755):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "unsafe.zip"
                self.raw_archive(path, ["root/a"], mode)
                with self.assertRaisesRegex(ValueError, "non-ordinary"):
                    release.archive_entries(path)

    def test_zip_budgets_precede_payload_reads(self):
        for limit, value, names, data in [
            ("MAX_MEMBER_BYTES", 1, ["root/a"], b"xx"),
            ("MAX_MANIFEST_BYTES", 512, ["root/build-manifest.json"], b"x" * 513),
            ("MAX_UNPACKED_BYTES", 300, ["root/a", "root/b"], b"x" * 200),
            ("MAX_MEMBERS", 1, ["root/a", "root/b"], b"x"),
            ("MAX_UNPACKED_BYTES", 1024, ["root/a"], b"x" * 8192),
        ]:
            with self.subTest(limit=limit, value=value), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "bounded.zip"
                self.raw_archive(path, names, data=data)
                with mock.patch.object(release, limit, value), self.assertRaises(ValueError):
                    release.verify(path)

    def test_zip_directory_is_bounded_before_zipfile_allocation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            original = root / "original.zip"
            self.fixture(original)
            data = original.read_bytes()
            end = data.rfind(b"PK\x05\x06")
            for field, value in [(8, 0), (10, 0), (12, release.MAX_MANIFEST_BYTES + 1), (4, 1)]:
                with self.subTest(field=field):
                    changed = bytearray(data)
                    struct.pack_into("<L" if field == 12 else "<H", changed, end + field, value)
                    path = root / f"bad-{field}.zip"
                    path.write_bytes(changed)
                    with mock.patch.object(release.zipfile, "ZipFile", side_effect=AssertionError("unbounded parse")), self.assertRaises(ValueError):
                        release.verify(path)

    def test_archive_path_symlinks_are_rejected_for_both_formats(self):
        for extension in (".tar.gz", ".zip"):
            with self.subTest(extension=extension), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / ("original" + extension)
                self.fixture(path)
                link = path.with_name("link" + extension)
                link.symlink_to(path)
                with self.assertRaisesRegex(ValueError, "non-ordinary"):
                    release.verify(link)

    def test_hash_and_parse_use_the_same_snapshot_despite_replacement(self):
        for extension in (".tar.gz", ".zip"):
            for replacement in ("pathname", "in-place"):
                with self.subTest(extension=extension, replacement=replacement), tempfile.TemporaryDirectory() as directory:
                    path = Path(directory) / ("original" + extension)
                    self.fixture(path)
                    expected = release.digest(path.read_bytes())
                    untrusted = path.with_name("replacement" + extension)
                    self.fixture(untrusted, changed=True)
                    untrusted_bytes = untrusted.read_bytes()
                    inode = path.stat().st_ino
                    original_hash = release.hashlib.file_digest

                    def replace_after_hash(source, algorithm, path=path, replacement=replacement,
                                           untrusted=untrusted, untrusted_bytes=untrusted_bytes,
                                           inode=inode, original_hash=original_hash):
                        result = original_hash(source, algorithm)
                        if replacement == "pathname":
                            untrusted.replace(path)
                        else:
                            path.write_bytes(untrusted_bytes)
                            self.assertEqual(path.stat().st_ino, inode)
                        return result

                    with mock.patch.object(release.hashlib, "file_digest", side_effect=replace_after_hash) as hashing:
                        result = release.verify(path, expected)
                    hashing.assert_called_once()
                    self.assertTrue(result["ok"])
                    self.assertEqual(path.read_bytes(), untrusted_bytes)
                    # The replacement really is invalid and was not parsed above.
                    with self.assertRaisesRegex(ValueError, "differ from"):
                        release.verify(path)

    def test_snapshot_rejects_special_descriptors_without_blocking(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fifo = root / "fifo.zip"
            release.os.mkfifo(fifo)
            for path in (root, fifo):
                with self.subTest(path=path), self.assertRaisesRegex(ValueError, "non-ordinary"):
                    release.verify(path)

    def test_snapshot_read_stays_bounded_if_descriptor_size_is_stale(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "grown.zip"
            path.write_bytes(b"x" * 2048)
            with (
                mock.patch.object(release, "MAX_UNPACKED_BYTES", 1024),
                mock.patch.object(release.os, "fstat", return_value=mock.Mock(st_mode=stat.S_IFREG, st_size=1)),
                self.assertRaisesRegex(ValueError, "oversized"),
            ):
                release.verify(path)

    def test_bundle_emits_both_formats_and_checksums_for_existing_targets(self):
        # Mocked build/vendor inputs exercise packaging, not architecture qualification.
        for target, label in release.TARGETS.items():
            with self.subTest(target=target), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                binary = root / "target" / target / "release/kk"
                binary.parent.mkdir(parents=True)
                binary.write_bytes(b"fixture executable")
                (root / "Cargo.toml").write_text('[package]\nversion = "0.8.0"\n')
                for name in ("README.md", "LICENSE", "CONTRACT.md", "Cargo.lock"):
                    (root / name).write_bytes(b"fixture " + name.encode())
                (root / "scripts").mkdir()
                for name in ("install.sh", "install.ps1"):
                    (root / "scripts" / name).write_bytes((release.ROOT / "scripts" / name).read_bytes())

                def command(*args, binary=binary):
                    return {
                        ("git", "status", "--porcelain"): "",
                        ("git", "show", "-s", "--format=%ct", "HEAD"): "1",
                        ("git", "rev-parse", "HEAD"): "fixture-commit",
                        ("readelf", "--version-info", str(binary)): "GLIBC_2.17",
                        ("rustc", "--version"): "fixture rustc",
                        ("ldd", "--version"): "fixture libc",
                    }[args]

                destination = root / "dist"
                with (
                    mock.patch.object(release, "ROOT", root),
                    mock.patch.object(release, "command", side_effect=command),
                    mock.patch.object(release, "macos_minimum", return_value="13.0.0"),
                    mock.patch.object(release, "licenses", return_value=(
                        [{"name": "fixture", "notices": ["licenses/fixture/LICENSE"]}],
                        {"licenses/fixture/LICENSE": (b"fixture notice", 0o644)},
                    )),
                    mock.patch.object(release.subprocess, "run"),
                    mock.patch.object(release.subprocess, "check_output", return_value=b"README.md\0"),
                    mock.patch.dict(release.os.environ, {}, clear=True),
                    contextlib.redirect_stdout(io.StringIO()) as output,
                ):
                    release.bundle(target, destination)
                report = json.loads(output.getvalue())
                stem = f"kk-v0.8.0-{label}"
                self.assertEqual(Path(report["archive"]).name, stem + ".tar.gz")
                self.assertEqual(Path(report["zip_archive"]).name, stem + ".zip")
                tar_entries = release.archive_entries(Path(report["archive"]))
                self.assertEqual(tar_entries, release.archive_entries(Path(report["zip_archive"])))
                self.assertIn(stem + "/licenses/fixture/LICENSE", tar_entries[0])
                sums = (destination / (stem + ".sha256")).read_text().splitlines()
                self.assertEqual(len(sums), 8)
                self.assertEqual([Path(path).name for path in report["bootstrap_entrypoints"]],
                                 ["install.sh", "install.ps1"])
                for path in report["bootstrap_entrypoints"]:
                    self.assertEqual(Path(path).read_bytes(), (root / "scripts" / Path(path).name).read_bytes())
                helper = Path(report["bootstrap_helper"])
                companion = Path(report["bootstrap_notices"])
                self.assertEqual(helper.name, stem + "-bootstrap.bin")
                self.assertEqual(companion.name, stem + "-bootstrap-notices.json")
                self.assertEqual(helper.read_bytes(), binary.read_bytes())
                record = json.loads(companion.read_bytes())
                self.assertEqual(record["target"], target)
                self.assertEqual(record["format"], 1)
                self.assertEqual(record["executable_format"], "macho" if target.endswith("-apple-darwin") else "elf")
                manifest = json.loads(record["files"]["build-manifest.json"])
                if target.endswith("-apple-darwin"):
                    self.assertEqual(manifest["binary_macos_required"], "13.0.0")
                    self.assertIsNone(manifest["binary_glibc_required"])
                    self.assertIsNone(manifest["build_system_libc"])
                self.assertEqual(record["helper"], {"asset": helper.name, "sha256": release.digest(binary.read_bytes())})
                self.assertEqual(record["files"]["licenses/fixture/LICENSE"], "fixture notice")
                self.assertEqual(record["files"]["LICENSE"], "fixture LICENSE")
                self.assertEqual(json.loads(record["files"]["THIRD-PARTY.json"])[0]["name"], "fixture")
                self.assertEqual({name: text.encode("utf-8") for name, text in record["files"].items()},
                                 {name.removeprefix(stem + "/"): data for name, data in tar_entries[0].items()
                                  if name != stem + "/kk"})
                for line in sums:
                    checksum, name = line.split("  ")
                    self.assertEqual(checksum, release.digest((destination / name).read_bytes()))

    def test_macos_deployment_metadata_uses_native_tools_not_candidate_execution(self):
        output = "Load command 9\n      cmd LC_BUILD_VERSION\n  cmdsize 32\n platform 1\n    minos 13.0\n      sdk 14.1\n"
        with mock.patch.object(release.platform, "system", return_value="Darwin"), \
                mock.patch.object(release, "command", return_value=output) as command:
            self.assertEqual(release.macos_minimum(Path("candidate")), "13.0.0")
        self.assertEqual(command.call_args_list[0].args,
                         ("/usr/bin/codesign", "--verify", "--strict", "candidate"))
        self.assertEqual(command.call_args_list[1].args, ("/usr/bin/otool", "-l", "candidate"))
        with mock.patch.object(release.platform, "system", return_value="Darwin"), \
                mock.patch.object(release, "command", return_value=output.replace("platform 1", "platform 2")), \
                self.assertRaisesRegex(ValueError, "not macOS"):
            release.macos_minimum(Path("candidate"))
        with mock.patch.object(release.platform, "system", return_value="Linux"), \
                mock.patch.object(release, "command") as command, self.assertRaisesRegex(ValueError, "native Apple"):
            release.macos_minimum(Path("candidate"))
        command.assert_not_called()

    @contextlib.contextmanager
    def bundle_fixture(self):
        # Actual deterministic archive bytes with MOCKED build/vendor/source inputs.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = "x86_64-unknown-linux-gnu"
            binary = root / "target" / target / "release/kk"
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b"fixture executable")
            (root / "Cargo.toml").write_text('[package]\nversion = "0.8.0"\n')
            for name in ("README.md", "LICENSE", "CONTRACT.md", "Cargo.lock"):
                (root / name).write_bytes(b"fixture " + name.encode())
            (root / "scripts").mkdir()
            for name in ("install.sh", "install.ps1"):
                (root / "scripts" / name).write_bytes(b"fixture reviewed " + name.encode())
            commands = {
                ("git", "status", "--porcelain"): "",
                ("git", "show", "-s", "--format=%ct", "HEAD"): "1",
                ("git", "rev-parse", "HEAD"): "a" * 40,
                ("readelf", "--version-info", str(binary)): "GLIBC_2.17",
                ("rustc", "--version"): "fixture rustc",
                ("ldd", "--version"): "fixture libc",
            }
            with (
                mock.patch.object(release, "ROOT", root),
                mock.patch.object(release, "command", side_effect=lambda *args: commands[args]),
                mock.patch.object(release, "licenses", side_effect=lambda: (
                    [{"name": "fixture", "notices": ["licenses/fixture/LICENSE"]}],
                    {"licenses/fixture/LICENSE": (b"Exact upstream author notice\n", 0o644)},
                )),
                mock.patch.object(release.subprocess, "run"),
                mock.patch.object(release.subprocess, "check_output", return_value=b"README.md\0"),
                mock.patch.dict(release.os.environ, {}, clear=True),
            ):
                yield target, root / "dist", commands

    def test_public_bundle_retry_recovers_every_publication_boundary(self):
        # Includes both archive links, helper/notices, entrypoints, source and sums.
        for failure_index in range(9):
            with self.subTest(failure_index=failure_index), self.bundle_fixture() as (target, destination, _):
                original_link = release.os.link
                attempted = []

                def fail_publication(source, path, *, attempted=attempted,
                                     failure_index=failure_index, original_link=original_link):
                    if path.parent == destination:
                        attempted.append(path.name)
                        if len(attempted) == failure_index + 1:
                            raise OSError("injected public bundle interruption")
                    original_link(source, path)

                with contextlib.redirect_stdout(io.StringIO()) as output:
                    with mock.patch.object(release.os, "link", side_effect=fail_publication), self.assertRaisesRegex(
                        RuntimeError, "Bundle publication incomplete.*identical inputs",
                    ):
                        release.bundle(target, destination)
                    self.assertEqual(output.getvalue(), "")
                    prior = {path.name: (path.read_bytes(), path.stat().st_ino, path.stat().st_mtime_ns)
                             for path in destination.iterdir() if path.is_file()}
                    self.assertEqual(len(prior), failure_index)
                    release.bundle(target, destination)
                    report = json.loads(output.getvalue())
                for name, before in prior.items():
                    path = destination / name
                    self.assertEqual((path.read_bytes(), path.stat().st_ino, path.stat().st_mtime_ns), before)
                sums = dict(line.split("  ")[::-1] for line in Path(report["checksums"]).read_text().splitlines())
                self.assertEqual(len(sums), 8)
                self.assertEqual({path.name for path in destination.iterdir() if path.is_file()},
                                 set(sums) | {Path(report["checksums"]).name})
                for name, checksum in sums.items():
                    self.assertEqual(release.digest((destination / name).read_bytes()), checksum)
                for key in ("archive", "zip_archive"):
                    path = Path(report[key])
                    self.assertTrue(release.verify(path, sums[path.name])["ok"])
                    with self.assertRaisesRegex(ValueError, "trusted digest"):
                        release.verify(path, "0" * 64)
                record = json.loads(Path(report["bootstrap_notices"]).read_bytes())
                self.assertEqual(record["helper"]["sha256"], sums[Path(report["bootstrap_helper"]).name])
                entries, prefix = release.archive_entries(Path(report["archive"]))
                self.assertEqual({name: text.encode("utf-8") for name, text in record["files"].items()},
                                 {name.removeprefix(prefix + "/"): data for name, data in entries.items()
                                  if name != prefix + "/kk"})
                self.assertEqual(record["files"]["licenses/fixture/LICENSE"], "Exact upstream author notice\n")

    def test_public_bundle_retry_after_private_bootstrap_failure(self):
        for failure in ("write", "link"):
            with self.subTest(failure=failure), self.bundle_fixture() as (target, destination, _):
                original_open, original_link = Path.open, release.os.link

                def fail_notice_write(path, *args, original_open=original_open, **kwargs):
                    if path.name.endswith("-bootstrap-notices.json") and args == ("xb",):
                        raise OSError("injected private notice write failure")
                    return original_open(path, *args, **kwargs)

                def fail_notice_link(source, path, *, original_link=original_link):
                    if path.name.endswith("-bootstrap-notices.json"):
                        raise OSError("injected private notice link failure")
                    original_link(source, path)

                patch = (mock.patch.object(Path, "open", fail_notice_write) if failure == "write"
                         else mock.patch.object(release.os, "link", side_effect=fail_notice_link))
                with contextlib.redirect_stdout(io.StringIO()) as output:
                    with patch, self.assertRaisesRegex(RuntimeError, "stage retained"):
                        release.bundle(target, destination)
                    self.assertEqual(output.getvalue(), "")
                    self.assertFalse(any(path.is_file() for path in destination.iterdir()))
                    retained = list(destination.glob(".kk-bundle-*"))
                    self.assertEqual(len(retained), 1)
                    release.bundle(target, destination)
                    report = json.loads(output.getvalue())
                self.assertTrue(retained[0].is_dir())
                self.assertTrue(release.verify(Path(report["archive"]))["ok"])
                self.assertEqual(len([path for path in destination.iterdir() if path.is_file()]), 9)

    def test_bundle_retry_recovers_second_bootstrap_link_failure_without_replacing_archives(self):
        with self.bundle_fixture() as (target, destination, _):
            original_link = release.os.link

            def fail_second_bootstrap(source, path):
                if path.parent == destination and path.name.endswith("-bootstrap-notices.json"):
                    raise OSError("injected second public bootstrap link failure")
                original_link(source, path)

            with contextlib.redirect_stdout(io.StringIO()) as output:
                with mock.patch.object(release.os, "link", side_effect=fail_second_bootstrap), self.assertRaisesRegex(
                    RuntimeError, "Bundle publication incomplete.*rerun the same bundle command",
                ):
                    release.bundle(target, destination)
                self.assertEqual(output.getvalue(), "")
                prior = {path.name: (path.read_bytes(), path.stat()) for path in destination.iterdir() if path.is_file()}
                self.assertEqual(len(prior), 4)  # TAR, ZIP, build manifest, helper.
                self.assertTrue(any(name.endswith(".tar.gz") for name in prior))
                self.assertTrue(any(name.endswith(".zip") for name in prior))
                release.bundle(target, destination)
                report = json.loads(output.getvalue())
            for name, (data, before) in prior.items():
                path = destination / name
                self.assertEqual(path.read_bytes(), data)
                self.assertEqual((path.stat().st_ino, path.stat().st_mtime_ns), (before.st_ino, before.st_mtime_ns))
            self.assertTrue(Path(report["bootstrap_notices"]).is_file())
            self.assertEqual(json.loads(Path(report["bootstrap_notices"]).read_bytes())["commit"], "a" * 40)
            for line in Path(report["checksums"]).read_text().splitlines():
                checksum, name = line.split("  ")
                self.assertEqual(release.digest((destination / name).read_bytes()), checksum)
            complete = {path.name: (path.read_bytes(), path.stat().st_ino) for path in destination.iterdir() if path.is_file()}
            with contextlib.redirect_stdout(io.StringIO()):
                release.bundle(target, destination)
            self.assertEqual(complete, {path.name: (path.read_bytes(), path.stat().st_ino)
                                        for path in destination.iterdir() if path.is_file()})

    def test_bundle_retry_preflights_every_existing_member_and_refuses_foreign_links(self):
        with self.bundle_fixture() as (target, baseline, _):
            with contextlib.redirect_stdout(io.StringIO()):
                release.bundle(target, baseline)
            expected = {path.name: path.read_bytes() for path in baseline.iterdir() if path.is_file()}
            self.assertEqual(len(expected), 9)
            companion = next(name for name in expected if name.endswith("-bootstrap-notices.json"))
            for index, name in enumerate(expected):
                for link in (False, True):
                    with self.subTest(member=name, foreign_link=link):
                        destination = baseline.parent / f"conflict-{index}-{link}"
                        destination.mkdir()
                        for member, data in expected.items():
                            if member == name or member == companion:
                                continue
                            (destination / member).write_bytes(data)
                        foreign = destination / name
                        if link:
                            foreign.symlink_to(baseline / name)  # Even equal bytes must not authorize links.
                        else:
                            foreign.write_bytes(b"foreign conflicting bytes")
                        before = {path.name: (path.is_symlink(), path.lstat().st_ino) for path in destination.iterdir()}
                        with contextlib.redirect_stdout(io.StringIO()) as output, self.assertRaisesRegex(
                            FileExistsError, "Different or unsafe release asset",
                        ):
                            release.bundle(target, destination)
                        self.assertEqual(output.getvalue(), "")
                        self.assertEqual(before, {path.name: (path.is_symlink(), path.lstat().st_ino)
                                                  for path in destination.iterdir() if not path.is_dir()})
                        if not link:
                            self.assertEqual(foreign.read_bytes(), b"foreign conflicting bytes")

    def test_bundle_retry_refuses_changed_commit_selector_before_filling_missing_pair(self):
        with self.bundle_fixture() as (target, destination, commands):
            original_link = release.os.link

            def fail_second_bootstrap(source, path):
                if path.parent == destination and path.name.endswith("-bootstrap-notices.json"):
                    raise OSError("injected second public bootstrap link failure")
                original_link(source, path)

            with mock.patch.object(release.os, "link", side_effect=fail_second_bootstrap), self.assertRaises(RuntimeError):
                release.bundle(target, destination)
            prior = {path.name: path.read_bytes() for path in destination.iterdir() if path.is_file()}
            commands[("git", "rev-parse", "HEAD")] = "b" * 40
            with self.assertRaises(FileExistsError), contextlib.redirect_stdout(io.StringIO()) as output:
                release.bundle(target, destination)
            self.assertEqual(output.getvalue(), "")
            self.assertEqual(prior, {path.name: path.read_bytes() for path in destination.iterdir() if path.is_file()})
            self.assertFalse(list(destination.glob("*-bootstrap-notices.json")))

    def test_bootstrap_missing_notices_and_existing_assets_fail_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory)
            files = {"kk": (b"fixture", 0o755), "LICENSE": (b"fixture license", 0o644),
                     "THIRD-PARTY.json": (b'[{"notices": []}]', 0o644)}
            args = ("x86_64-unknown-linux-gnu", "0.8.0", "a" * 40, files, destination)
            with self.assertRaisesRegex(ValueError, "upstream notice inventory"):
                release.bootstrap_assets(*args)
            self.assertEqual(list(destination.iterdir()), [])
            files["THIRD-PARTY.json"] = (b'[]', 0o644)
            with mock.patch.object(release, "MAX_MEMBER_BYTES", 1), self.assertRaisesRegex(ValueError, "Oversized"):
                release.bootstrap_assets(*args)
            self.assertEqual(list(destination.iterdir()), [])
            paths = release.bootstrap_assets(*args)
            original = [path.read_bytes() for path in paths]
            self.assertEqual(release.bootstrap_assets(*args), paths)
            self.assertEqual([path.read_bytes() for path in paths], original)
            paths[1].write_bytes(b"foreign version bytes")
            with self.assertRaises(FileExistsError):
                release.bootstrap_assets(*args)
            self.assertEqual(paths[1].read_bytes(), b"foreign version bytes")
            files["LICENSE"] = (b'\xff', 0o644)
            with self.assertRaises(UnicodeDecodeError):
                release.bootstrap_assets(*args)
            del files["LICENSE"]
            with self.assertRaisesRegex(ValueError, "project license"):
                release.bootstrap_assets(*args)

    def test_notice_union_retains_top_license_and_curated_nested_author_text(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            dependency = root / "package"
            dependency.mkdir()
            (dependency / "LICENSE").write_bytes(b"Top license, exact author text\n")
            nested = root / "third_party/collected/nested-NOTICE"
            nested.parent.mkdir(parents=True)
            author_text = b"Nested notice, objectionable author retained verbatim\n"
            nested.write_bytes(author_text)
            relative = "collected/nested-NOTICE"
            (root / "third_party/notices.json").write_text(json.dumps({
                "packages": {"fixture@1": [relative, relative]},
                "sources": {relative: {"sha256": release.digest(author_text)}},
            }))
            metadata = {"resolve": {"root": "root"}, "packages": [
                {"id": "fixture", "name": "fixture", "version": "1", "source": "registry",
                 "license": "MIT", "manifest_path": str(dependency / "Cargo.toml")},
            ]}
            with mock.patch.object(release, "ROOT", root), mock.patch.object(
                release, "check_override_sources", return_value={},
            ), mock.patch.object(release, "command", return_value=json.dumps(metadata)):
                notices, files = release.licenses()
                self.assertEqual(notices[0]["notices"], ["licenses/fixture-1/LICENSE", relative])
                self.assertEqual(files[relative][0], author_text)
                self.assertEqual(files["licenses/fixture-1/LICENSE"][0], (dependency / "LICENSE").read_bytes())
                nested.write_bytes(b" \n")
                with self.assertRaisesRegex(ValueError, "Empty collected notice"):
                    release.licenses()
                nested.write_bytes(author_text)
                (dependency / "LICENSE").write_bytes(b"")
                with self.assertRaisesRegex(ValueError, "Empty dependency notice"):
                    release.licenses()

    def test_bootstrap_empty_declared_notice_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            files = {"kk": (b"fixture", 0o755), "LICENSE": (b"license", 0o644),
                     "THIRD-PARTY.json": (b'[{"notices": ["NOTICE"]}]', 0o644),
                     "NOTICE": (b" \n", 0o644)}
            with self.assertRaisesRegex(ValueError, "empty upstream notices"):
                release.bootstrap_assets("x86_64-unknown-linux-gnu", "0.8.0", "a" * 40, files, Path(directory))

    def test_second_pair_publication_failure_retains_stage_and_recovers_idempotently(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory)
            files = {"kk": (b"fixture", 0o755), "LICENSE": (b"license", 0o644),
                     "THIRD-PARTY.json": (b'[]', 0o644)}
            args = ("x86_64-unknown-linux-gnu", "0.8.0", "a" * 40, files, destination)
            original_link = release.os.link
            calls = []

            def fail_second(source, target):
                calls.append(target)
                if len(calls) == 2:
                    raise OSError("injected second publication failure")
                original_link(source, target)

            with mock.patch.object(release.os, "link", side_effect=fail_second), self.assertRaisesRegex(
                RuntimeError, "Partial release asset publication; complete stage retained",
            ):
                release.bootstrap_assets(*args)
            self.assertEqual(calls[0].read_bytes(), b"fixture")
            self.assertFalse(calls[1].exists())
            stages = list(destination.glob(".kk-assets-*"))
            self.assertEqual(len(stages), 1)
            self.assertEqual(len(list(stages[0].iterdir())), 2)
            paths = release.bootstrap_assets(*args)
            self.assertEqual([path.read_bytes() for path in paths],
                             [(stages[0] / path.name).read_bytes() for path in paths])
            self.assertEqual(release.bootstrap_assets(*args), paths)

    def test_entrypoint_immutable_collision_policy(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory)
            assets = {"install.sh": b"reviewed shell", "install.ps1": b"reviewed powershell"}
            paths = release.publish_assets(assets, destination)
            self.assertEqual(release.publish_assets(assets, destination), paths)
            with self.assertRaises(FileExistsError):
                release.publish_assets({**assets, "install.sh": b"different"}, destination)
            self.assertEqual(paths[0].read_bytes(), assets["install.sh"])

    def test_second_staged_write_failure_never_publishes_partial_pair(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory)
            assets = {"helper.bin": b"helper", "notices.json": b"notices"}
            original_open = Path.open

            def fail_second(path, *args, **kwargs):
                if path.name == "notices.json" and args == ("xb",):
                    raise OSError("injected second write failure")
                return original_open(path, *args, **kwargs)

            with mock.patch.object(Path, "open", fail_second), self.assertRaisesRegex(
                RuntimeError, "Incomplete private release stage retained.*no publication attempted",
            ):
                release.publish_assets(assets, destination)
            self.assertFalse((destination / "helper.bin").exists())
            self.assertFalse((destination / "notices.json").exists())
            stage = next(destination.glob(".kk-assets-*"))
            self.assertEqual((stage / "helper.bin").read_bytes(), b"helper")
            paths = release.publish_assets(assets, destination)
            self.assertEqual([path.read_bytes() for path in paths], list(assets.values()))

    def test_zip_crc_and_manifest_inventory_remain_required(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for names in (["root/a"], ["root/a", "root/build-manifest.json"]):
                path = root / (str(len(names)) + ".zip")
                self.raw_archive(path, names, data=b'{}')
                with self.assertRaises(ValueError):
                    release.verify(path)
            path = root / "crc.zip"
            self.fixture(path)
            data = bytearray(path.read_bytes())
            # Disagree with the actual payload CRC, without changing its size.
            offset = data.index(b"PK\x01\x02")
            data[offset + 16] ^= 1
            path.write_bytes(data)
            with self.assertRaises(zipfile.BadZipFile):
                release.verify(path)


if __name__ == "__main__":
    unittest.main()
