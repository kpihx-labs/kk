import importlib.util
import io
import json
import tarfile
import tempfile
import unittest
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
        expected = release.check_override_sources()
        notices, _ = release.licenses()
        actual = {item["name"] + "@" + item["version"]: item for item in notices}
        for key in expected:
            self.assertIn(key, actual)
            self.assertEqual(actual[key]["override"], expected[key])

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


if __name__ == "__main__":
    unittest.main()
