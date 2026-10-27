import contextlib
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


class ReleaseTests(unittest.TestCase):
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
        with self.assertRaisesRegex(ValueError, "qualified Linux"):
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
                self.assertEqual(len(sums), 4)
                for line in sums:
                    checksum, name = line.split("  ")
                    self.assertEqual(checksum, release.digest((destination / name).read_bytes()))

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
