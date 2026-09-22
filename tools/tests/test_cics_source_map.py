import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest


TOOL = Path(__file__).resolve().parents[1] / "generate_cics_source_map.py"
ROOT = TOOL.parent.parent
sys.path.insert(0, str(TOOL.parent))
SPEC = importlib.util.spec_from_file_location("generate_cics_source_map", TOOL)
source_map = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = source_map
SPEC.loader.exec_module(source_map)


class CicsSourceMapTests(unittest.TestCase):
    def fixture(self, root: Path) -> None:
        for relative in [
            source_map.descriptors.CATALOG_PATH,
            Path("conformance/0.2/catalogs/cics.json"),
            source_map.TOC_PROJECTION_PATH,
            *(config.map_path for config in source_map.BATCHES.values()),
        ]:
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)

    def mapping(self, root: Path = ROOT) -> dict:
        return json.loads((root / source_map.MAP_PATH).read_text())

    def projection(self, root: Path = ROOT) -> dict:
        return json.loads((root / source_map.TOC_PROJECTION_PATH).read_text())

    def batch_mapping(self, batch_id: str, root: Path = ROOT) -> dict:
        return json.loads((root / source_map.batch_config(batch_id).map_path).read_text())

    def write(self, root: Path, relative: Path, value: object) -> None:
        (root / relative).write_text(json.dumps(value, indent=2) + "\n")

    def test_repository_mapping_is_fresh_bounded_and_zero_credit(self):
        source_map.check(ROOT)
        mapping = self.mapping()
        self.assertEqual(
            mapping["counts"],
            {
                "row_count": 88,
                "resolved_row_count": 85,
                "source_gap_count": 3,
                "edge_count": 117,
                "unique_topic_count": 109,
                "multi_topic_row_count": 9,
                "shared_topic_count": 7,
                "selection_kind_counts": {
                    "exact": 61,
                    "variant-set": 9,
                    "shared-page": 9,
                    "combined-page": 6,
                    "source-gap": 3,
                },
            },
        )
        self.assertEqual(mapping["status"], "candidate")
        self.assertFalse(mapping["semantic_authority"])
        self.assertFalse(mapping["automatic_registration"])
        self.assertEqual(mapping["coverage_credit"], 0)
        self.assertEqual(mapping["differential_credit"], 0)

    def test_source_gaps_are_exact_and_not_aliased_to_neighboring_commands(self):
        gaps = [row for row in self.mapping()["rows"] if row["state"] == "source-gap"]
        self.assertEqual(
            [(row["label"], row["eibfn"], row["topics"]) for row in gaps],
            [
                ("CICSMESSAGE", "6C12", []),
                ("DUMP", "1C02", []),
                ("ENTER TRACEID", "1A04", []),
            ],
        )
        mapped = {row["label"]: row for row in self.mapping()["rows"]}
        self.assertEqual(mapped["DUMP TRANSACTION"]["eibfn"], "7E02")
        self.assertEqual(mapped["ENTER TRACENUM"]["eibfn"], "4802")

    def test_sources_b_and_c_cover_the_remaining_catalog_rows(self):
        expected = {
            "sources-b": {
                "row_count": 88,
                "resolved_row_count": 88,
                "source_gap_count": 0,
                "edge_count": 113,
                "unique_topic_count": 109,
                "multi_topic_row_count": 7,
                "shared_topic_count": 3,
                "selection_kind_counts": {
                    "exact": 73,
                    "variant-set": 8,
                    "shared-page": 5,
                    "combined-page": 2,
                    "source-gap": 0,
                },
            },
            "sources-c": {
                "row_count": 87,
                "resolved_row_count": 85,
                "source_gap_count": 2,
                "edge_count": 127,
                "unique_topic_count": 121,
                "multi_topic_row_count": 13,
                "shared_topic_count": 4,
                "selection_kind_counts": {
                    "exact": 58,
                    "variant-set": 15,
                    "shared-page": 6,
                    "combined-page": 4,
                    "aliased-page": 2,
                    "source-gap": 2,
                },
            },
        }
        all_rows = []
        for batch_id in source_map.BATCHES:
            source_map.check_batch(ROOT, batch_id)
            mapping = self.batch_mapping(batch_id)
            all_rows.extend(row["official_row"] for row in mapping["rows"])
            if batch_id in expected:
                self.assertEqual(mapping["counts"], expected[batch_id])
        self.assertEqual(
            all_rows,
            [
                f"ibm-cics-ts-6x-2026-08-31:api-commands:{ordinal:04d}"
                for ordinal in range(1, 264)
            ],
        )

    def test_sources_b_and_c_gaps_variants_and_aliases_are_explicit(self):
        b_rows = {
            row["label"]: row for row in self.batch_mapping("sources-b")["rows"]
        }
        c_rows = {
            row["label"]: row for row in self.batch_mapping("sources-c")["rows"]
        }
        self.assertEqual(
            b_rows["LINK ACQACTIVITY"]["topics"][0]["topic_path"],
            b_rows["LINK ACTIVITY"]["topics"][0]["topic_path"],
        )
        self.assertEqual(b_rows["LINK ACTIVITY"]["selection_kind"], "shared-page")
        self.assertEqual(b_rows["LINK ACTIVITY"]["topics"][0]["role"], "shared")
        self.assertEqual(
            [row["label"] for row in c_rows.values() if row["state"] == "source-gap"],
            ["SET ASSOCIATION USERCORRDATA", "TRACE"],
        )
        self.assertEqual(len(b_rows["RECEIVE"]["topics"]), 20)
        self.assertEqual(len(c_rows["SEND"]["topics"]), 26)
        self.assertEqual(
            [topic["toc_label"] for topic in c_rows["START"]["topics"]],
            ["START", "START CHANNEL"],
        )
        self.assertEqual(
            (
                c_rows["WAIT"]["selection_kind"],
                c_rows["WAIT"]["topics"][0]["toc_label"],
            ),
            ("aliased-page", "GDS WAIT"),
        )
        self.assertEqual(
            (
                c_rows["WRITE FILE"]["selection_kind"],
                c_rows["WRITE FILE"]["topics"][0]["toc_label"],
            ),
            ("aliased-page", "WRITE"),
        )

    def test_batch_specific_check_rejects_cross_batch_content(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            mapping = self.batch_mapping("sources-b", root)
            mapping["rows"][0]["official_row"] = mapping["rows"][1]["official_row"]
            self.write(root, source_map.SOURCES_B.map_path, mapping)
            with self.assertRaises(source_map.SourceMapError):
                source_map.check_batch(root, "sources-b")

    def test_shared_combined_and_variant_page_shapes_are_explicit(self):
        rows = {row["label"]: row for row in self.mapping()["rows"]}
        acquire = {topic["topic_path"] for topic in rows["ACQUIRE PROCESS"]["topics"]}
        self.assertEqual(
            acquire,
            {"SSJL4D_6.x/reference-applications/commands-bts/dfhp4_acquire.html"},
        )
        self.assertEqual(rows["ACQUIRE PROCESS"]["selection_kind"], "shared-page")
        self.assertEqual(rows["ACQUIRE ACTIVITYID"]["topics"], rows["ACQUIRE PROCESS"]["topics"])
        self.assertEqual(rows["DEFINE COUNTER"]["topics"], rows["DEFINE DCOUNTER"]["topics"])
        self.assertEqual(rows["CONVERSE"]["selection_kind"], "variant-set")
        self.assertEqual(len(rows["CONVERSE"]["topics"]), 22)
        self.assertEqual(len(rows["ALLOCATE"]["topics"]), 3)
        self.assertEqual(len(rows["FREE"]["topics"]), 4)

    def test_digests_are_independently_recomputed_from_canonical_projections(self):
        projection = self.projection()
        projection_core = {
            "source_toc_sha256": projection["source_toc_sha256"],
            "summary": projection["summary"],
            "topics": projection["topics"],
        }
        projection_bytes = json.dumps(
            projection_core, ensure_ascii=True, separators=(",", ":"), sort_keys=True
        ).encode()
        self.assertEqual(
            projection["projection_sha256"],
            "sha256:"
            + hashlib.sha256(
                b"mainframe-env.cics-command-summary-topics@1\0" + projection_bytes
            ).hexdigest(),
        )
        self.assertEqual(
            projection["projection_sha256"],
            "sha256:09b341b14694259acbda538811594876c14e006dead02e6d84bec6a93422d249",
        )

        mapping = self.mapping()
        mapping_core = {
            key: mapping[key]
            for key in ["catalog", "toc_projection", "row_range", "rows"]
        }
        mapping_bytes = json.dumps(
            mapping_core, ensure_ascii=True, separators=(",", ":"), sort_keys=True
        ).encode()
        self.assertEqual(
            mapping["mapping_sha256"],
            "sha256:"
            + hashlib.sha256(
                b"mainframe-env.cics-command-source-map@1\0" + mapping_bytes
            ).hexdigest(),
        )
        self.assertEqual(
            mapping["mapping_sha256"],
            "sha256:c535f2cac1072e16a0f3e252045cac975934c5e24d405a560a4e0ad14e0de726",
        )

    def test_external_toc_projection_is_structural_and_digest_checked(self):
        committed = self.projection()
        children = [
            {
                "label": topic["label"],
                "href": topic["topic_path"],
                "topicId": topic["topic_id"],
            }
            for topic in committed["topics"]
        ]
        raw = json.dumps(
            {
                "unrelated": [{"href": "elsewhere"}],
                "topics": [
                    {
                        "label": source_map.SUMMARY_LABEL,
                        "href": source_map.SUMMARY_PATH,
                        "topicId": source_map.SUMMARY_TOPIC_ID,
                        "topics": children,
                    }
                ],
            }
        ).encode()
        digest = "sha256:" + hashlib.sha256(raw).hexdigest()
        projected = source_map.projection_from_toc(raw, digest)
        self.assertEqual(projected["topics"], committed["topics"])
        with self.assertRaisesRegex(source_map.SourceMapError, "SHA-256 differs"):
            source_map.projection_from_toc(raw + b" ", digest)

    def test_mapping_rejects_missing_reordered_or_changed_catalog_rows(self):
        mutators = {
            "missing": lambda value: value["rows"].pop(),
            "reordered": lambda value: value["rows"].reverse(),
            "label": lambda value: value["rows"][0].__setitem__("label", "CHANGED"),
            "topic": lambda value: value["rows"][0]["topics"][0].__setitem__(
                "topic_path", value["rows"][1]["topics"][0]["topic_path"]
            ),
        }
        for name, mutate in mutators.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                self.fixture(root)
                mapping = self.mapping(root)
                mutate(mapping)
                self.write(root, source_map.MAP_PATH, mapping)
                with self.assertRaises(source_map.SourceMapError):
                    source_map.check(root)

    def test_count_preserving_ambiguous_toc_labels_are_rejected(self):
        projection = copy.deepcopy(self.projection())
        changed = next(
            topic
            for topic in projection["topics"]
            if topic["label"] == "BUILD ATTACH (MRO)"
        )
        changed["label"] = "ABEND"
        projection["projection_sha256"] = source_map.canonical_digest(
            source_map.TOC_DOMAIN, source_map.projection_core(projection)
        )
        with self.assertRaisesRegex(source_map.SourceMapError, "ABEND exact mapping"):
            source_map.build_mapping(ROOT, projection)

    def test_projection_rejects_duplicate_path_bad_ordinal_and_digest_drift(self):
        mutators = {
            "duplicate": lambda value: value["topics"][1].__setitem__(
                "topic_path", value["topics"][0]["topic_path"]
            ),
            "ordinal": lambda value: value["topics"][0].__setitem__("ordinal", 2),
            "digest": lambda value: value.__setitem__("projection_sha256", "sha256:" + "0" * 64),
            "definition": lambda value: value.__setitem__(
                "projection_digest_definition", "different"
            ),
            "traversal": lambda value: value["topics"][0].__setitem__(
                "topic_path",
                "SSJL4D_6.x/reference-applications/commands-api/../../outside/forged.html",
            ),
        }
        for name, mutate in mutators.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                self.fixture(root)
                projection = self.projection(root)
                mutate(projection)
                if name == "traversal":
                    projection["projection_sha256"] = source_map.canonical_digest(
                        source_map.TOC_DOMAIN, source_map.projection_core(projection)
                    )
                self.write(root, source_map.TOC_PROJECTION_PATH, projection)
                with self.assertRaises(source_map.SourceMapError):
                    source_map.check(root)

    def test_coverage_or_registration_claims_are_rejected(self):
        cases = [
            ("coverage_credit", 1),
            ("differential_credit", 1),
            ("automatic_registration", True),
            ("semantic_authority", True),
        ]
        for field, value in cases:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                self.fixture(root)
                mapping = self.mapping(root)
                mapping[field] = value
                self.write(root, source_map.MAP_PATH, mapping)
                with self.assertRaises(source_map.SourceMapError):
                    source_map.check(root)

    def test_noncanonical_generated_json_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            path = root / source_map.MAP_PATH
            path.write_text(path.read_text().rstrip() + "  \n")
            with self.assertRaisesRegex(source_map.SourceMapError, "canonical"):
                source_map.check(root)


if __name__ == "__main__":
    unittest.main()
