"""Facts-only MQ return membership, tamper guards and deterministic generation."""

import copy
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import mq_status_catalog as authority
import generate_mq_status_catalog as generator
import verify_mq_status_catalog as verifier


class MqStatusCatalogTests(unittest.TestCase):
    def fixture(self, root):
        for path in (authority.CATALOG, authority.registry.SOURCE_LIST_PATH,
                     authority.registry.CONTRACT_CATALOG_PATH, authority.registry.OFFICIAL_CATALOG_PATH,
                     authority.registry.TOPIC_MANIFEST_PATH, Path("rustfmt.toml")):
            target = root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(authority.ROOT / path, target)
        for path in [authority.SUPPLEMENT_MANIFEST, authority.SUPPLEMENT_MANIFEST.parent / "index.json"]:
            target = root / path
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(authority.ROOT / path, target)
        shutil.copytree(authority.ROOT / authority.OUTPUT, root / authority.OUTPUT)

    def reject(self, mutation, refresh=False):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            catalog = authority.load(root)
            mutation(catalog)
            if refresh:
                for call in catalog["calls"]:
                    call["reviewed_projection_sha256"] = authority.digest(authority.projection(call))
            (root / authority.CATALOG).write_text(json.dumps(catalog))
            with self.assertRaises(ValueError):
                authority.load(root)

    def test_exact_reviewed_call_memberships_and_numeric_symbol_consistency(self):
        catalog = authority.load()
        expected = [17,15,16,69,0,34,20,42,61,13,67,23,12,15,96,37,28,18,76,127,137,42,19,14,24,8]
        self.assertEqual([len(call["pairs"]) for call in catalog["calls"]], expected)
        self.assertEqual(sum(len(call["source_positions"]) for call in catalog["calls"]), 27)
        numbers = {}
        for call in catalog["calls"]:
            for pair in call["pairs"]:
                if pair["review"] == "admitted":
                    self.assertEqual(pair["declared_decimal"], int(pair["declared_hex"], 16))
                    self.assertEqual(numbers.setdefault(pair["reason_symbol"], pair["declared_decimal"]), pair["declared_decimal"])
        callback = catalog["calls"][4]
        self.assertEqual(callback["return_kind"], "callback-notification-no-call-return")
        self.assertIsNone(callback["reason_section"])
        self.assertEqual(callback["pairs"], [])
        self.assertEqual(catalog["completion_numeric_mapping"], "reviewed-supplemental")

    def test_reviewed_completion_facts_and_original_canonical_identity_are_fixed(self):
        catalog = authority.load()
        wire = catalog["completion_wire_mapping"]
        # Independently reviewed MQCC constants, not generated Rust expectations.
        self.assertEqual([(f["symbol"], f["decimal"], f["hexadecimal"]) for f in wire["classes"]],
                         [("MQCC_OK", 0, "00000000"), ("MQCC_WARNING", 1, "00000001"),
                          ("MQCC_FAILED", 2, "00000002")])
        self.assertEqual(wire["excluded_unknown"]["decimal"], -1)
        self.assertEqual(wire["canonical_call_return_sha256"],
                         "02bd30d078388918b002fe4e8272bf8c4b3403b1d7e383d580cc1496dd64edaa")
        rendered = generator.render()[authority.OUTPUT / "mod.rs"]
        self.assertIn('"sha256:02bd30d078388918b002fe4e8272bf8c4b3403b1d7e383d580cc1496dd64edaa"', rendered)

    def test_completion_projection_mutants_fail_even_with_recomputed_digest(self):
        mutations = [
            lambda w: w["classes"][0].__setitem__("decimal", 9),
            lambda w: w["classes"][0].__setitem__("decimal", True),
            lambda w: w["classes"][0].__setitem__("decimal", 2**31),
            lambda w: w["classes"][1].__setitem__("symbol", "MQCC_OK"),
            lambda w: w["classes"][2].__setitem__("symbol", "MQCC_UNKNOWN"),
            lambda w: w["classes"].pop(),
            lambda w: w.__setitem__("topic_sha256", "0"*64),
            lambda w: w["topic_manifest"].__setitem__("sha256", "0"*64),
            lambda w: w["topic_manifest"].__setitem__("topic_manifest_digest", "0"*64),
            lambda w: w["classes"][0].__setitem__("first_line", 19),
            lambda w: w["classes"][0].__setitem__("fragment_sha256", "0"*64),
            lambda w: w.__setitem__("review", "pending"),
            lambda w: w.__setitem__("canonical_call_return_sha256", "0"*64),
            lambda w: w["excluded_unknown"].__setitem__("disposition", "admitted"),
            lambda w: w["corroboration"][0].__setitem__("official_row", "foreign"),
        ]
        for mutation in mutations:
            def mutate(catalog):
                wire = catalog["completion_wire_mapping"]
                mutation(wire)
                wire["projection_sha256"] = authority.digest({k:v for k,v in wire.items() if k != "projection_sha256"})
            with self.subTest(mutation=mutation):
                self.reject(mutate)

    def test_supplement_registry_and_manifest_tampering_fail_without_html(self):
        for relative, mutate in [
            (authority.SUPPLEMENT_MANIFEST, lambda d: d["topics"][0].__setitem__("sha256", "0"*64)),
            (authority.SUPPLEMENT_MANIFEST.parent / "index.json", lambda d: d.__setitem__("semantic_authority", True)),
            (authority.SUPPLEMENT_MANIFEST.parent / "index.json", lambda d: d["manifests"][0].__setitem__("manifest_sha256", "sha256:"+"0"*64)),
            (authority.SUPPLEMENT_MANIFEST.parent / "index.json", lambda d: d["manifests"].append(copy.deepcopy(d["manifests"][0]))),
        ]:
            with self.subTest(relative=relative), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.fixture(root)
                document = json.loads((root / relative).read_text())
                mutate(document)
                (root / relative).write_text(json.dumps(document))
                with self.assertRaises(ValueError):
                    authority.load(root)

    def test_pending_reviews_and_coherent_reason_alias_changes_are_immutable(self):
        def promote(catalog):
            pair = next(p for c in catalog["calls"] for p in c["pairs"] if p["review"] == "pending-symbol-conflict")
            pair["review"] = "admitted"
        self.reject(promote, refresh=True)
        def alias(catalog):
            pair = catalog["calls"][0]["pairs"][0]
            pair["declared_decimal"] = 2192
            pair["declared_hex"] = "890"
        self.reject(alias, refresh=True)

    def test_independent_completion_table_parser_rejects_numeric_aliases_and_overflow(self):
        lines = ["MQCC_* (completion codes)", "Last Updated: 2026-05-18", "Table 1. Values of constants",
                 "Name", "|", "Decimal value", "|", "Hexadecimal value", "|",
                 "MQCC_OK |", "0 |", "X'00000000' |", "MQCC_WARNING |", "1 |", "X'00000001' |",
                 "MQCC_FAILED |", "2 |", "X'00000002' |", "MQCC_UNKNOWN |", "-1 |", "X'FFFFFFFF' |",
                 "Parent topic:", "fixture-parent"]
        self.assertEqual(verifier.project_completions(lines), [("MQCC_OK",0,"00000000"),
                         ("MQCC_WARNING",1,"00000001"), ("MQCC_FAILED",2,"00000002"),
                         ("MQCC_UNKNOWN",-1,"FFFFFFFF")])
        for index, value in [(10,"1 |"), (12,"MQCC_OK |"), (16,"2147483648 |"),
                             (17,"X'FFFFFFFF' |"), (9,"MQRC_CONTEXT |")]:
            changed = lines.copy();changed[index] = value
            with self.subTest(index=index), self.assertRaises(ValueError):
                verifier.project_completions(changed)
        catalog = authority.load()
        for tool in [generator, verifier]:
            with patch.object(authority, "wire_source_lines", return_value={catalog["completion_wire_mapping"]["topic_path"]:lines}):
                with self.assertRaises((ValueError, KeyError)):
                    tool.reproduce_wire(catalog, Path("unused"), Path("unused"))

    def test_pending_and_duplicate_source_occurrences_are_not_silently_corrected(self):
        catalog = authority.load()
        pairs = [pair for call in catalog["calls"] for pair in call["pairs"]]
        pending = [pair for pair in pairs if pair["review"] != "admitted"]
        self.assertEqual(len(pending), 10)
        self.assertEqual(sum(pair["review"] == "pending-number" for pair in pending), 2)
        self.assertEqual(sum(pair["review"] == "pending-numeric-conflict" for pair in pending), 5)
        self.assertEqual(sum(pair["review"] == "pending-symbol-conflict" for pair in pending), 2)
        self.assertEqual(sum(pair["review"] == "pending-symbol-spelling" for pair in pending), 1)
        duplicates = [pair for pair in pairs if len(pair["source_locations"]) > 1]
        self.assertEqual(len(duplicates), 1)
        self.assertEqual(duplicates[0]["reason_symbol"], "MQRC_SUB_USER_DATA_ERROR")

    def test_missing_duplicate_foreign_call_and_pins_fail(self):
        for mutate in [lambda c: c["calls"].pop(),
                       lambda c: c["calls"].__setitem__(1, copy.deepcopy(c["calls"][0])),
                       lambda c: c["calls"][0].__setitem__("label", "MQFOREIGN"),
                       lambda c: c["calls"][15].__setitem__("topic_sha256", "0"*64),
                       lambda c: c["calls"][17].__setitem__("source_positions", [18]),
                       lambda c: c["topic_manifest"].__setitem__("sha256", "f"*64)]:
            with self.subTest(mutate=mutate):
                self.reject(mutate)

    def test_pair_tampering_duplicates_numeric_conflicts_and_missing_pairs_fail(self):
        for mutate in [lambda c: c["calls"][0]["pairs"].pop(),
                       lambda c: c["calls"][0]["pairs"].append(copy.deepcopy(c["calls"][0]["pairs"][0])),
                       lambda c: c["calls"][0]["pairs"][0].__setitem__("completion_symbol", "MQCC_OTHER"),
                       lambda c: c["calls"][0]["pairs"][0].__setitem__("declared_decimal", 999),
                       lambda c: c["calls"][0]["pairs"][0].__setitem__("declared_hex", "GG"),
                       lambda c: c["calls"][0]["pairs"][0].__setitem__("declared_decimal", True),
                       lambda c: c["calls"][4].__setitem__("pairs", copy.deepcopy(c["calls"][0]["pairs"])),
                       lambda c: c["calls"][0]["pairs"][0]["source_locations"][0].__setitem__("number_line", 2001)]:
            with self.subTest(mutate=mutate):
                self.reject(mutate, refresh=True)
        self.reject(lambda c: c["calls"][0]["pairs"][0].__setitem__("reason_symbol", "MQRC_INVENTED"))

    def test_pending_number_cannot_be_promoted_without_consistent_identity(self):
        def mutate(catalog):
            pair = next(p for p in catalog["calls"][5]["pairs"] if p["review"] == "pending-number")
            pair["review"] = "admitted"
        self.reject(mutate, refresh=True)

    def test_bounded_malformed_objects_and_duplicate_json_fields_fail(self):
        self.reject(lambda c: c["calls"][0].__setitem__("reason_section", {"first_line": "21", "last_line":59}), refresh=True)
        self.reject(lambda c: c["calls"][0]["context_notes"][0].__setitem__("first_line", None), refresh=True)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input.json"
            path.write_text('{"status": 1, "status": 2}')
            with self.assertRaisesRegex(ValueError, "duplicate JSON field"):
                authority.read_json(path)
            path.write_text(" " * (2*1024*1024+1))
            with self.assertRaisesRegex(ValueError, "byte bound"):
                authority.read_json(path)

    def test_return_projection_excludes_context_options_and_usage_reasons(self):
        # Authored parser fixture, no publication prose or external HTML.
        lines = ["MQRC_CONTEXT_ONLY", "Reason", "Type", "If CompCode is MQCC_OK:",
                 "MQRC_NONE", "(0, X'000')", "If CompCode is MQCC_FAILED:",
                 "MQRC_TEST", "(123, X'07B')", "For detailed information",
                 "MQRC_USAGE_ONLY", "(456, X'1C8')"]
        pairs = verifier.project_pairs(lines, 2, 9)
        self.assertEqual([(p["completion_symbol"], p["reason_symbol"]) for p in pairs],
                         [("MQCC_OK", "MQRC_NONE"), ("MQCC_FAILED", "MQRC_TEST")])
        lines[3] = "MQCC_OK"
        with self.assertRaisesRegex(ValueError, "outside explicit completion"):
            verifier.project_pairs(lines, 2, 9)
        lines[3] = "If CompCode is MQCC_OK:"
        lines[9] = "Usage notes"
        with self.assertRaisesRegex(ValueError, "boundary"):
            verifier.project_pairs(lines, 2, 9)

    def test_numeric_conflicts_placeholders_malformed_symbols_stay_pending(self):
        lines = ["Reason", "If CompCode is MQCC_FAILED:", "MQRC_TEST", "(123, X'07C')",
                 "MQRC_UNKNOWN", "(nnnn, X'xxx')", "MQRC_MALFORMED_ SYMBOL", "(456, X'1C8')",
                 "For more information about these codes"]
        pairs = verifier.project_pairs(lines,1,8)
        self.assertEqual([p["review"] for p in pairs],
                         ["pending-numeric-conflict", "pending-number", "pending-symbol-spelling"])
        other = copy.deepcopy(pairs[0]);other.update(declared_decimal=124,declared_hex="07C",review="admitted")
        pairs[0]["declared_hex"]="07B";pairs[0]["review"]="admitted"
        calls=[{"pairs":pairs},{"pairs":[other]}]
        verifier.apply_symbol_conflicts(calls)
        self.assertEqual(pairs[0]["review"], "pending-symbol-conflict")
        self.assertEqual(other["review"], "pending-symbol-conflict")

    def test_generation_is_fresh_bounded_and_rejects_stale_foreign_files(self):
        generator.check()
        rendered = generator.render()
        self.assertTrue(all(len(text.splitlines()) < 1200 for text in rendered.values()))
        self.assertEqual(sum(text.count("MqStatusPairDescriptor {") for text in rendered.values()), 1030)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            path = root / authority.OUTPUT / "row_0001_0.rs"
            original = path.read_text();path.write_text(original+"// stale\n")
            with self.assertRaisesRegex(ValueError, "stale"):
                generator.check(root)
            path.write_text(original)
            (root / authority.OUTPUT / "foreign.rs").write_text("// foreign\n")
            with self.assertRaisesRegex(ValueError, "module set"):
                generator.check(root)


if __name__ == "__main__":
    unittest.main()
