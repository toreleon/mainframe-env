"""Deterministic tests for the zero-credit CIC-901 sources-a corpus."""

import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch


TOOL = Path(__file__).resolve().parents[1] / "fetch_cics_application_sources.py"
ROOT = TOOL.parents[3]
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
            Path("conformance/0.2/catalogs/cics.json"),
            sources.CORPUS_PATH,
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
            "sha256:54806ff4096b6d9b2a5c7b7f8bf398a938db22277be70beedaa02d16e0992b92",
        )
        self.assertEqual(
            corpus["topic_manifest"]["file_sha256"],
            "sha256:" + hashlib.sha256(manifest_bytes).hexdigest(),
        )
        self.assertEqual(
            manifest["topic_manifest_digest"],
            sources.docs_api.manifest_digest(manifest["topics"]),
        )

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


if __name__ == "__main__":
    unittest.main()
