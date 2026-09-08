import hashlib
import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]


def load(name, relative):
    spec = importlib.util.spec_from_file_location(name, ROOT / relative)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


archive = load("reproducible_archive", "tools/reproducible_archive.py")
publish = load("publish_release_assets", "tools/publish_release_assets.py")


class ReproducibleArchiveTests(unittest.TestCase):
    def test_two_clean_mtime_distinct_stages_must_match_before_immutable_publish(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            source.mkdir()
            (source / "payload").write_bytes(b"stable\n")
            output = root / "result.tar.gz"
            mtimes = []

            def fake_container(input_directory, output_directory, member):
                self.assertEqual(member, ".")
                mtimes.append((input_directory / "payload").stat().st_mtime_ns)
                (output_directory / "archive.tar.gz").write_bytes(b"same archive")

            with patch.object(archive, "_run_container", side_effect=fake_container):
                digest = archive.create_archive(source, output)
                self.assertEqual(digest, hashlib.sha256(b"same archive").hexdigest())
                self.assertEqual(len(mtimes), 2)
                self.assertNotEqual(mtimes[0], mtimes[1])
                self.assertEqual(
                    output.with_name("result.tar.gz.sha256").read_text(),
                    f"{digest}  result.tar.gz\n",
                )
                archive.create_archive(source, output)
                output.write_bytes(b"different")
                with self.assertRaises(archive.ArchiveError):
                    archive.create_archive(source, output)

    def test_reproduction_mismatch_and_escaping_symlink_fail_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            source.mkdir()
            (source / "payload").write_text("payload")
            calls = 0

            def divergent(_input, output, _member):
                nonlocal calls
                calls += 1
                (output / "archive.tar.gz").write_bytes(str(calls).encode())

            with patch.object(archive, "_run_container", side_effect=divergent):
                with self.assertRaises(archive.ArchiveError):
                    archive.create_archive(source, root / "different.tar.gz")
            (source / "escape").symlink_to(root)
            with self.assertRaises(archive.ArchiveError):
                archive.create_archive(source, root / "escape.tar.gz")

    def test_container_coordinate_and_archive_flags_are_frozen(self):
        self.assertRegex(
            archive.REPRODUCIBLE_ARCHIVE_IMAGE,
            r"@sha256:[0-9a-f]{64}$",
        )
        source = (ROOT / "tools/reproducible_archive.py").read_text()
        for control in [
            "--pull=always",
            "--network",
            "--read-only",
            "--cap-drop",
            "--sort=name",
            "--mtime=@0",
            "--numeric-owner",
            "gzip -9 -n",
        ]:
            self.assertIn(control, source)


class ImmutablePublicationTests(unittest.TestCase):
    def test_identical_remote_bytes_are_retained_and_different_bytes_fail(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            local = root / "asset.tar.gz"
            local.write_bytes(b"candidate")
            download_root = root / "downloads"
            download_root.mkdir()

            def same(_name, destination):
                shutil.copyfile(local, destination)

            self.assertEqual(
                publish.verify_existing_assets(
                    [local], {local.name}, same, download_root
                ),
                [],
            )

            def different(_name, destination):
                destination.write_bytes(b"tampered")

            with self.assertRaises(publish.PublicationError):
                publish.verify_existing_assets(
                    [local], {local.name}, different, download_root
                )
            self.assertEqual(
                publish.verify_existing_assets(
                    [local], set(), same, download_root
                ),
                [local],
            )

    def test_release_pipeline_has_no_remote_clobber_or_host_tar_fallback(self):
        pipeline = (ROOT / "Jenkinsfile").read_text()
        package = (ROOT / "tools/package_offline_cargo_bundle.sh").read_text()
        self.assertNotIn("gh release upload", pipeline)
        self.assertNotIn("--clobber", pipeline)
        self.assertNotIn("tar --version", package)
        self.assertIn("tools/reproducible_archive.py", package)
        self.assertIn("tools/publish_release_assets.py", pipeline)


if __name__ == "__main__":
    unittest.main()
