"""Offline cache integration tests using synthetic publication bodies."""

from contextlib import redirect_stdout
import io
import os
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import docs_api
import ibm_docs


class CacheTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.cache = Path(self.directory.name) / "topic-cache"
        self.body = b'<h1 class="topictitle1">Example operation</h1><p>Example rule.</p>'
        self.pin = ibm_docs.Pin("example", "example-v1", "PRODUCT/ref/example.html",
                                "https://example.invalid/topic", docs_api.digest(self.body), len(self.body))
        self.toc = b'{"toc": []}'

    def archive(self, entries):
        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode="w") as archive:
            for name, body, link in entries:
                member = tarfile.TarInfo(name)
                member.size = len(body) if not link else 0
                if link:
                    member.type = tarfile.SYMTYPE
                    member.linkname = link
                archive.addfile(member, io.BytesIO(body) if not link else None)
        stream.seek(0)
        return stream

    def import_entries(self, entries):
        return ibm_docs.import_cache(self.archive(entries), self.cache, [self.pin], {})

    def seed(self):
        docs_api.write_retrieved(self.cache / self.pin.key, self.body)

    def test_environment_default_and_legacy_fallback(self):
        with patch.dict(os.environ, {"MAINFRAME_ENV_IBM_DOCS_CACHE": str(self.cache)}):
            self.assertEqual(docs_api.default_cache(), self.cache)
        with patch.dict(os.environ, {"MAINFRAME_ENV_IBM_DOCS_CACHE": ""}):
            self.assertEqual(docs_api.default_cache(),
                             Path(tempfile.gettempdir()).resolve() / "cobolgrammar/topic-cache")

    def test_repository_destination_is_rejected(self):
        with patch.dict(os.environ, {"MAINFRAME_ENV_IBM_DOCS_CACHE": str(docs_api.REPOSITORY)}):
            with self.assertRaises(docs_api.InsideRepository):
                docs_api.default_cache()
        with self.assertRaises(docs_api.InsideRepository):
            ibm_docs.import_cache(io.BytesIO(), docs_api.REPOSITORY, [], {})

    def test_import_is_verified_and_idempotent_including_toc(self):
        entries = [("./" + self.pin.key, self.body, None), ("toc.json", self.toc, None)]
        for key in ("imported", "already_present"):
            result = ibm_docs.import_cache(self.archive(entries), self.cache, [self.pin],
                                          {"toc.json": docs_api.digest(self.toc)})
            self.assertEqual(result[key], 2)
        self.assertEqual(ibm_docs.cached_body(self.cache, self.pin), self.body)

    def test_import_rejects_wrong_bytes_without_writing(self):
        counts = self.import_entries([(self.pin.key, b"unreviewed", None)])
        self.assertEqual(counts["rejected_mismatch"], 1)
        self.assertFalse((self.cache / self.pin.key).exists())

    def test_import_preserves_existing_conflicts(self):
        docs_api.write_retrieved(self.cache / self.pin.key, b"older cache")
        counts = self.import_entries([(self.pin.key, self.body, None)])
        self.assertEqual(counts["rejected_conflict"], 1)
        self.assertEqual((self.cache / self.pin.key).read_bytes(), b"older cache")

    def test_interrupted_write_does_not_publish_partial_bytes_and_can_retry(self):
        def interrupted(path, body):
            path.write_bytes(body[:3])
            raise OSError("simulated interruption")

        entries = [(self.pin.key, self.body, None)]
        with patch.object(docs_api, "write_retrieved", side_effect=interrupted):
            with self.assertRaisesRegex(OSError, "interruption"):
                self.import_entries(entries)
        self.assertEqual(list(self.cache.iterdir()), [])
        self.assertEqual(self.import_entries(entries)["imported"], 1)

    def test_archive_paths_and_links_are_not_extracted(self):
        entries = [("../" + self.pin.key, self.body, None),
                   ("/" + self.pin.key, self.body, None),
                   (self.pin.key, b"", "../outside"),
                   ("unrecognized.html", self.body, None)]
        self.assertEqual(self.import_entries(entries)["skipped_unrecognized"], 4)
        self.assertFalse(self.cache.exists())

    def test_destination_symlinks_are_refused_for_import_and_read(self):
        self.cache.mkdir()
        target = Path(self.directory.name) / "outside"
        target.write_bytes(self.body)
        (self.cache / self.pin.key).symlink_to(target)
        self.assertEqual(self.import_entries([(self.pin.key, self.body, None)])["rejected_conflict"], 1)
        with self.assertRaisesRegex(ValueError, "symlink"):
            ibm_docs.cached_body(self.cache, self.pin)

    def test_import_bounds_file_and_total_bytes(self):
        entries = [(self.pin.key, self.body, None)]
        for limit in ("MAX_FILE", "MAX_IMPORT"):
            with patch.object(ibm_docs, limit, len(self.body) - 1):
                with self.assertRaisesRegex(ValueError, "bounded"):
                    self.import_entries(entries)
        self.assertFalse(self.cache.exists())

    def test_read_rejects_missing_truncated_and_same_size_corruption(self):
        with self.assertRaises(FileNotFoundError):
            ibm_docs.cached_body(self.cache, self.pin)
        for body in (self.body[:-1], b"X" + self.body[1:]):
            docs_api.write_retrieved(self.cache / self.pin.key, body)
            with self.assertRaisesRegex(ValueError, "mismatch"):
                ibm_docs.cached_body(self.cache, self.pin)

    def test_plain_text_omits_scripts_and_keeps_block_boundaries(self):
        self.assertEqual(ibm_docs.plain_text(
            b"<style>hidden</style><p>A &amp; B</p><script>alert(1)</script><p>C</p>"
        ), ["A & B", "C"])

    def test_search_and_read_are_offline_and_show_provenance(self):
        self.seed()
        for args in (["search", "Example operation"], ["read", self.pin.topic, "--lines", "1"]):
            with patch.object(ibm_docs, "load_pins", return_value=([self.pin], {})), \
                    patch.object(docs_api, "fetch", side_effect=AssertionError("network forbidden")), \
                    redirect_stdout(io.StringIO()) as output:
                self.assertEqual(ibm_docs.main(["--cache", str(self.cache), *args]), 0)
                self.assertIn("Example operation", output.getvalue())
                if args[0] == "read":
                    self.assertIn(self.pin.sha256, output.getvalue())
                    self.assertIn(self.pin.baseline, output.getvalue())
                    self.assertIn(self.pin.url, output.getvalue())
                    self.assertNotIn("Example rule.", output.getvalue())

    def test_search_does_not_present_corrupt_cache_as_verified(self):
        docs_api.write_retrieved(self.cache / self.pin.key, b"changed")
        with patch.object(ibm_docs, "load_pins", return_value=([self.pin], {})), \
                redirect_stdout(io.StringIO()) as output:
            self.assertEqual(ibm_docs.main(["--cache", str(self.cache), "search", "Example"]), 1)
            self.assertIn('"mismatch": 1', output.getvalue())
            self.assertNotIn("[example]", output.getvalue())

    def test_shipped_pins_validate_and_inconsistent_manifests_fail(self):
        pins, tocs = ibm_docs.load_pins()
        self.assertTrue(pins)
        self.assertTrue(tocs)
        with patch.object(docs_api, "manifest_digest", return_value="bad"):
            with self.assertRaisesRegex(ValueError, "manifest disagrees"):
                ibm_docs.load_pins()


if __name__ == "__main__":
    unittest.main()
