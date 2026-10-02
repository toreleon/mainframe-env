from __future__ import annotations

import copy
import importlib.util
import json
from pathlib import Path
import sys
import unittest


TOOL = Path(__file__).resolve().parents[1] / "generate_spi1001_catalog.py"
SPEC = importlib.util.spec_from_file_location("generate_spi1001_catalog", TOOL)
catalog_tool = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = catalog_tool
SPEC.loader.exec_module(catalog_tool)


class Spi1001CatalogTests(unittest.TestCase):
    def setUp(self) -> None:
        self.authority, self.official = catalog_tool.source.validate_authority()

    def test_repository_catalog_is_fresh_and_exact(self) -> None:
        catalog_tool.check()
        catalog = json.loads((catalog_tool.ROOT / catalog_tool.OUTPUT_PATH).read_text())
        self.assertEqual(catalog["counts"], {
            "spi": 269,
            "fepi": 39,
            "total": 308,
            "raw_source_rows": 312,
            "deduplicated_source_rows": 4,
        })
        self.assertEqual(len(catalog["commands"]), 308)
        self.assertEqual(catalog["coverage_credit"], 0)

    def test_every_identity_is_semantically_blocked_and_unrouted(self) -> None:
        catalog = catalog_tool.build_catalog(self.authority, self.official)
        for command in catalog["commands"]:
            self.assertEqual(command["semantic_contract"]["state"], "blocked-source-gap")
            self.assertEqual(
                command["semantic_contract"]["blocked_facts"],
                catalog_tool.source.EXPECTED_BLOCKED_FACTS,
            )
            self.assertEqual(command["source_label_aliases"], [])
            self.assertEqual(
                command["runtime"],
                {"handler": None, "advertised": False, "automatically_registered": False},
            )
            self.assertEqual(command["coverage_credit"], 0)

    def test_spi_deduplication_and_fepi_shared_codes_are_preserved(self) -> None:
        commands = catalog_tool.build_catalog(self.authority, self.official)["commands"]
        by_label = {(row["interface"], row["label"]): row for row in commands}
        self.assertEqual(
            by_label[("SPI", "INQUIRE NETNAME")]["additional_eibfn_codes"], ["5206"]
        )
        allocate = by_label[("FEPI", "FEPI ALLOCATE PASSCONVID")]
        self.assertEqual(allocate["eibfn"], "8210")
        self.assertEqual(len(allocate["shared_eibfn_rows"]), 1)
        self.assertIn("fepi-commands:0003", allocate["shared_eibfn_rows"][0])

    def test_missing_official_identity_is_rejected(self) -> None:
        mutated = copy.deepcopy(self.official)
        unit = next(unit for unit in mutated["units"] if unit["id"] == "fepi-commands")
        unit["rows"].pop()
        with self.assertRaisesRegex(catalog_tool.CatalogError, "FEPI denominator"):
            catalog_tool.build_catalog(self.authority, mutated)

    def test_duplicate_official_identity_is_rejected(self) -> None:
        mutated = copy.deepcopy(self.official)
        unit = next(unit for unit in mutated["units"] if unit["id"] == "fepi-commands")
        unit["rows"][-1] = copy.deepcopy(unit["rows"][0])
        with self.assertRaisesRegex(catalog_tool.CatalogError, "row identities are duplicated"):
            catalog_tool.build_catalog(self.authority, mutated)

    def test_foreign_interface_locator_is_rejected(self) -> None:
        mutated = copy.deepcopy(self.official)
        unit = next(unit for unit in mutated["units"] if unit["id"] == "fepi-commands")
        unit["rows"][0]["source_locator"] = unit["rows"][0]["source_locator"].replace(
            "family:FEPI", "family:SPI"
        )
        with self.assertRaisesRegex(
            catalog_tool.source.SourceAuthorityError, "official interface differs"
        ):
            catalog_tool.build_catalog(self.authority, mutated)

    def test_source_gap_weakening_is_rejected(self) -> None:
        mutated = copy.deepcopy(self.authority)
        mutated["projection_boundary"]["semantic_authority"] = True
        with self.assertRaisesRegex(
            catalog_tool.source.SourceAuthorityError, "granted semantic"
        ):
            catalog_tool.build_catalog(mutated, self.official)

    def test_identity_digest_is_order_and_content_sensitive(self) -> None:
        commands = catalog_tool.build_catalog(self.authority, self.official)["commands"]
        reordered = list(reversed(commands))
        self.assertNotEqual(
            catalog_tool.identity_digest(commands), catalog_tool.identity_digest(reordered)
        )
        changed = copy.deepcopy(commands)
        changed[0]["label"] = "MUTATED"
        self.assertNotEqual(
            catalog_tool.identity_digest(commands), catalog_tool.identity_digest(changed)
        )

    def test_rust_registry_is_deterministic_and_has_no_dispatch_surface(self) -> None:
        catalog = catalog_tool.final_catalog()
        rendered = catalog_tool.render_rust_from_catalog(catalog).decode()
        self.assertEqual(rendered.count("CicsAdministrativeCommandIdentity {"), 308)
        self.assertIn("CICS_SPI_FEPI_AUTOMATIC_REGISTRATION: bool = false", rendered)
        self.assertIn("CICS_SPI_FEPI_PUBLIC_ROUTES: bool = false", rendered)
        self.assertNotIn("handler_id", rendered)
        self.assertNotIn("runtime_operation", rendered)

    def test_rust_registry_rejects_advertisement_mutation(self) -> None:
        catalog = catalog_tool.final_catalog()
        catalog["commands"][0]["runtime"]["advertised"] = True
        catalog["identity_sha256"] = catalog_tool.identity_digest(catalog["commands"])
        with self.assertRaisesRegex(catalog_tool.CatalogError, "executable or credited"):
            catalog_tool.render_rust_from_catalog(catalog)


