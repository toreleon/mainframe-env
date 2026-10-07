"""Deterministic tests for the zero-credit CIC-901 sources-a corpus."""

import copy
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch


TOOL = Path(__file__).resolve().parents[1] / "fetch_cics_application_sources.py"
ROOT = TOOL.parents[5]
SPEC = importlib.util.spec_from_file_location("fetch_cics_application_sources", TOOL)
sources = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sources)


class CicsApplicationSourcesTests(unittest.TestCase):
    def corpus(self, root: Path = ROOT) -> dict:
        return json.loads((root / sources.CORPUS_PATH).read_text())

    def manifest(self, root: Path = ROOT) -> dict:
        return json.loads((root / sources.MANIFEST_PATH).read_text())

    def mapping(self, root: Path = ROOT) -> dict:
        return json.loads((root / sources.MAP_PATH).read_text())

    def fixture(self, root: Path) -> None:
        for relative in [
            sources.MAP_PATH,
            sources.source_map.TOC_PROJECTION_PATH,
            sources.source_map.descriptors.CATALOG_PATH,
            Path("conformance/subsystems/coverage/catalogs/cics.json"),
            sources.CORPUS_PATH,
            sources.BROWSER_RECEIPT_PATH,
            sources.MANIFEST_PATH,
            sources.REGISTRY_PATH,
        ]:
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)

    def write(self, root: Path, relative: Path, value: object) -> None:
        (root / relative).write_text(json.dumps(value, indent=2) + "\n")

    def test_repository_corpus_is_fresh_without_network_or_publication_bytes(self):
        with patch.object(
            sources.docs_api, "fetch", side_effect=AssertionError("network forbidden")
        ), patch.object(
            sources.docs_api, "topic", side_effect=AssertionError("topic bytes forbidden")
        ), patch.object(
            sources, "fetch_binary", side_effect=AssertionError("browser forbidden")
        ):
            sources.check(ROOT)
        corpus = self.corpus()
        self.assertEqual(corpus["status"], "candidate")
        self.assertFalse(corpus["semantic_authority"])
        self.assertFalse(corpus["automatic_registration"])
        self.assertEqual(corpus["coverage_credit"], 0)
        self.assertEqual(corpus["differential_credit"], 0)

    def test_counts_and_topic_union_are_exact(self):
        corpus = self.corpus()
        manifest = self.manifest()
        self.assertEqual(
            corpus["counts"],
            {
                "mapping_rows": 88,
                "mapped_command_topics": 109,
                "linked_context_topics": 58,
                "manual_topics": 6,
                "html_topics": 173,
                "source_gaps_pending_review": 3,
            },
        )
        mapped = set(corpus["mapped_command_topics"])
        linked = {row["topic_path"] for row in corpus["linked_context_topics"]}
        manual = {row["topic_path"] for row in corpus["manual_topics"]}
        manifest_paths = {row["topic_path"] for row in manifest["topics"]}
        self.assertFalse(mapped & linked)
        self.assertFalse(mapped & manual)
        self.assertFalse(linked & manual)
        self.assertEqual(mapped | linked | manual, manifest_paths)
        self.assertEqual(len(manifest_paths), 173)

    def test_mapped_topics_and_gaps_remain_bound_to_the_accepted_map(self):
        mapping = self.mapping()
        expected = {
            topic["topic_path"]
            for row in mapping["rows"]
            for topic in row["topics"]
        }
        self.assertEqual(set(self.corpus()["mapped_command_topics"]), expected)
        self.assertEqual(
            [(row["official_row"], row["label"], row["state"]) for row in self.corpus()["source_gaps"]],
            [
                (sources.ROW_CICSMESSAGE, "CICSMESSAGE", "pending-review"),
                (sources.ROW_DUMP, "DUMP", "pending-review"),
                (sources.ROW_TRACEID, "ENTER TRACEID", "pending-review"),
            ],
        )

    def test_manifest_and_corpus_digests_are_independently_recomputed(self):
        manifest = self.manifest()
        manifest_bytes = sources.pretty(manifest).encode()
        corpus = self.corpus()
        core = {
            key: corpus[key]
            for key in [
                "mapping",
                "topic_manifest",
                "counts",
                "mapped_command_topics",
                "linked_context_topics",
                "manual_topics",
                "excluded_navigation",
                "source_gaps",
            ]
        }
        encoded = json.dumps(
            core, ensure_ascii=True, separators=(",", ":"), sort_keys=True
        ).encode()
        self.assertEqual(
            corpus["corpus_sha256"],
            "sha256:"
            + hashlib.sha256(b"mainframe-env.cics-source-corpus@1\0" + encoded).hexdigest(),
        )
        self.assertEqual(
            corpus["corpus_sha256"],
            "sha256:d7f0a016175b855b8e9231d6a81b97faae16e12ccd060a06f80380df7f02eb31",
        )
        self.assertEqual(
            corpus["topic_manifest"]["file_sha256"],
            "sha256:" + hashlib.sha256(manifest_bytes).hexdigest(),
        )
        self.assertEqual(
            manifest["topic_manifest_digest"],
            sources.docs_api.manifest_digest(manifest["topics"]),
        )

    def test_browser_receipt_is_exactly_manifest_bound(self):
        manifest = self.manifest()
        receipt = json.loads((ROOT / sources.BROWSER_RECEIPT_PATH).read_text())
        self.assertEqual(receipt, sources.expected_browser_receipt(manifest))
        self.assertEqual(receipt["observation"]["topics_requested"], 173)
        self.assertEqual(receipt["observation"]["topic_identity_matches"], 173)
        self.assertEqual(receipt["observation"]["mismatches"], [])
        self.assertEqual(
            receipt["observation"]["identity_sha256"],
            "sha256:6d33171e07e5e247c28e52cb07d6cf7c0a4d132ad110fc1e04a83dfddf119a6d",
        )

    def test_browser_receipt_tampering_fails_closed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            receipt = json.loads((root / sources.BROWSER_RECEIPT_PATH).read_text())
            receipt["observation"]["topic_identity_matches"] -= 1
            self.write(root, sources.BROWSER_RECEIPT_PATH, receipt)
            with self.assertRaisesRegex(sources.CorpusError, "browser"):
                sources.check(root)

    def test_corpus_is_html_only_and_gaps_remain_candidates(self):
        corpus = self.corpus()
        dfhcmp = "SSJL4D_6.x/reference-diagnostics/modules/dfhs3c001248.html"
        self.assertNotIn("external_publications", corpus)
        self.assertNotIn("external_publications", corpus["counts"])
        for gap in corpus["source_gaps"]:
            self.assertEqual(
                set(gap), {"official_row", "label", "state", "topic_sources"}
            )
            self.assertEqual(gap["state"], "pending-review")
            self.assertTrue(all(path.endswith(".html") for path in gap["topic_sources"]))
        trace_gap = next(row for row in corpus["source_gaps"] if row["label"] == "ENTER TRACEID")
        self.assertIn(dfhcmp, trace_gap["topic_sources"])
        self.assertIn(
            {
                "topic_path": dfhcmp,
                "role": "compatibility-context-candidate",
                "applies_to_rows": [sources.ROW_TRACEID],
                "reason": "legacy-monitoring-module-context",
            },
            corpus["manual_topics"],
        )

    def test_link_normalization_accepts_only_canonical_current_product_topics(self):
        source = "SSJL4D_6.x/reference-applications/commands-api/example.html"
        self.assertEqual(
            sources.linked_topic(
                "/docs/en/SSJL4D_6.x/reference-applications/commands-api/next.html#x",
                source,
            ),
            "SSJL4D_6.x/reference-applications/commands-api/next.html",
        )
        self.assertEqual(
            sources.linked_topic("../commands-bts/other.html", source),
            "SSJL4D_6.x/reference-applications/commands-bts/other.html",
        )
        for href in [
            "../../../../outside.html",
            "/docs/en/OTHER/reference/topic.html",
            "https://example.invalid/topic.html",
            "javascript:alert(1)",
        ]:
            with self.subTest(href=href):
                self.assertIsNone(sources.linked_topic(href, source))

    def test_each_corpus_boundary_rejects_tampering(self):
        mutators = {
            "manifest topic": lambda corpus, manifest: manifest["topics"].pop(),
            "mapped topic": lambda corpus, manifest: corpus["mapped_command_topics"].pop(),
            "linked role": lambda corpus, manifest: corpus["linked_context_topics"][0].__setitem__(
                "role", "accepted"
            ),
            "external field": lambda corpus, manifest: corpus.__setitem__(
                "external_publications", []
            ),
            "topic order": lambda corpus, manifest: manifest["topics"].reverse(),
            "gap": lambda corpus, manifest: corpus["source_gaps"][0].__setitem__(
                "topic_sources", ["SSJL4D_6.x/reference-applications/not-pinned.html"]
            ),
            "digest": lambda corpus, manifest: corpus.__setitem__(
                "corpus_sha256", "sha256:" + "0" * 64
            ),
        }
        for name, mutate in mutators.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                self.fixture(root)
                corpus = copy.deepcopy(self.corpus(root))
                manifest = copy.deepcopy(self.manifest(root))
                mutate(corpus, manifest)
                self.write(root, sources.CORPUS_PATH, corpus)
                self.write(root, sources.MANIFEST_PATH, manifest)
                with self.assertRaises(sources.CorpusError):
                    sources.check(root)

    def test_fresh_browser_bytes_win_over_a_poisoned_legacy_cache_entry(self):
        path = "SSJL4D_6.x/reference-applications/commands-api/fresh.html"
        fresh = (
            b'<h1 class="topictitle1">Fresh</h1>'
            b'<div id="lastModifiedDate">Last Updated: 2026-09-10</div>'
        )
        with tempfile.TemporaryDirectory() as temporary:
            cache = Path(temporary) / "cache"
            cache.mkdir()
            (cache / sources.docs_api._key(path)).write_bytes(b"poisoned")
            with patch.object(sources, "fetch_binary", return_value=(200, fresh)):
                bodies = sources.retrieve_topics([path], object())
            sources.publish_bodies(cache, bodies)
            target = sources.cache_target("topic", path, fresh, ".html")
            self.assertEqual(sources.ibm_docs.cached_bytes(cache, target), fresh)

    def test_browser_topic_retrieval_fails_closed(self):
        path = "SSJL4D_6.x/reference-applications/commands-api/missing.html"
        for result in [(404, None), (200, b"no provenance")]:
            with self.subTest(result=result), patch.object(
                sources, "fetch_binary", return_value=result
            ), self.assertRaises(sources.CorpusError):
                sources.retrieve_topics([path], object())

    def test_cache_check_rejects_missing_or_corrupt_html(self):
        failures = [FileNotFoundError("topic"), ValueError("cache SHA-256 mismatch")]
        for failure in failures:
            with self.subTest(failure=type(failure).__name__), tempfile.TemporaryDirectory() as temporary, patch.object(
                sources.ibm_docs, "cached_body", side_effect=failure
            ), self.assertRaises(type(failure)):
                sources.check(ROOT, Path(temporary))

    def test_cache_check_rejects_missing_or_corrupt_toc(self):
        failures = [FileNotFoundError("toc"), ValueError("cache SHA-256 mismatch")]
        for failure in failures:
            with self.subTest(failure=type(failure).__name__), tempfile.TemporaryDirectory() as temporary, patch.object(
                sources.ibm_docs, "select", return_value=([], [object()])
            ), patch.object(
                sources.ibm_docs, "cached_toc", side_effect=failure
            ), self.assertRaises(type(failure)):
                sources.check(ROOT, Path(temporary))

    def test_cache_check_rejects_changed_one_hop_closure(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(
            sources.ibm_docs, "select", return_value=([], [])
        ), patch.object(
            sources, "derive_linked_topics", return_value=([], [])
        ), self.assertRaisesRegex(sources.CorpusError, "closure"):
            sources.check(ROOT, Path(temporary))

    def test_later_batch_corpora_are_fresh_and_close_every_map_gap(self):
        expected = {
            "b": {
                "mapping_rows": 88,
                "mapped_command_topics": 109,
                "linked_context_topics": 63,
                "manual_topics": 13,
                "html_topics": 185,
                "mapping_source_gaps": 0,
                "source_gaps_unresolved": 0,
                "supplemental_topics": 0,
            },
            "c": {
                "mapping_rows": 87,
                "mapped_command_topics": 121,
                "linked_context_topics": 95,
                "manual_topics": 8,
                "html_topics": 224,
                "mapping_source_gaps": 2,
                "source_gaps_unresolved": 0,
                "supplemental_topics": 1,
            },
        }
        for batch, counts in expected.items():
            with self.subTest(batch=batch), patch.object(
                sources, "fetch_binary", side_effect=AssertionError("browser forbidden")
            ):
                sources.check(ROOT, batch=batch)
            corpus = json.loads(
                (ROOT / sources.source_batches.source_batch(batch).corpus_path).read_text()
            )
            mapping = json.loads(
                (ROOT / sources.source_batches.source_batch(batch).map_path).read_text()
            )
            self.assertEqual(corpus["counts"], counts)
            if batch == "b":
                # Four general API topics plus the nine APPC/GDS row-context
                # pins from d6039c69 and 2c9c4895 (#294, #325).
                self.assertEqual(
                    sorted(
                        topic["topic_path"].rsplit("/", 1)[-1]
                        for topic in corpus["manual_topics"]
                    ),
                    [
                        "appcbasic_sl0.html",
                        "appcbasic_sl1.html",
                        "appcbasic_sl2.html",
                        "appcmapped_sl0.html",
                        "appcmapped_sl1.html",
                        "appcmapped_sl2.html",
                        "dfhp3c00237.html",
                        "dfhp4_apiformat.html",
                        "dfhp4_argumentvalues.html",
                        "dfhp4_gdssend.html",
                        "dfhp4_threadsafelist.html",
                        "dfhp616.html",
                        "dfhp625.html",
                    ],
                )
            self.assertEqual(
                set(corpus["mapped_command_topics"]),
                {
                    topic["topic_path"]
                    for row in mapping["rows"]
                    for topic in row["topics"]
                },
            )
            self.assertFalse(corpus["semantic_authority"])
            self.assertEqual(corpus["coverage_credit"], 0)

    def test_sources_c_binds_target_and_cross_product_gap_evidence(self):
        corpus = json.loads(
            (ROOT / sources.source_batches.source_batch("c").corpus_path).read_text()
        )
        self.assertEqual(
            [row["state"] for row in corpus["source_resolutions"]],
            ["target-product-authority", "cross-product-evidence"],
        )
        [trace] = corpus["supplemental_topics"]
        self.assertEqual(trace["topic_path"], sources.TRACE_TOPIC)
        self.assertEqual(trace["bytes"], 20495)
        self.assertEqual(
            trace["sha256"],
            "28e56eef7556509f9a473ae573f6295a56c170ae9a1dd79041c18b15fafd9c83",
        )
        self.assertFalse(trace["target_product_authority"])
        self.assertEqual(
            corpus["source_resolutions"][0]["target_topic_sources"],
            [sources.ASSOCIATION_TOPIC],
        )

    def test_browser_capture_archive_is_identity_checked(self):
        topic = "SSJL4D_6.x/reference-applications/commands-api/example.html"
        body = (
            b'<h1 class="topictitle1">Example</h1>'
            b'<div id="lastModifiedDate">Last Updated: 2026-09-10</div>'
        )

        def archive(sha256: str) -> io.BytesIO:
            metadata = json.dumps(
                [
                    {
                        "topic_path": topic,
                        "bytes": len(body),
                        "sha256": sha256,
                    }
                ]
            ).encode()
            stream = io.BytesIO()
            with tarfile.open(fileobj=stream, mode="w") as output:
                for name, value in [
                    ("browser-meta.json", metadata),
                    (sources.docs_api._key(topic), body),
                ]:
                    info = tarfile.TarInfo(name)
                    info.size = len(value)
                    output.addfile(info, io.BytesIO(value))
            stream.seek(0)
            return stream

        digest = hashlib.sha256(body).hexdigest()
        self.assertEqual(sources.read_browser_capture(archive(digest)), {topic: body})
        with self.assertRaisesRegex(sources.CorpusError, "identity differs"):
            sources.read_browser_capture(archive("0" * 64))


if __name__ == "__main__":
    unittest.main()
