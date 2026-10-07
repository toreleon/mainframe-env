"""Independent identity expectations and rejection contracts for SPI/FEPI maps."""

import copy
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
import cics_system_source_map as source_map  # noqa: E402


class CicsSystemSourceMapTests(unittest.TestCase):
    def fixture(self, root):
        paths = [
            "conformance/subsystems/coverage/catalogs/cics.json",
            "conformance/subsystems/coverage/manifests/cics-topics.json",
            "conformance/subsystems/cics/system/cics/spi-fepi-source-authority.json",
            str(source_map.FORM_LOCATORS),
            "conformance/subsystems/cics/system/generated/cics-spi-fepi-identity-catalog.json",
            "conformance/subsystems/cics/system/manifests/index.json",
            *(row["manifest"] for row in json.loads(
                (ROOT / "conformance/subsystems/cics/system/manifests/index.json").read_text()
            )["manifests"]),
            *(str(p) for family in ["spi", "fepi"] for p in source_map.artifact_paths(family)),
        ]
        for relative in paths:
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)

    def read(self, family, index, root=ROOT):
        return json.loads((root / source_map.artifact_paths(family)[index]).read_text())

    def test_frozen_rows_and_source_counts_preserve_zero_credit(self):
        for family, rows, pages in [("spi", 269, 267), ("fepi", 39, 36)]:
            source_map.run(ROOT, family, True, None, None)
            mapping = self.read(family, 1)
            self.assertEqual(mapping["counts"]["row_count"], rows)
            self.assertEqual(mapping["counts"]["unique_topic_count"], pages)
            self.assertEqual(len({row["official_row"] for row in mapping["rows"]}), rows)
            for key in ["semantic_authority", "automatic_registration", "public_routes"]:
                self.assertIs(mapping[key], False)
            self.assertEqual((mapping["coverage_credit"], mapping["differential_credit"]), (0, 0))
            self.assertEqual(mapping["counts"]["mapped_row_count"], 266 if family == "spi" else 39)
            self.assertEqual(mapping["counts"]["unresolved_row_count"], 3 if family == "spi" else 0)

    def test_independent_qualified_and_list_page_expectations(self):
        # Reviewed EIBFN rows and command-page headings, independent of selection code.
        expected = {
            ("spi", "0201"): ("PERFORM SECURITY", "6402", "dfha8ee__title__1", "PERFORM SECURITY REBUILD"),
            ("spi", "0203"): ("PERFORM SSL", "6412", "dfha8_performssl__title__1", "PERFORM SSL REBUILD"),
            ("spi", "0204"): ("PERFORM STATISTICS", "7006", "dfha81l__title__1", "PERFORM STATISTICS RECORD"),
            ("fepi", "0033"): ("FEPI SET NODELIST", "8444", "dfhp73e__title__1", "FEPI SET NODE"),
            ("fepi", "0035"): ("FEPI SET POOLLIST", "8464", "dfhp73f__title__1", "FEPI SET POOL"),
            ("fepi", "0037"): ("FEPI SET TARGETLIST", "8484", "dfhp73g__title__1", "FEPI SET TARGET"),
        }
        for (family, ordinal), values in expected.items():
            row = next(r for r in self.read(family, 1)["rows"] if r["official_row"].endswith(":" + ordinal))
            self.assertEqual((row["label"], row["eibfn"], row["topic"]["heading_id"], row["topic"]["heading_label"]), values)

    def test_annotations_and_shared_eibfn_never_change_identity(self):
        spi = self.read("spi", 1)["rows"]
        self.assertEqual(spi[147]["label"], "6.3 and later INQUIRE OTEL")
        self.assertEqual(spi[188]["label"], "INQUIRE VTAM 1")
        fepi = self.read("fepi", 1)["rows"]
        self.assertEqual(fepi[31]["eibfn"], fepi[32]["eibfn"])
        self.assertNotEqual(fepi[31]["official_row"], fepi[32]["official_row"])
        self.assertEqual(fepi[31]["topic"], fepi[32]["topic"])
        self.assertEqual(fepi[32]["shared_eibfn_rows"], [fepi[31]["official_row"]])

    def test_rehashed_map_mutations_cannot_change_expected_rows_or_credit(self):
        mutations = [
            lambda m: m["rows"].reverse(),
            lambda m: m["rows"][0].update(label="GENERIC SUCCESS"),
            lambda m: m["rows"][0].update(eibfn="0000"),
            lambda m: m["rows"][0].update(official_row="foreign:0001"),
            lambda m: m["rows"][0]["topic"].update(sha256="sha256:" + "0" * 64),
            lambda m: m.update(coverage_credit=1),
            lambda m: m.update(public_routes=True),
            lambda m: m["rows"][200].update(state="mapped"),
            lambda m: m["rows"].pop(),
        ]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            original = self.read("spi", 1, root)
            path = root / source_map.artifact_paths("spi")[1]
            for mutate in mutations:
                candidate = copy.deepcopy(original)
                mutate(candidate)
                candidate["mapping_sha256"] = source_map.shared.canonical_digest(
                    source_map.MAP_DOMAIN, source_map.core(candidate, "mapping_sha256"))
                path.write_text(source_map.shared.pretty(candidate))
                with self.assertRaises(source_map.shared.SourceMapError):
                    source_map.run(root, "spi", True, None, None)

    def test_projection_mutation_rejects_rehashed_wrong_body_and_foreign_family(self):
        for mutate in [lambda p: p.update(family="fepi"),
                       lambda p: p["topics"][0].update(bytes=1),
                       lambda p: p["topics"][0].update(sha256="sha256:" + "0" * 64),
                       lambda p: p["topics"].reverse(),
                       lambda p: p.update(semantic_authority=True)]:
            p = self.read("spi", 0)
            mutate(p)
            p["projection_sha256"] = source_map.shared.canonical_digest(
                source_map.PROJECTION_DOMAIN, source_map.core(p, "projection_sha256"))
            with self.assertRaises(source_map.shared.SourceMapError):
                source_map.validate_projection(ROOT, "spi", p)

    def test_retained_body_verifier_rejects_heading_and_missing_form_symbol(self):
        mapping = self.read("fepi", 1)
        row = next(r for r in mapping["rows"] if r["label"] == "FEPI SET NODELIST")
        mapping["rows"] = [row]
        for body in [b'<h1 id="wrong">FEPI SET NODE</h1>NODELIST',
                     b'<h1 id="dfhp73e__title__1">FEPI SET NODE</h1>']:
            with patch.object(source_map, "body_bytes", return_value=body):
                with self.assertRaises(source_map.shared.SourceMapError):
                    source_map.verify_bodies(ROOT, "fepi", mapping, Path("unused-external-cache"))

    def test_source_reproduction_requires_both_external_inputs(self):
        for toc, cache in [(b"{}", None), (None, Path("unused-external-cache"))]:
            with self.assertRaisesRegex(source_map.shared.SourceMapError, "both --toc and --cache"):
                source_map.run(ROOT, "spi", True, toc, cache)

    def test_wrong_toc_hash_is_rejected_before_projection(self):
        with self.assertRaisesRegex(source_map.shared.SourceMapError, "TOC SHA-256 differs"):
            source_map.projection_from_source(ROOT, "spi", b"{}", Path("unused-external-cache"))

    def test_qualified_form_promotion_in_review_input_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            p = root / source_map.FORM_LOCATORS
            review = json.loads(p.read_text())
            next(row for row in review["rows"] if row["label"] == "PERFORM SECURITY")["state"] = "mapped"
            p.write_text(source_map.shared.pretty(review))
            with self.assertRaisesRegex(source_map.shared.SourceMapError, "reviewed form locator differs"):
                source_map.build_mapping(root, "spi", self.read("spi", 0, root))