class AdministrativeGrammarTests(unittest.TestCase):
    def setUp(self) -> None:
        root = catalog_tool.ROOT
        self.family = json.loads((root / catalog_tool.FAMILY_PATH / "spi-program.json").read_text())
        self.mapping = json.loads((root / "conformance/0.10/cics/spi-command-source-map.json").read_text())
        self.manifest = json.loads((root / "conformance/0.10/manifests/cics-spi-command-topics.json").read_text())

    def test_enrolled_source_cohorts_are_disjoint_pinned_and_schema_declared(self) -> None:
        root = catalog_tool.ROOT
        schema = json.loads((root / "conformance/0.10/schemas/cics-system-family-contract.schema.json").read_text())
        self.assertEqual(set(catalog_tool.FAMILY_ROWS), set(schema["properties"]["family"]["enum"]))
        seen = set()
        for _, (interface, suffixes) in catalog_tool.FAMILY_ROWS.items():
            self.assertTrue(0 < len(suffixes) <= 32)
            mapping = json.loads((root / f"conformance/0.10/cics/{interface}-command-source-map.json").read_text())
            manifest = json.loads((root / f"conformance/0.10/manifests/cics-{interface}-command-topics.json").read_text())
            mapped = {row["official_row"]: row for row in mapping["rows"]}
            pinned = {topic["topic_path"]: topic for topic in manifest["topics"]}
            unit = "spi-commands-unique" if interface == "spi" else "fepi-commands"
            for suffix in suffixes:
                identity = f"ibm-cics-ts-6x-2026-08-31:{unit}:{suffix}"
                self.assertNotIn(identity, seen)
                seen.add(identity)
                row = mapped[identity]
                self.assertEqual(row["state"], "mapped")
                topic = row["topic"]
                self.assertEqual(topic["sha256"], "sha256:" + pinned[topic["topic_path"]]["sha256"])
        self.assertEqual(len(seen), 118)

    def form_fixture(self):
        family = copy.deepcopy(self.family)
        grammar = family["commands"][0]["grammar"]
        shape = copy.deepcopy(grammar)
        grammar["forms"] = [{"id": "named", "selector_options": ["PROGRAM"],
                              "grammar": shape, "source_lines": [1]}]
        return family

    def test_absent_and_empty_forms_retain_the_prior_product_fact_preimage(self) -> None:
        family = copy.deepcopy(self.family)
        family["commands"][0]["grammar"]["forms"] = []
        self.assertEqual(self.project(), self.project(family))
        self.assertEqual(catalog_tool.render_grammar_facts(self.project()),
                         catalog_tool.render_grammar_facts(self.project(family)))

    def test_form_projection_preserves_direction_and_ignores_source_line_metadata(self) -> None:
        family = self.form_fixture()
        before = catalog_tool.render_grammar_facts(self.project(family))
        form = family["commands"][0]["grammar"]["forms"][0]
        form["source_lines"] = [2, 3]
        form["grammar"]["options"][0]["source_lines"] = [4]
        self.assertEqual(before, catalog_tool.render_grammar_facts(self.project(family)))
        form["grammar"]["options"][0]["direction"] = "output"
        after = catalog_tool.render_grammar_facts(self.project(family))
        self.assertNotEqual(before, after)
        self.assertIn(b"CicsAdministrativeGrammarForm", after)
        self.assertIn(b"selector_options: &[\"PROGRAM\"]", after)
        self.assertEqual(after.count(b"CicsApplicationConstraintStatus::Pending"), 5)
        self.assertNotIn(b"CicsResponse", after)

    def test_form_projection_rejects_unbound_selectors_duplicate_ids_and_nested_forms(self) -> None:
        for mutation in range(5):
            family = self.form_fixture()
            forms = family["commands"][0]["grammar"]["forms"]
            if mutation == 0:
                forms[0]["selector_options"] = ["MISSING"]
            elif mutation == 1:
                forms[0]["grammar"]["required"] = []
            elif mutation == 2:
                forms.append(copy.deepcopy(forms[0]))
            elif mutation == 3:
                forms[0]["grammar"]["forms"] = []
            else:
                forms[0]["grammar"]["options"][0]["name"] = "MISSING"
            with self.assertRaises(catalog_tool.CatalogError):
                self.project(family)

    def project(self, family=None):
        return catalog_tool.project_family_grammar(
            self.family if family is None else family, self.mapping, self.manifest
        )

    def test_exact_program_cohort_uses_shared_operand_types_and_pending_status(self) -> None:
        facts = self.project()
        self.assertEqual([fact["official_row"].rsplit(":", 1)[1] for fact in facts],
                         ["0026", "0084", "0155", "0241"])
        self.assertEqual(sum(len(fact["grammar"]["options"]) for fact in facts), 97)
        rendered = catalog_tool.render_grammar_facts(facts).decode()
        self.assertEqual(rendered.count("CicsApplicationConstraintStatus::Pending"), 4)
        self.assertIn("CicsApplicationOptionDescriptor", rendered)
        self.assertNotIn("runtime_operation", rendered)
        self.assertNotIn("handler_id", rendered)
        self.assertNotIn("CicsResponse", rendered)

    def test_cases_verdicts_and_lifecycle_prose_cannot_generate_product_behavior(self) -> None:
        original = catalog_tool.render_grammar_facts(self.project())
        changed = copy.deepcopy(self.family)
        command = changed["commands"][0]
        command["obligations"][0]["cases"][0]["expected"] = "invented pass"
        command["obligations"][0]["gates"] = []
        command["lifecycle"]["mutations"] = ["invented mutation"]
        command["responses"][0]["resp2"] = 999
        self.assertEqual(original, catalog_tool.render_grammar_facts(self.project(changed)))

    def test_stale_source_pin_label_topic_and_baseline_are_rejected(self) -> None:
        for field, value in (("sha256", "sha256:" + "0" * 64),
                             ("topic_path", "wrong.html"), ("baseline", "wrong-baseline")):
            with self.subTest(field=field):
                changed = copy.deepcopy(self.family)
                changed["commands"][0]["source"][field] = value
                with self.assertRaises((catalog_tool.CatalogError, KeyError)):
                    self.project(changed)
        changed = copy.deepcopy(self.family)
        changed["commands"][0]["label"] = "SET PROGRAM"
        with self.assertRaisesRegex(catalog_tool.CatalogError, "source identity"):
            self.project(changed)

    def test_missing_duplicate_and_reordered_rows_are_rejected(self) -> None:
        for mode in ("missing", "duplicate", "reordered"):
            with self.subTest(mode=mode):
                changed = copy.deepcopy(self.family)
                if mode == "missing":
                    changed["commands"].pop()
                elif mode == "duplicate":
                    changed["commands"][-1] = copy.deepcopy(changed["commands"][0])
                else:
                    changed["commands"].reverse()
                with self.assertRaisesRegex(catalog_tool.CatalogError, "row identity"):
                    self.project(changed)

    def test_public_binding_and_foreign_family_or_version_are_rejected(self) -> None:
        for field, value in (("runtime_binding", "public-registered"),
                             ("target_version", "0.9.0"), ("family", "spi-everything")):
            with self.subTest(field=field):
                changed = copy.deepcopy(self.family)
                changed[field] = value
                with self.assertRaises(catalog_tool.CatalogError):
                    self.project(changed)

    def test_product_fact_digest_is_sensitive_to_shape_direction_and_byte_bound(self) -> None:
        original = catalog_tool.render_grammar_facts(self.project())
        for field, value in (("value_shape", "optional-value"),
                             ("direction", "input-output"), ("source_max_value_bytes", 42)):
            changed = copy.deepcopy(self.family)
            changed["commands"][0]["grammar"]["options"][0][field] = value
            self.assertNotEqual(original, catalog_tool.render_grammar_facts(self.project(changed)))

    def test_emitted_grammar_is_current_and_deterministic(self) -> None:
        expected = catalog_tool.render_grammar()
        self.assertEqual(expected, catalog_tool.render_grammar())
        self.assertEqual(expected, (catalog_tool.ROOT / catalog_tool.GRAMMAR_OUTPUT_PATH).read_bytes())

    def test_optional_alternative_field_is_backward_compatible(self) -> None:
        original = catalog_tool.render_grammar_facts(self.project())
        explicit_empty = copy.deepcopy(self.family)
        for command in explicit_empty["commands"]:
            command["grammar"]["alternative_groups"] = []
        self.assertEqual(original, catalog_tool.render_grammar_facts(self.project(explicit_empty)))

    def test_alternatives_preserve_requirement_and_reject_nonboolean(self) -> None:
        changed = copy.deepcopy(self.family)
        changed["commands"][0]["grammar"]["alternative_groups"] = [
            {"members": ["LOG", "NOLOG"], "required": True}
        ]
        required = catalog_tool.render_grammar_facts(self.project(changed)).decode()
        self.assertIn('members: &["LOG", "NOLOG"], required: true', required)
        changed["commands"][0]["grammar"]["alternative_groups"][0]["required"] = False
        optional = catalog_tool.render_grammar_facts(self.project(changed)).decode()
        self.assertIn('members: &["LOG", "NOLOG"], required: false', optional)
        self.assertNotEqual(required, optional)
        changed["commands"][0]["grammar"]["alternative_groups"][0]["required"] = "true"
        with self.assertRaisesRegex(catalog_tool.CatalogError, "not boolean"):
            catalog_tool.render_grammar_facts(self.project(changed))


if __name__ == "__main__":
    unittest.main()
