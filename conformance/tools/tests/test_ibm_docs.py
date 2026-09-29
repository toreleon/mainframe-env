"""Offline IBM cache tests using only synthetic publication bodies."""

from contextlib import redirect_stderr, redirect_stdout
from concurrent.futures import ThreadPoolExecutor
import hashlib
import io
import json
import os
from pathlib import Path
import sys
import tarfile
import tempfile
from threading import Barrier
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
        self.toc_body = b'{"toc": []}'
        self.scope = ibm_docs.Scope(
            "example-scope",
            "example",
            "example-v1",
            "0.9.0",
            "conformance/0.9/manifests/example.json",
        )
        self.pin = ibm_docs.Pin(
            "PRODUCT/ref/example.html",
            "https://www.ibm.com/docs/api/v1/content/example",
            docs_api.digest(self.body),
            len(self.body),
            (self.scope,),
        )
        self.toc = ibm_docs.TocPin(
            "https://www.ibm.com/docs/api/v1/toc/example?lang=en",
            docs_api.digest(self.toc_body),
            (self.scope,),
        )

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

    def complete_entries(self):
        return [
            (self.pin.legacy_key, self.body, None),
            (self.toc.legacy_key, self.toc_body, None),
        ]

    def seed(self, pin=None, body=None, *, legacy=False):
        pin = pin or self.pin
        body = body or self.body
        key = pin.legacy_key if legacy else pin.key
        docs_api.write_retrieved(self.cache / key, body)

    def test_environment_default_and_legacy_fallback(self):
        with patch.dict(os.environ, {"MAINFRAME_ENV_IBM_DOCS_CACHE": str(self.cache)}):
            self.assertEqual(docs_api.default_cache(), self.cache.resolve())
        with patch.dict(os.environ, {"MAINFRAME_ENV_IBM_DOCS_CACHE": ""}):
            self.assertEqual(
                docs_api.default_cache(),
                Path(tempfile.gettempdir()).resolve() / "cobolgrammar/topic-cache",
            )

    def test_repository_destination_is_rejected(self):
        with patch.dict(
            os.environ, {"MAINFRAME_ENV_IBM_DOCS_CACHE": str(docs_api.REPOSITORY)}
        ):
            with self.assertRaises(docs_api.InsideRepository):
                docs_api.default_cache()
        with self.assertRaises(docs_api.InsideRepository):
            ibm_docs.import_cache(io.BytesIO(), docs_api.REPOSITORY, [], [])

    def test_topic_and_manifest_paths_reject_traversal_and_unowned_suffixes(self):
        for value in [
            "PRODUCT/../outside.html",
            "PRODUCT/ref\\outside.html",
            "PRODUCT/ref/example.html?other=1",
            "/PRODUCT/ref/example.html",
        ]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                ibm_docs.safe_topic_path(value, "PRODUCT", "test")
        self.assertEqual(
            ibm_docs.safe_topic_path("PRODUCT/ref/example.html?pos=2", "PRODUCT", "test"),
            "PRODUCT/ref/example.html?pos=2",
        )
        with self.assertRaises(ValueError):
            ibm_docs.safe_relative_manifest(
                "conformance/0.9/manifests/../escape.json",
                "conformance/0.9/manifests/",
                "test",
            )

    def test_import_is_verified_complete_idempotent_and_content_addressed(self):
        entries = self.complete_entries()
        first = ibm_docs.import_cache(
            self.archive(entries), self.cache, [self.pin], [self.toc]
        )
        second = ibm_docs.import_cache(
            self.archive(entries), self.cache, [self.pin], [self.toc]
        )
        self.assertEqual(first["imported"], 2)
        self.assertEqual(second["already_present"], 2)
        self.assertEqual(first["missing_expected"], 0)
        self.assertEqual(ibm_docs.cached_body(self.cache, self.pin), self.body)
        self.assertEqual(ibm_docs.cached_toc(self.cache, self.toc), self.toc_body)
        self.assertTrue((self.cache / self.pin.key).is_file())
        self.assertNotEqual(self.pin.key, self.pin.legacy_key)

    def test_empty_partial_and_wrong_imports_cannot_satisfy_scope(self):
        cases = {
            "empty": [],
            "partial": [(self.pin.legacy_key, self.body, None)],
            "wrong": [
                (self.pin.legacy_key, b"unreviewed", None),
                (self.toc.legacy_key, self.toc_body, None),
            ],
        }
        for name, entries in cases.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                counts = ibm_docs.import_cache(
                    self.archive(entries), Path(temporary) / "cache", [self.pin], [self.toc]
                )
                self.assertGreater(counts["missing_expected"], 0)
                if name == "wrong":
                    self.assertEqual(counts["rejected_mismatch"], 1)

    def test_import_preserves_existing_conflicts(self):
        self.seed(body=b"older cache")
        counts = ibm_docs.import_cache(
            self.archive(self.complete_entries()), self.cache, [self.pin], [self.toc]
        )
        self.assertEqual(counts["rejected_conflict"], 1)
        self.assertEqual((self.cache / self.pin.key).read_bytes(), b"older cache")
        self.assertEqual(counts["mismatch_expected"], 1)

    def test_interrupted_write_does_not_publish_partial_bytes_and_can_retry(self):
        original = docs_api.write_retrieved

        def interrupted(path, body):
            path.write_bytes(body[:3])
            raise OSError("simulated interruption")

        with patch.object(docs_api, "write_retrieved", side_effect=interrupted):
            with self.assertRaisesRegex(OSError, "interruption"):
                ibm_docs.import_cache(
                    self.archive(self.complete_entries()), self.cache, [self.pin], [self.toc]
                )
        self.assertEqual(list(self.cache.iterdir()), [])
        with patch.object(docs_api, "write_retrieved", side_effect=original):
            self.assertEqual(
                ibm_docs.import_cache(
                    self.archive(self.complete_entries()), self.cache, [self.pin], [self.toc]
                )["imported"],
                2,
            )

    def test_concurrent_identical_publish_is_idempotent(self):
        target = ibm_docs.pin_target(self.pin)
        barrier = Barrier(2)
        original_link = os.link

        def racing(source, destination, *args, **kwargs):
            barrier.wait(timeout=5)
            return original_link(source, destination, *args, **kwargs)

        def publish():
            counts = ibm_docs.Counter()
            ibm_docs.publish(self.cache, target, self.body, counts)
            return counts

        with patch.object(os, "link", side_effect=racing), ThreadPoolExecutor(
            max_workers=2
        ) as workers:
            results = list(workers.map(lambda _: publish(), range(2)))
        self.assertEqual(sum(counts["imported"] for counts in results), 1)
        self.assertEqual(sum(counts["already_present"] for counts in results), 1)
        self.assertEqual(ibm_docs.cached_body(self.cache, self.pin), self.body)

    def test_existing_oversized_target_is_rejected_without_reading_it(self):
        self.cache.mkdir()
        destination = self.cache / self.pin.key
        destination.write_bytes(b"X" * (len(self.body) + 2))
        counts = ibm_docs.Counter()
        with patch.object(ibm_docs, "MAX_FILE", len(self.body) + 1), patch.object(
            Path, "open", side_effect=AssertionError("must not read oversized target")
        ):
            ibm_docs.publish(
                self.cache, ibm_docs.pin_target(self.pin), self.body, counts
            )
        self.assertEqual(counts["rejected_conflict"], 1)

    def test_archive_paths_links_and_unknown_files_are_not_extracted(self):
        entries = [
            ("../" + self.pin.legacy_key, self.body, None),
            ("/" + self.pin.legacy_key, self.body, None),
            (self.pin.legacy_key, b"", "../outside"),
            ("unrecognized.html", self.body, None),
        ]
        counts = ibm_docs.import_cache(
            self.archive(entries), self.cache, [self.pin], [self.toc]
        )
        self.assertEqual(counts["skipped_unrecognized"], 4)
        self.assertEqual(counts["missing_expected"], 2)

    def test_destination_symlinks_are_refused_for_import_and_read(self):
        self.cache.mkdir()
        outside = Path(self.directory.name) / "outside"
        outside.write_bytes(self.body)
        (self.cache / self.pin.key).symlink_to(outside)
        counts = ibm_docs.import_cache(
            self.archive(self.complete_entries()), self.cache, [self.pin], [self.toc]
        )
        self.assertEqual(counts["rejected_conflict"], 1)
        with self.assertRaisesRegex(ValueError, "symlink"):
            ibm_docs.cached_body(self.cache, self.pin)

    def test_import_bounds_file_total_and_entry_count(self):
        entries = self.complete_entries()
        for limit in ("MAX_FILE", "MAX_IMPORT"):
            with self.subTest(limit=limit), patch.object(
                ibm_docs, limit, len(self.body) - 1
            ):
                with self.assertRaisesRegex(ValueError, "bounded"):
                    ibm_docs.import_cache(
                        self.archive(entries), self.cache, [self.pin], [self.toc]
                    )
        with patch.object(ibm_docs, "MAX_ARCHIVE_ENTRIES", 1):
            with self.assertRaisesRegex(ValueError, "bounded"):
                ibm_docs.import_cache(
                    self.archive(entries), self.cache, [self.pin], [self.toc]
                )

    def test_legacy_read_rejects_truncation_and_same_size_corruption(self):
        for body in (self.body[:-1], b"X" + self.body[1:]):
            with self.subTest(size=len(body)), tempfile.TemporaryDirectory() as temporary:
                cache = Path(temporary) / "cache"
                docs_api.write_retrieved(cache / self.pin.legacy_key, body)
                with self.assertRaisesRegex(ValueError, "mismatch"):
                    ibm_docs.cached_body(cache, self.pin)
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary) / "cache"
            docs_api.write_retrieved(cache / self.pin.legacy_key, self.body)
            self.assertEqual(ibm_docs.cached_body(cache, self.pin), self.body)

    def test_plain_text_omits_scripts_and_keeps_block_boundaries(self):
        self.assertEqual(
            ibm_docs.plain_text(
                b"<style>hidden</style><p>A &amp; B</p>"
                b"<script>alert(1)</script><p>C</p>"
            ),
            ["A & B", "C"],
        )

    def test_status_verifies_toc_and_search_read_are_offline(self):
        self.seed()
        with patch.object(ibm_docs, "load_pins", return_value=([self.pin], [self.toc])), \
                redirect_stdout(io.StringIO()) as output:
            self.assertEqual(ibm_docs.main(["--cache", str(self.cache), "status"]), 1)
            self.assertIn('"missing": 1', output.getvalue())
            self.assertIn('"tocs"', output.getvalue())
        with patch.object(
            ibm_docs, "load_pins", return_value=([self.pin], [self.toc])
        ), redirect_stdout(io.StringIO()) as output, redirect_stderr(io.StringIO()):
            self.assertEqual(
                ibm_docs.main(
                    ["--cache", str(self.cache), "read", self.pin.topic, "--lines", "1"]
                ),
                1,
            )
            self.assertNotIn("Verified scopes", output.getvalue())
        docs_api.write_retrieved(self.cache / self.toc.key, self.toc_body)
        for args in (
            ["search", "Example operation"],
            ["read", self.pin.topic, "--lines", "1"],
        ):
            with patch.object(
                ibm_docs, "load_pins", return_value=([self.pin], [self.toc])
            ), patch.object(
                docs_api, "fetch", side_effect=AssertionError("network forbidden")
            ), redirect_stdout(io.StringIO()) as output:
                self.assertEqual(
                    ibm_docs.main(["--cache", str(self.cache), *args]), 0
                )
                self.assertIn("Example operation", output.getvalue())
                if args[0] == "read":
                    self.assertIn(self.pin.sha256, output.getvalue())
                    self.assertIn(self.scope.scope_id, output.getvalue())
                    self.assertNotIn("Example rule.", output.getvalue())

    def test_search_does_not_present_corrupt_cache_as_verified(self):
        corrupt = b"X" + self.body[1:]
        docs_api.write_retrieved(self.cache / self.pin.key, corrupt)
        docs_api.write_retrieved(self.cache / self.toc.key, self.toc_body)
        with patch.object(
            ibm_docs, "load_pins", return_value=([self.pin], [self.toc])
        ), redirect_stdout(io.StringIO()) as output:
            self.assertEqual(
                ibm_docs.main(
                    ["--cache", str(self.cache), "search", "Example operation"]
                ),
                1,
            )
            self.assertIn('"mismatch": 1', output.getvalue())
            self.assertNotIn("[example] Example operation", output.getvalue())

    def test_same_topic_different_snapshots_are_separately_addressable(self):
        newer_body = self.body + b" newer"
        newer_scope = ibm_docs.Scope(
            "newer-scope", "example", "example-v2", "0.9.0", self.scope.manifest
        )
        newer = ibm_docs.Pin(
            self.pin.topic,
            self.pin.url,
            docs_api.digest(newer_body),
            len(newer_body),
            (newer_scope,),
        )
        combined_toc = ibm_docs.TocPin(
            self.toc.url, self.toc.sha256, (self.scope, newer_scope)
        )
        self.assertNotEqual(self.pin.key, newer.key)
        self.seed()
        self.seed(newer, newer_body)
        docs_api.write_retrieved(self.cache / self.toc.key, self.toc_body)
        with patch.object(
            ibm_docs, "load_pins", return_value=([self.pin, newer], [combined_toc])
        ), redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                ibm_docs.main(["--cache", str(self.cache), "read", self.pin.topic])
        with patch.object(
            ibm_docs, "load_pins", return_value=([self.pin, newer], [combined_toc])
        ), redirect_stdout(io.StringIO()) as output:
            self.assertEqual(
                ibm_docs.main(
                    [
                        "--cache",
                        str(self.cache),
                        "read",
                        self.pin.topic,
                        "--sha256",
                        newer.sha256,
                        "--lines",
                        "1",
                    ]
                ),
                0,
            )
            self.assertIn(newer.sha256, output.getvalue())

    def manifest(self, body, target, baseline, subsystem, topic):
        topic_row = {
            "topic_path": topic,
            "sha256": docs_api.digest(body),
            "bytes": len(body),
            "last_modified": "2026-09-10",
        }
        product = topic.split("/", 1)[0]
        return {
            "schema_version": "mainframe-env.topic-manifest@1",
            "target_version": target,
            "baseline_id": baseline,
            "subsystem": subsystem,
            "product": product,
            "book_label": "Synthetic",
            "book_href": topic,
            "toc_url": "https://www.ibm.com/docs/api/v1/toc/example?lang=en",
            "toc_sha256": docs_api.digest(self.toc_body),
            "content_url_template": docs_api.CONTENT_URL,
            "snapshot_date": "2026-09-10",
            "topic_count": 1,
            "total_bytes": len(body),
            "topic_manifest_digest": docs_api.manifest_digest([topic_row]),
            "topic_manifest_digest_definition": docs_api.DIGEST_DEFINITION,
            "coverage_credit": 0,
            "retained_in_repository": False,
            "topics": [topic_row],
        }

    def source_repository(self, later_body=None, later_topic="PRODUCT/ref/example.html"):
        root = Path(self.directory.name) / "repository"
        old_manifest = self.manifest(
            self.body, "0.2.0", "baseline-old", "example", self.pin.topic
        )
        later_body = self.body if later_body is None else later_body
        new_manifest = self.manifest(
            later_body, "0.9.0", "baseline-new", "example", later_topic
        )
        paths = {
            "conformance/0.2/manifests/example.json": old_manifest,
            "conformance/0.9/manifests/example.json": new_manifest,
        }
        for relative, value in paths.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(value))
        index = {
            "baselines": [
                {
                    "id": "baseline-old",
                    "subsystem": "example",
                    "source": {
                        "kind": "documentation-topics",
                        "product": old_manifest["product"],
                        "book_href": old_manifest["book_href"],
                        "url": old_manifest["toc_url"],
                        "toc_sha256": "sha256:" + old_manifest["toc_sha256"],
                        "content_url_template": docs_api.CONTENT_URL,
                        "manifest": "conformance/0.2/manifests/example.json",
                        "topic_count": 1,
                        "bytes": len(self.body),
                        "sha256": "sha256:" + old_manifest["topic_manifest_digest"],
                    },
                }
            ]
        }
        registry = {
            "schema_version": "mainframe-env.topic-manifest-registry@1",
            "target_version": "0.9.0",
            "semantic_authority": False,
            "coverage_credit": 0,
            "manifests": [
                {
                    "scope_id": "later-scope",
                    "subsystem": "example",
                    "baseline_id": "baseline-new",
                    "manifest": "conformance/0.9/manifests/example.json",
                    "manifest_sha256": "sha256:"
                    + docs_api.digest(
                        (root / "conformance/0.9/manifests/example.json").read_bytes()
                    ),
                    "topic_count": 1,
                    "topic_manifest_sha256": "sha256:"
                    + new_manifest["topic_manifest_digest"],
                    "semantic_authority": False,
                    "coverage_credit": 0,
                }
            ],
        }
        index_path = root / "conformance/0.2/catalogs/index.json"
        registry_path = root / "conformance/0.9/manifests/index.json"
        index_path.parent.mkdir(parents=True, exist_ok=True)
        index_path.write_text(json.dumps(index))
        registry_path.write_text(json.dumps(registry))
        return root, index_path, registry_path

    def test_same_snapshot_coalesces_scopes_and_changed_snapshot_does_not(self):
        root, index, registry = self.source_repository()
        with patch.object(docs_api, "REPOSITORY", root):
            pins, _ = ibm_docs.load_pins(index, registry)
        self.assertEqual(len(pins), 1)
        self.assertEqual(len(pins[0].scopes), 2)

        root, index, registry = self.source_repository(later_body=self.body + b" changed")
        with patch.object(docs_api, "REPOSITORY", root):
            pins, _ = ibm_docs.load_pins(index, registry)
        self.assertEqual(len(pins), 2)
        self.assertEqual(len({pin.key for pin in pins}), 2)

    def test_sanitized_legacy_collision_does_not_collide_addressed_keys(self):
        root, index, registry = self.source_repository(
            later_body=self.body + b" changed", later_topic="PRODUCT/ref_example.html"
        )
        with patch.object(docs_api, "REPOSITORY", root):
            pins, _ = ibm_docs.load_pins(index, registry)
        self.assertEqual(pins[0].legacy_key, pins[1].legacy_key)
        self.assertEqual(len({pin.key for pin in pins}), 2)

    def test_content_address_collision_fails_closed(self):
        root, index, registry = self.source_repository(later_body=self.body + b" changed")
        with patch.object(docs_api, "REPOSITORY", root), patch.object(
            ibm_docs, "cache_key", return_value="collision"
        ):
            with self.assertRaisesRegex(ValueError, "conflicting content-addressed"):
                ibm_docs.load_pins(index, registry)

    def test_registry_binds_exact_manifest_bytes(self):
        root, index, registry = self.source_repository()
        manifest = root / "conformance/0.9/manifests/example.json"
        manifest.write_text(manifest.read_text() + "\n")
        with patch.object(docs_api, "REPOSITORY", root):
            with self.assertRaisesRegex(ValueError, "registry entry disagrees"):
                ibm_docs.load_pins(index, registry)

    def test_global_scope_and_immutable_baseline_collisions_fail_closed(self):
        root, index, registry = self.source_repository(later_body=self.body + b" changed")
        document = json.loads(registry.read_text())
        document["manifests"][0]["scope_id"] = "baseline-old"
        registry.write_text(json.dumps(document))
        with patch.object(docs_api, "REPOSITORY", root):
            with self.assertRaisesRegex(ValueError, "duplicate global source scope"):
                ibm_docs.load_pins(index, registry)

        root, index, registry = self.source_repository(later_body=self.body + b" changed")
        manifest_path = root / "conformance/0.9/manifests/example.json"
        manifest = json.loads(manifest_path.read_text())
        manifest["baseline_id"] = "baseline-old"
        manifest_path.write_text(json.dumps(manifest))
        document = json.loads(registry.read_text())
        document["manifests"][0]["baseline_id"] = "baseline-old"
        document["manifests"][0]["manifest_sha256"] = (
            "sha256:" + docs_api.digest(manifest_path.read_bytes())
        )
        registry.write_text(json.dumps(document))
        with patch.object(docs_api, "REPOSITORY", root):
            with self.assertRaisesRegex(ValueError, "conflicting immutable baseline"):
                ibm_docs.load_pins(index, registry)

    def test_shipped_zero_credit_pins_include_registered_0_9_scopes(self):
        pins, tocs = ibm_docs.load_pins()
        scopes = {scope.scope_id for pin in [*pins, *tocs] for scope in pin.scopes}
        self.assertIn("cics-file-uow-pilot", scopes)
        self.assertIn("cics-handle-aid", scopes)
        self.assertIn("cics-task-enqueue", scopes)
        self.assertIn("cobol-numeric-move-pilot", scopes)
        self.assertTrue(pins)
        self.assertTrue(tocs)
        index_digest = hashlib.sha256(ibm_docs.INDEX.read_bytes()).hexdigest()
        self.assertEqual(
            index_digest,
            "f82fd884a9ad21ce478c97fb593c99a5595a385a92b08ea96b1178c2456392db",
        )


if __name__ == "__main__":
    unittest.main()
