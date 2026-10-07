"""Offline IBM cache tests using only synthetic publication bodies."""

from contextlib import redirect_stderr, redirect_stdout
from concurrent.futures import ThreadPoolExecutor
import hashlib
from copy import deepcopy
import io
import json
import os
from pathlib import Path
import shutil
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
            "cics.application-api",
            "conformance/subsystems/cics/application/manifests/example.json",
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
                "conformance/subsystems/cics/application/manifests/../escape.json",
                "conformance/subsystems/cics/application/manifests/",
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
            "newer-scope", "example", "example-v2", "cics.application-api", self.scope.manifest
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
            "target_subsystem": target,
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
            self.body, "coverage.foundation", "baseline-old", "example", self.pin.topic
        )
        later_body = self.body if later_body is None else later_body
        new_manifest = self.manifest(
            later_body, "cics.application-api", "baseline-new", "example", later_topic
        )
        paths = {
            "conformance/subsystems/coverage/manifests/example.json": old_manifest,
            "conformance/subsystems/cics/application/manifests/example.json": new_manifest,
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
                        "manifest": "conformance/subsystems/coverage/manifests/example.json",
                        "topic_count": 1,
                        "bytes": len(self.body),
                        "sha256": "sha256:" + old_manifest["topic_manifest_digest"],
                    },
                }
            ]
        }
        registry = {
            "schema_version": "mainframe-env.topic-manifest-registry@1",
            "target_subsystem": "cics.application-api",
            "semantic_authority": False,
            "coverage_credit": 0,
            "manifests": [
                {
                    "scope_id": "later-scope",
                    "subsystem": "example",
                    "baseline_id": "baseline-new",
                    "manifest": "conformance/subsystems/cics/application/manifests/example.json",
                    "manifest_sha256": "sha256:"
                    + docs_api.digest(
                        (root / "conformance/subsystems/cics/application/manifests/example.json").read_bytes()
                    ),
                    "topic_count": 1,
                    "topic_manifest_sha256": "sha256:"
                    + new_manifest["topic_manifest_digest"],
                    "semantic_authority": False,
                    "coverage_credit": 0,
                }
            ],
        }
        index_path = root / "conformance/subsystems/coverage/catalogs/index.json"
        registry_path = root / "conformance/subsystems/cics/application/manifests/index.json"
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
        manifest = root / "conformance/subsystems/cics/application/manifests/example.json"
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
        manifest_path = root / "conformance/subsystems/cics/application/manifests/example.json"
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

    def test_shipped_zero_credit_pins_include_all_registered_later_scopes(self):
        pins, tocs = ibm_docs.load_pins()
        scopes = {scope.scope_id for pin in [*pins, *tocs] for scope in pin.scopes}
        self.assertIn("cics-file-uow-pilot", scopes)
        self.assertIn("cics-handle-aid", scopes)
        self.assertIn("cics-task-enqueue", scopes)
        self.assertIn("cobol-numeric-move-pilot", scopes)
        self.assertIn("ims-programming-contracts", scopes)
        self.assertIn("ims-database-contracts", scopes)
        self.assertIn("ims-tm-contracts", scopes)
        self.assertIn("mq-programming-supplements", scopes)
        self.assertIn("mq-point-layout-sources", scopes)
        self.assertIn("mq-property-sources", scopes)
        self.assertIn("mq-recovery-policy-sources", scopes)
        self.assertIn("mq-producer-attribute-sources", scopes)
        self.assertIn("mq-rfh2-sources", scopes)
        self.assertIn("mq-message-handle-sources", scopes)
        supplemental, _ = ibm_docs.select(pins, tocs, "mq-programming-supplements", None)
        self.assertEqual(len(supplemental), 80)
        self.assertTrue(all(pin.baseline == "ibm-mq-9.4-programming-supplements-2026-09-12"
                            for pin in supplemental))
        layouts, _ = ibm_docs.select(pins, tocs, "mq-point-layout-sources", None)
        self.assertEqual(len(layouts), 12)
        self.assertTrue(all(pin.baseline == "ibm-mq-9.4-point-layout-sources-2026-09-12"
                            for pin in layouts))
        self.assertFalse({pin.topic for pin in supplemental} & {pin.topic for pin in layouts})
        properties, property_tocs = ibm_docs.select(pins, tocs, "mq-property-sources", None)
        self.assertEqual({pin.topic for pin in properties}, {
            "SSFKSJ_9.4.0/develop/q022940_.html",
            "SSFKSJ_9.4.0/develop/q022950_.html",
            "SSFKSJ_9.4.0/develop/q022960_.html",
            "SSFKSJ_9.4.0/refdev/q091110_.html",
            "SSFKSJ_9.4.0/refdev/q091050_.html",
            "SSFKSJ_9.4.0/refdev/q091320_.html",
            "SSFKSJ_9.4.0/refdev/q091330_.html",
            "SSFKSJ_9.4.0/refdev/q091730_.html",
            "SSFKSJ_9.4.0/refdev/q092160_.html",
            "SSFKSJ_9.4.0/refdev/q092800_.html",
            "SSFKSJ_9.4.0/refdev/q094690_.html",
            "SSFKSJ_9.4.0/refdev/q094695_.html",
        })
        self.assertTrue(all(pin.baseline == "ibm-mq-9.4-property-sources-2026-09-12"
                            for pin in properties))
        self.assertEqual(len(property_tocs), 1)
        self.assertFalse({pin.topic for pin in properties}
                         & {pin.topic for pin in [*supplemental, *layouts]})
        self.assertEqual(docs_api.digest((docs_api.REPOSITORY /
                         "conformance/subsystems/mq/manifests/mq-point-layout-sources-topics.json").read_bytes()),
                         "40142139c033bea0426d1c489469c1239c8897554c6e0a013a64cff1bdfc1e46")
        self.assertEqual(docs_api.digest((docs_api.REPOSITORY /
                         "conformance/subsystems/mq/manifests/mq-programming-supplements-topics.json").read_bytes()),
                         "d6b4ec6f15426df98f529c144fb5d5320be0e74a2a82598d54a95712c2b9866a")
        self.assertTrue(pins)
        self.assertTrue(tocs)
        index_digest = hashlib.sha256(ibm_docs.INDEX.read_bytes()).hexdigest()
        self.assertEqual(
            index_digest,
            "b51614abbcb8e9522dfced3adc79a9fc834ffe825046cfe3d8e385485461fd48",
        )

    def test_recovery_scope_is_one_independent_frozen_zero_credit_source(self):
        pins, tocs = ibm_docs.load_pins()
        selected, selected_tocs = ibm_docs.select(
            pins, tocs, "mq-recovery-policy-sources", None
        )
        self.assertEqual(len(selected), 1)
        self.assertEqual(len(selected_tocs), 1)
        pin = selected[0]
        self.assertEqual(pin.topic, "SSFKSJ_9.4.0/refdev/q103230_.html")
        self.assertEqual(pin.sha256,
                         "22ee650c2f0fb23bc181d928ff70d401f0b4e288a0039d47110a012b9702a8a1")
        self.assertEqual(pin.size, 3691)
        self.assertEqual(pin.baseline, "ibm-mq-9.4-recovery-policy-sources-2026-09-12")
        self.assertEqual(selected_tocs[0].sha256,
                         "5b23147424db490f5292bd56afe1a0dd2a6ccdde3a08388a79d599998e002bd4")
        registry = json.loads((docs_api.REPOSITORY /
                               "conformance/subsystems/mq/manifests/index.json").read_text())
        current_scopes = {
            "mq-programming-supplements", "mq-point-layout-sources", "mq-property-sources",
            "mq-recovery-policy-sources", "mq-producer-attribute-sources", "mq-rfh2-sources",
            "mq-message-handle-sources", "mq-inquiry-attribute-sources",
        }
        self.assertEqual(len(registry["manifests"]), 8)
        self.assertEqual({row["scope_id"] for row in registry["manifests"]}, current_scopes)
        historical = [row for row in registry["manifests"]
                      if row["scope_id"] != "mq-inquiry-attribute-sources"]
        self.assertEqual(len(historical), 7)
        self.assertEqual({row["scope_id"] for row in historical},
                         current_scopes - {"mq-inquiry-attribute-sources"})
        inquiry, inquiry_tocs = ibm_docs.select(
            pins, tocs, "mq-inquiry-attribute-sources", None
        )
        self.assertEqual(sorted((pin.topic, pin.sha256, pin.size) for pin in inquiry), [
            ("SSFKSJ_9.4.0/refdev/q092480_.html",
             "ac5de9d74f62635456e566bfcbba7706699686cef14580ab30aa22166b2fb1ba", 4661),
            ("SSFKSJ_9.4.0/refdev/q102690_.html",
             "d488b1433fe1fe6b5051459af841e02801df4d62d43bbfc3f9f0954575b411b4", 1247),
            ("SSFKSJ_9.4.0/refdev/q103420_.html",
             "27228d1ae3bf316c77949becc2ed9b26fe9bb4aad97f974a4d31b95caccc3861", 2105),
            ("SSFKSJ_9.4.0/refdev/q103490_.html",
             "93bd55632f95b85f9790e3cba75c623bff93c1a679983115546edf32ce527639", 2267),
        ])
        self.assertTrue(all(pin.baseline == "ibm-mq-9.4-inquiry-attribute-sources-2026-09-12"
                            for pin in inquiry))
        self.assertEqual(len(inquiry_tocs), 1)
        self.assertEqual(inquiry_tocs[0].sha256,
                         "5b23147424db490f5292bd56afe1a0dd2a6ccdde3a08388a79d599998e002bd4")
        inquiry_row = next(row for row in registry["manifests"]
                           if row["scope_id"] == "mq-inquiry-attribute-sources")
        self.assertFalse(inquiry_row["semantic_authority"])
        self.assertEqual(inquiry_row["coverage_credit"], 0)
        self.assertEqual(inquiry_row["topic_count"], 4)
        self.assertEqual(inquiry_row["manifest_sha256"],
                         "sha256:2a015ae7e74819b603d2b07a0d7af5eef172623550be86f7fc8e74189b3eb294")
        self.assertEqual(inquiry_row["topic_manifest_sha256"],
                         "sha256:463e5ba10a4b572cd5a73ff08066820b66c59e3133b37910ecd4c1c9af5ce527")
        old_rows = [row for row in registry["manifests"]
                    if row["scope_id"] not in {
                        "mq-recovery-policy-sources", "mq-producer-attribute-sources",
                        "mq-rfh2-sources", "mq-message-handle-sources",
                        "mq-inquiry-attribute-sources"}]
        self.assertEqual(docs_api.digest(json.dumps(
            old_rows, sort_keys=True, separators=(",", ":")).encode()),
            "087da2037d9a5eadb046dfda6f771293405c10a1a3ec1a99c0c9e498114bdd0e")
        recovery = next(row for row in registry["manifests"]
                        if row["scope_id"] == "mq-recovery-policy-sources")
        self.assertFalse(recovery["semantic_authority"])
        self.assertEqual(recovery["coverage_credit"], 0)
        self.assertEqual(recovery["topic_count"], 1)
        self.assertEqual(recovery["manifest_sha256"],
                         "sha256:4c22648e9ad90c7e60832f6f37488f0c5f7f87401d00707dd4a6c154e2c47742")
        self.assertEqual(recovery["topic_manifest_sha256"],
                         "sha256:414608491fdbefa8ec0b8b2b1adf8742a4dbc5553cd488344db649ef945bba11")
        frozen = {
            "conformance/subsystems/coverage/catalogs/index.json":
                "b51614abbcb8e9522dfced3adc79a9fc834ffe825046cfe3d8e385485461fd48",
            "conformance/subsystems/coverage/catalogs/mq.json":
                "62e70382c8d59e28234f249acde75faf3495acb78f8ac19829ff2d7e8290ebe7",
            "conformance/subsystems/coverage/manifests/mq-topics.json":
                "90841b086eb887cd2b13512ce41312aab2367f72d7351d6c93e9f52fbd7fab56",
            "conformance/subsystems/mq/manifests/mq-programming-supplements-topics.json":
                "d6b4ec6f15426df98f529c144fb5d5320be0e74a2a82598d54a95712c2b9866a",
            "conformance/subsystems/mq/manifests/mq-point-layout-sources-topics.json":
                "40142139c033bea0426d1c489469c1239c8897554c6e0a013a64cff1bdfc1e46",
            "conformance/subsystems/mq/manifests/mq-property-sources-topics.json":
                "5b452011d141285b7ad8d7f064ef87d74d73e9dd19e5368da2ed23bd951a6509",
        }
        for relative, digest in frozen.items():
            with self.subTest(path=relative):
                self.assertEqual(docs_api.digest(
                    (docs_api.REPOSITORY / relative).read_bytes()), digest)

    def test_producer_attribute_scope_binds_nine_independent_zero_credit_pins(self):
        pins, tocs = ibm_docs.load_pins()
        selected, selected_tocs = ibm_docs.select(
            pins, tocs, "mq-producer-attribute-sources", None
        )
        expected = {
            "q090310_": ("99c5eb46d8046b6ab2ce7aad0c8284e79bb4f0ae24c0d1942664bec451fa3c2d", 12159),
            "q102230_": ("3a983b400b7497dde61623f58c5656d6d5da7920147b638ea25dc8ee3bae6291", 1650),
            "q102510_": ("c55745f2e5ab347547fb95b1deeff9e9ce980435643fc300dc831c2202a76baa", 1617),
            "q102520_": ("4a4821f9faac0b5605ef05c260fcb9f5858f3770014b2157ae7653514cfae57a", 668),
            "q103140_": ("b62d0e01bb209571b35c9cff1bd0292cac6e5a6b517a4e90123a855c785e71a0", 2165),
            "q103180_": ("20aa3eba7a13ebcc714d405228fb192cb86411429bb8729142943782138074a0", 3963),
            "q103190_": ("523f962203da33fdce4cf7fa638b1c5ee03bfeab664f491d5ae714510f031bc1", 4278),
            "q103280_": ("3749a7eea034280f0762a2d80ff564cacbdc67dc4fbd7efe24231845afef289b", 3682),
            "q103300_": ("0df88519d9ea7e46448590980fe1619e494048676ac095a66097a5c9ec7762b7", 6113),
        }
        self.assertEqual({Path(pin.topic).stem: (pin.sha256, pin.size)
                          for pin in selected}, expected)
        self.assertTrue(all(pin.baseline == "ibm-mq-9.4-producer-attribute-sources-2026-09-12"
                            for pin in selected))
        self.assertEqual(len(selected_tocs), 1)
        self.assertEqual(selected_tocs[0].sha256,
                         "5b23147424db490f5292bd56afe1a0dd2a6ccdde3a08388a79d599998e002bd4")
        registry = json.loads((docs_api.REPOSITORY /
                               "conformance/subsystems/mq/manifests/index.json").read_text())
        row = next(row for row in registry["manifests"]
                   if row["scope_id"] == "mq-producer-attribute-sources")
        self.assertFalse(row["semantic_authority"])
        self.assertEqual(row["coverage_credit"], 0)
        old = [row for row in registry["manifests"]
               if row["scope_id"] not in {"mq-producer-attribute-sources", "mq-rfh2-sources",
                                           "mq-message-handle-sources",
                                           "mq-inquiry-attribute-sources"}]
        self.assertEqual([row["topic_count"] for row in old], [80, 12, 12, 1])
        for prior in old:
            self.assertEqual(prior["manifest_sha256"], "sha256:" + docs_api.digest(
                (docs_api.REPOSITORY / prior["manifest"]).read_bytes()))
        self.assertFalse({pin.topic for pin in selected} & {
            pin.topic for pin in pins if pin not in selected})

    def test_producer_nine_topic_fixture_uses_shared_import_search_and_read(self):
        # Authored synthetic bodies, never copied publication text or source credit.
        root, index, registry, first_path, document, _ = self.registry_015()
        first_bytes = first_path.read_bytes()
        bodies = [f'<h1>Producer fixture {n}</h1><p>Authored example {n}.</p>'.encode()
                  for n in range(9)]
        manifest = self.manifest(bodies[0], "mq.programming", "producer-fixture", "mq",
                                 "PRODUCT/ref/producer-0.html")
        manifest["topics"] = [{
            "topic_path": f"PRODUCT/ref/producer-{n}.html",
            "sha256": docs_api.digest(body), "bytes": len(body),
            "last_modified": "2026-09-10",
        } for n, body in enumerate(bodies)]
        manifest.update(topic_count=9, total_bytes=sum(map(len, bodies)),
                        topic_manifest_digest=docs_api.manifest_digest(manifest["topics"]))
        relative = "conformance/subsystems/mq/manifests/producer-topics.json"
        path = root / relative
        path.write_text(json.dumps(manifest))
        entry = deepcopy(document["manifests"][0])
        entry.update(scope_id="producer-fixture", baseline_id="producer-fixture",
                     manifest=relative, topic_count=9,
                     manifest_sha256="sha256:" + docs_api.digest(path.read_bytes()),
                     topic_manifest_sha256="sha256:" + manifest["topic_manifest_digest"])
        document["manifests"].append(entry)
        registry.write_text(json.dumps(document))
        with patch.object(docs_api, "REPOSITORY", root):
            pins, tocs = ibm_docs.load_pins(index, registry)
            selected, selected_tocs = ibm_docs.select(pins, tocs, "producer-fixture", None)
            entries = [(pin.legacy_key, body, None) for pin, body in zip(selected, bodies)]
            entries.append((selected_tocs[0].legacy_key, self.toc_body, None))
            counts = ibm_docs.import_cache(self.archive(entries), self.cache,
                                          selected, selected_tocs)
            self.assertEqual(counts["imported"], 10)
            self.assertEqual(counts["missing_expected"], 0)
            with patch.object(ibm_docs, "load_pins", return_value=(pins, tocs)):
                for n, pin in enumerate(selected):
                    output = io.StringIO()
                    with redirect_stdout(output):
                        self.assertEqual(ibm_docs.main([
                            "--cache", str(self.cache), "read", pin.topic,
                            "--scope", "producer-fixture", "--sha256", pin.sha256,
                            "--lines", "2"]), 0)
                    self.assertIn(f"Producer fixture {n}", output.getvalue())
                with redirect_stdout(io.StringIO()) as output:
                    self.assertEqual(ibm_docs.main([
                        "--cache", str(self.cache), "search", "Producer fixture 8",
                        "--scope", "producer-fixture"]), 0)
                    self.assertIn(selected[8].sha256, output.getvalue())
            (self.cache / selected[0].key).write_bytes(b"x" * selected[0].size)
            with self.assertRaises(ValueError):
                ibm_docs.cached_body(self.cache, selected[0])
            with self.assertRaises(FileNotFoundError):
                ibm_docs.cached_body(self.cache, next(pin for pin in pins if pin not in selected))
        self.assertEqual(first_path.read_bytes(), first_bytes)

    def test_rfh2_scope_matches_fixed_reference_pins_without_other_body_caches(self):
        fixture = json.loads((Path(__file__).parent /
                              "fixtures/mq-rfh2-source-pins.json").read_text())
        pins, tocs = ibm_docs.load_pins()
        selected, selected_tocs = ibm_docs.select(pins, tocs, "mq-rfh2-sources", None)
        self.assertEqual(len(selected), 15)
        self.assertEqual(len(selected_tocs), 1)
        self.assertEqual(selected_tocs[0].sha256, fixture["toc_sha256"])
        self.assertEqual(sorted((p.topic, p.sha256, p.size) for p in selected),
                         sorted((p["topic_path"], p["sha256"], p["bytes"])
                                for p in fixture["topics"]))
        self.assertTrue(all(p.baseline == fixture["baseline_id"] for p in selected))
        root = docs_api.REPOSITORY
        registry = json.loads((root / "conformance/subsystems/mq/manifests/index.json").read_text())
        old = [r for r in registry["manifests"]
               if r["scope_id"] not in {"mq-rfh2-sources", "mq-producer-attribute-sources",
                                       "mq-message-handle-sources",
                                       "mq-inquiry-attribute-sources"}]
        self.assertEqual(docs_api.digest(json.dumps(
            old, sort_keys=True, separators=(",", ":")).encode()),
            fixture["old_registry_rows_sha256"])
        row = next(r for r in registry["manifests"] if r["scope_id"] == "mq-rfh2-sources")
        self.assertFalse(row["semantic_authority"])
        self.assertEqual(row["coverage_credit"], 0)
        self.assertEqual(row["manifest_sha256"], fixture["manifest_sha256"])
        self.assertEqual(row["topic_manifest_sha256"], "sha256:" + fixture["topic_manifest_digest"])
        manifest = json.loads((root / row["manifest"]).read_text())
        self.assertEqual(manifest["topics"], fixture["topics"])
        self.assertEqual(manifest["product"], fixture["product"])
        self.assertFalse(self.cache.exists())  # No unrelated external HTML is required.

    def test_rfh2_registered_artifact_mutants_fail_before_cache_access(self):
        self.registered_scope_mutants("mq-rfh2-sources", 14)

    def test_message_handle_registered_artifact_mutants_fail_before_cache_access(self):
        self.registered_scope_mutants("mq-message-handle-sources", 2)

    def registered_scope_mutants(self, scope_id, wrong_count):
        root = Path(self.directory.name) / (scope_id + "-registry")
        directory = root / "conformance/subsystems/mq/manifests"
        directory.mkdir(parents=True)
        source = docs_api.REPOSITORY / "conformance/subsystems/mq/manifests"
        for path in source.glob("*.json"):
            shutil.copyfile(path, directory / path.name)
        registry_path = directory / "index.json"
        manifest_path = directory / (scope_id + "-topics.json")
        original_registry = json.loads(registry_path.read_text())
        original_manifest = json.loads(manifest_path.read_text())
        scope = next(i for i, row in enumerate(original_registry["manifests"])
                     if row["scope_id"] == scope_id)
        with patch.object(docs_api, "REPOSITORY", root):
            for field, value in [("manifest_sha256", "sha256:" + "0" * 64),
                                 ("topic_manifest_sha256", "sha256:" + "0" * 64),
                                 ("topic_count", wrong_count), ("subsystem", "ims"),
                                 ("semantic_authority", True), ("coverage_credit", 1)]:
                with self.subTest(registry_field=field):
                    registry = deepcopy(original_registry)
                    registry["manifests"][scope][field] = value
                    registry_path.write_text(json.dumps(registry))
                    with self.assertRaises(ValueError):
                        ibm_docs.registered_sources(registry_path)
            for mutation in ["hash", "bytes", "missing", "duplicate", "foreign", "version"]:
                with self.subTest(manifest_mutation=mutation):
                    manifest = deepcopy(original_manifest)
                    if mutation == "hash":
                        manifest["topics"][0]["sha256"] = "0" * 64
                    elif mutation == "bytes":
                        manifest["topics"][0]["bytes"] += 1
                    elif mutation == "missing":
                        manifest["topics"].pop()
                    elif mutation == "duplicate":
                        manifest["topics"].append(deepcopy(manifest["topics"][0]))
                    elif mutation == "foreign":
                        manifest["topics"][0]["topic_path"] = "FOREIGN/ref/topic.html"
                    else:
                        manifest["target_subsystem"] = "ims.programming"
                    manifest_path.write_text(json.dumps(manifest))
                    registry = deepcopy(original_registry)
                    registry["manifests"][scope]["manifest_sha256"] = (
                        "sha256:" + docs_api.digest(manifest_path.read_bytes()))
                    registry_path.write_text(json.dumps(registry))
                    with self.assertRaises(ValueError):
                        ibm_docs.registered_sources(registry_path)
            manifest_path.write_text(json.dumps(original_manifest))
            registry = deepcopy(original_registry)
            registry["manifests"][scope]["manifest_sha256"] = (
                "sha256:" + docs_api.digest(manifest_path.read_bytes()))
            registry_path.write_text(json.dumps(registry))
            ibm_docs.registered_sources(registry_path)
            duplicate = deepcopy(registry)
            duplicate["manifests"].append(deepcopy(duplicate["manifests"][scope]))
            registry_path.write_text(json.dumps(duplicate))
            with self.assertRaises(ValueError):
                ibm_docs.registered_sources(registry_path)
            registry_path.write_text(json.dumps(registry))
            unregistered = deepcopy(registry)
            unregistered["manifests"].pop(scope)
            registry_path.write_text(json.dumps(unregistered))
            with self.assertRaises(ValueError):
                ibm_docs.registered_sources(registry_path)
            registry_path.write_text(json.dumps(registry))
            manifest_path.unlink()
            with self.assertRaises(OSError):
                ibm_docs.registered_sources(registry_path)
        self.assertFalse(self.cache.exists())

    def test_message_handle_scope_matches_fixed_pins_and_preserves_prior_registry(self):
        fixture = json.loads((Path(__file__).parent /
                              "fixtures/mq-message-handle-source-pins.json").read_text())
        pins, tocs = ibm_docs.load_pins()
        selected, selected_tocs = ibm_docs.select(pins, tocs, fixture["scope_id"], None)
        self.assertEqual(len(selected), 1)
        self.assertEqual(len(selected_tocs), 1)
        self.assertEqual(selected_tocs[0].sha256, fixture["toc_sha256"])
        self.assertEqual((selected[0].topic, selected[0].sha256, selected[0].size),
                         ("SSFKSJ_9.4.0/refdev/q091560_.html",
                          "72f96585920e26aeab0f567a42a3959cdb58e4016d907600ef820d077db80baa",
                          2409))
        self.assertEqual(selected[0].baseline, fixture["baseline_id"])
        root = docs_api.REPOSITORY
        registry = json.loads((root / "conformance/subsystems/mq/manifests/index.json").read_text())
        old = [r for r in registry["manifests"]
               if r["scope_id"] not in {fixture["scope_id"], "mq-inquiry-attribute-sources"}]
        self.assertEqual(docs_api.digest(json.dumps(
            old, sort_keys=True, separators=(",", ":")).encode()),
            fixture["old_registry_rows_sha256"])
        row = next(r for r in registry["manifests"] if r["scope_id"] == fixture["scope_id"])
        self.assertFalse(row["semantic_authority"])
        self.assertEqual(row["coverage_credit"], 0)
        self.assertEqual(row["manifest_sha256"], fixture["manifest_sha256"])
        self.assertEqual(row["topic_manifest_sha256"], "sha256:" + fixture["topic_manifest_digest"])
        manifest = json.loads((root / row["manifest"]).read_text())
        self.assertEqual(manifest["topics"], fixture["topics"])
        self.assertEqual(manifest["product"], fixture["product"])
        self.assertFalse({p.topic for p in selected} &
                         {p.topic for p in pins if p not in selected})
        self.assertFalse(self.cache.exists())

    def test_message_handle_single_topic_reader_fixture_is_hash_and_scope_bound(self):
        # This authored text exercises the shared reader; it is not IBM content.
        root, index, registry, _, document, _ = self.registry_015()
        scope = document["manifests"][0]["scope_id"]
        with patch.object(docs_api, "REPOSITORY", root):
            pins, tocs = ibm_docs.load_pins(index, registry)
            selected, selected_tocs = ibm_docs.select(pins, tocs, scope, None)
            self.assertEqual(len(selected), 1)
            pin = selected[0]
            counts = ibm_docs.import_cache(self.archive([
                (pin.legacy_key, self.body, None),
                (selected_tocs[0].legacy_key, self.toc_body, None),
            ]), self.cache, selected, selected_tocs)
            self.assertEqual(counts["imported"], 2)
            with patch.object(ibm_docs, "load_pins", return_value=(pins, tocs)):
                with redirect_stdout(io.StringIO()) as output:
                    self.assertEqual(ibm_docs.main([
                        "--cache", str(self.cache), "search", "Example operation",
                        "--scope", scope]), 0)
                    self.assertIn(pin.sha256, output.getvalue())
                with redirect_stdout(io.StringIO()) as output:
                    self.assertEqual(ibm_docs.main([
                        "--cache", str(self.cache), "read", pin.topic,
                        "--scope", scope, "--sha256", pin.sha256, "--lines", "2"]), 0)
                    self.assertIn("Example operation", output.getvalue())
                with self.assertRaises(ValueError):
                    ibm_docs.select(pins, tocs, "unregistered-message-handle", None)
            (self.cache / pin.key).write_bytes(b"x" * pin.size)
            with self.assertRaises(ValueError):
                ibm_docs.cached_body(self.cache, pin)

    def registry_015(self):
        root, index, old_registry = self.source_repository()
        old = json.loads(old_registry.read_text())
        old_manifest = root / old["manifests"][0]["manifest"]
        manifest = json.loads(old_manifest.read_text())
        manifest["target_subsystem"] = "mq.programming"
        manifest["subsystem"] = "mq"
        relative = "conformance/subsystems/mq/manifests/synthetic-topics.json"
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(manifest))
        old["target_subsystem"] = "mq.programming"
        entry = old["manifests"][0]
        entry["manifest"] = relative
        entry["subsystem"] = "mq"
        entry["manifest_sha256"] = "sha256:" + docs_api.digest(path.read_bytes())
        registry = path.parent / "index.json"
        registry.write_text(json.dumps(old))
        return root, index, registry, path, old, manifest

    def test_015_synthetic_scope_uses_shared_reader_without_other_body_caches(self):
        root, index, registry, _, _, _ = self.registry_015()
        with patch.object(docs_api, "REPOSITORY", root):
            pins, tocs = ibm_docs.load_pins(index, registry)
            selected, _ = ibm_docs.select(pins, tocs, "later-scope", None)
        self.assertEqual(len(selected), 1)
        self.assertTrue(any(scope.scope_id == "later-scope" and scope.target_subsystem == "mq.programming"
                            for scope in selected[0].scopes))
        self.assertFalse(self.cache.exists())

    def test_015_two_scopes_preserve_first_pin_and_import_only_selected_bodies(self):
        root, index, registry, first_path, document, _ = self.registry_015()
        first_bytes = first_path.read_bytes()
        first_entry = deepcopy(document["manifests"][0])
        second = self.manifest(b'<h1>Layout</h1>', "mq.programming", "layout-baseline",
                               "mq", "PRODUCT/ref/layout.html")
        relative = "conformance/subsystems/mq/manifests/layout-topics.json"
        path = root / relative
        path.write_text(json.dumps(second))
        entry = deepcopy(first_entry)
        entry.update(scope_id="layout-scope", baseline_id="layout-baseline", manifest=relative,
                     manifest_sha256="sha256:" + docs_api.digest(path.read_bytes()),
                     topic_manifest_sha256="sha256:" + second["topic_manifest_digest"])
        document["manifests"].append(entry)
        registry.write_text(json.dumps(document))
        with patch.object(docs_api, "REPOSITORY", root):
            pins, tocs = ibm_docs.load_pins(index, registry)
            selected, selected_tocs = ibm_docs.select(pins, tocs, "layout-scope", None)
            self.assertEqual(len(selected), 1)
            counts = ibm_docs.import_cache(self.archive([
                (selected[0].legacy_key, b'<h1>Layout</h1>', None),
                (selected_tocs[0].legacy_key, self.toc_body, None),
            ]), self.cache, selected, selected_tocs)
            self.assertEqual(counts["imported"], 2)
            self.assertEqual(counts["missing_expected"], 0)
            self.assertEqual(ibm_docs.cached_body(self.cache, selected[0]), b'<h1>Layout</h1>')
            first, _ = ibm_docs.select(pins, tocs, "later-scope", None)
            with self.assertRaises(FileNotFoundError):
                ibm_docs.cached_body(self.cache, first[0])
            document["manifests"][1]["manifest_sha256"] = "sha256:" + "0" * 64
            registry.write_text(json.dumps(document))
            with self.assertRaisesRegex(ValueError, "registry entry disagrees"):
                ibm_docs.load_pins(index, registry)
        self.assertEqual(first_path.read_bytes(), first_bytes)
        self.assertEqual(document["manifests"][0], first_entry)

    def test_015_registry_identity_pin_and_credit_mutants_fail_closed(self):
        root, _, registry, _, original, _ = self.registry_015()
        mutations = [
            ("manifest_sha256", "sha256:" + "0" * 64),
            ("topic_manifest_sha256", "sha256:" + "0" * 64),
            ("topic_count", 2), ("subsystem", "ims"),
            ("baseline_id", "wrong-baseline"), ("coverage_credit", 1),
            ("semantic_authority", True),
            ("manifest", "conformance/subsystems/ims/manifests/synthetic-topics.json"),
            ("manifest", "conformance/subsystems/mq/manifests/missing.json"),
        ]
        with patch.object(docs_api, "REPOSITORY", root):
            for field, value in mutations:
                document = deepcopy(original)
                document["manifests"][0][field] = value
                registry.write_text(json.dumps(document))
                with self.subTest(field=field, value=value), self.assertRaises((ValueError, OSError)):
                    ibm_docs.registered_sources(registry)
            for field, value in [("target_subsystem", "ims.programming"),
                                 ("coverage_credit", 1), ("semantic_authority", True)]:
                document = deepcopy(original)
                document[field] = value
                registry.write_text(json.dumps(document))
                with self.subTest(field=field), self.assertRaises(ValueError):
                    ibm_docs.registered_sources(registry)
            for repeated_path in (False, True):
                document = deepcopy(original)
                other = deepcopy(document["manifests"][0])
                if repeated_path:
                    other["scope_id"] = "another-scope"
                else:
                    other["manifest"] = "conformance/subsystems/mq/manifests/another.json"
                document["manifests"].append(other)
                registry.write_text(json.dumps(document))
                with self.subTest(repeated_path=repeated_path), self.assertRaises(ValueError):
                    ibm_docs.registered_sources(registry)

    def test_015_manifest_mutants_fail_even_with_updated_file_hash(self):
        root, _, registry, path, registry_original, manifest_original = self.registry_015()
        mutations = [("target_subsystem", "ims.programming"), ("subsystem", "ims"),
                     ("topic_count", 2), ("total_bytes", 1),
                     ("topic_manifest_digest", "0" * 64),
                     ("coverage_credit", 1), ("retained_in_repository", True)]
        with patch.object(docs_api, "REPOSITORY", root):
            for field, value in mutations:
                manifest = deepcopy(manifest_original)
                manifest[field] = value
                path.write_text(json.dumps(manifest))
                document = deepcopy(registry_original)
                document["manifests"][0]["manifest_sha256"] = "sha256:" + docs_api.digest(path.read_bytes())
                registry.write_text(json.dumps(document))
                with self.subTest(field=field), self.assertRaises(ValueError):
                    ibm_docs.registered_sources(registry)
            for variant in ("duplicate", "duplicate-different-hash", "missing-date", "foreign"):
                manifest = deepcopy(manifest_original)
                if variant.startswith("duplicate"):
                    row = deepcopy(manifest["topics"][0])
                    if variant == "duplicate-different-hash":
                        row["sha256"] = "0" * 64
                    manifest["topics"].append(row)
                elif variant == "missing-date":
                    del manifest["topics"][0]["last_modified"]
                else:
                    manifest["topics"][0]["topic_path"] = "FOREIGN/ref/example.html"
                path.write_text(json.dumps(manifest))
                document = deepcopy(registry_original)
                document["manifests"][0]["manifest_sha256"] = "sha256:" + docs_api.digest(path.read_bytes())
                registry.write_text(json.dumps(document))
                with self.subTest(variant=variant), self.assertRaises(ValueError):
                    ibm_docs.registered_sources(registry)

    def test_015_missing_and_unregistered_manifests_are_rejected(self):
        root, _, registry, path, _, _ = self.registry_015()
        extra = path.with_name("unregistered.json")
        with patch.object(docs_api, "REPOSITORY", root):
            extra.write_text("{}")
            with self.assertRaisesRegex(ValueError, "unregistered or missing"):
                ibm_docs.registered_sources(registry)
            extra.unlink()
            path.unlink()
            with self.assertRaises(FileNotFoundError):
                ibm_docs.registered_sources(registry)


if __name__ == "__main__":
    unittest.main()
