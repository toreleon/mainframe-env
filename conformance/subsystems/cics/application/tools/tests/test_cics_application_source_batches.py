from __future__ import annotations

import importlib.util
import json
from collections import Counter
from pathlib import Path
import sys
import unittest


TOOL = Path(__file__).resolve().parents[1] / "cics_application_source_batches.py"
spec = importlib.util.spec_from_file_location(
    "cics_application_source_batches_test", TOOL
)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)
ROOT = Path(__file__).resolve().parents[6]


class CicsApplicationSourceBatchTests(unittest.TestCase):
    @staticmethod
    def projections() -> list[dict]:
        return [
            json.loads((ROOT / batch.projection_path).read_text(encoding="utf-8"))
            for batch in module.BATCHES.values()
        ]

    def test_three_batches_close_the_263_row_catalog_without_overlap(self) -> None:
        ordinals = [
            ordinal
            for batch in module.BATCHES.values()
            for ordinal in range(batch.ordinal_start, batch.ordinal_end + 1)
        ]
        self.assertEqual(ordinals, list(range(1, 264)))
        self.assertEqual(
            [batch.row_count for batch in module.BATCHES.values()], [88, 88, 87]
        )

    def test_appc_state_context_is_bounded_to_its_batch_b_rows(self) -> None:
        projection = json.loads(
            (ROOT / module.BATCHES["b"].projection_path).read_text(encoding="utf-8")
        )
        basic = {
            "SSJL4D_6.x/reference-applications/commands-api/dfhp4_gdssend.html",
            "SSJL4D_6.x/applications/developing/connections/dfhp625.html",
        }
        mapped = {
            "SSJL4D_6.x/applications/developing/connections/dfhp616.html",
            *(f"SSJL4D_6.x/applications/developing/connections/appcmapped_sl{level}.html"
              for level in range(3)),
        }
        basic_tables = {
            f"SSJL4D_6.x/applications/developing/connections/appcbasic_sl{level}.html"
            for level in range(3)
        }
        all_paths = basic | basic_tables | mapped
        for row in projection["rows"]:
            context = next(
                dimension for dimension in row["dimensions"]
                if dimension["name"] == "execution-context"
            )
            paths = {
                candidate["evidence"]["topic_path"]
                for candidate in context["candidates"]
            } & all_paths
            ordinal = row["official_row"].rsplit(":", 1)[-1]
            expected = set()
            if ordinal in {"0109", "0113", "0123", "0128", "0135"}:
                expected = basic_tables | {next(path for path in basic if path.endswith("dfhp625.html"))}
            if ordinal in {"0113", "0123"}:
                expected |= {next(path for path in basic if path.endswith("dfhp4_gdssend.html"))}
            if ordinal in {"0108", "0112", "0122", "0127", "0136"}:
                expected = mapped
            self.assertEqual(
                paths,
                expected,
                row["official_row"],
            )

    def test_paths_and_work_packages_are_derived_from_batch_identity(self) -> None:
        for name, batch in module.BATCHES.items():
            with self.subTest(batch=name):
                self.assertEqual(
                    batch.projection_path.as_posix(),
                    "conformance/subsystems/cics/application/generated/"
                    f"cics-application-api-sources-{name}-candidates.json",
                )
                self.assertEqual(
                    batch.project_work_package, f"CIC-901.sources-{name}-project"
                )
                self.assertEqual(
                    batch.review_work_package, f"CIC-901.sources-{name}-review"
                )

    def test_row_lookup_selects_exact_batch_boundaries(self) -> None:
        expected = {
            1: "a",
            88: "a",
            89: "b",
            176: "b",
            177: "c",
            263: "c",
        }
        for ordinal, name in expected.items():
            row = f"ibm-cics-ts-6x-2026-08-31:api-commands:{ordinal:04d}"
            self.assertEqual(module.source_batch_for_row(row).name, name)
        with self.assertRaises(ValueError):
            module.source_batch_for_row("ibm-cics-ts-6x-2026-08-31:api-commands:0264")

    def test_applicability_prose_and_language_inventory_are_closed(self) -> None:
        applicability = []
        for projection in self.projections():
            for row in projection["rows"]:
                merged = {}
                for dimension in row["dimensions"]:
                    for candidate in dimension["candidates"]:
                        merged.update(
                            candidate.get("candidate_value", {}).get(
                                "applicability", {}
                            )
                        )
                applicability.append((row["label"], merged))
        dpl = [value for _, value in applicability if value["dpl_server"] == "restricted"]
        self.assertEqual(len(dpl), 56)
        # Fifty-five rows come from the DPL section; GDS EXTRACT ATTRIBUTES is
        # separately bounded by its command-page RETCODE table.
        structured = [value["dpl_restriction"] for value in dpl if "dpl_restriction" in value]
        self.assertEqual(len(structured), 55)
        self.assertEqual(Counter(value["resp2"] for value in structured), {200: 40, 1: 12, 5: 3})
        language = Counter(
            value["language_restriction"]["profile"] for _, value in applicability
        )
        self.assertEqual(
            language,
            {
                "exec-cics-all-supported-languages": 240,
                "assembler-and-c-only": 13,
                "non-le-amode64-assembler-only": 4,
                "cobol-pli-non-amode64-assembler-only": 5,
                "internal-only": 1,
            },
        )
        self.assertEqual(
            Counter(value["cobol"] for _, value in applicability),
            {"allowed": 245, "not-applicable": 18},
        )

    def test_threadsafe_predicates_and_syntax_panel_roles_are_complete(self) -> None:
        conditional = []
        continuations = []
        for projection in self.projections():
            for row in projection["rows"]:
                for dimension in row["dimensions"]:
                    for candidate in dimension["candidates"]:
                        value = candidate["candidate_value"]
                        if value.get("applicability", {}).get("threadsafe") == "conditional":
                            conditional.append((row["label"], value["applicability"]))
                        if (
                            candidate["kind"] == "source-syntax"
                            and value["panel"]["role"] == "continuation"
                        ):
                            continuations.append((row["label"], value["syntax_head"]))
        self.assertEqual(len(conditional), 20)
        self.assertTrue(
            all("threadsafe_condition" in value for _, value in conditional)
        )
        self.assertEqual(len(continuations), 16)
        self.assertEqual(
            {label for label, _ in continuations},
            {
                "RECEIVE PARTN",
                "SEND CONTROL",
                "SEND MAP",
                "SEND TEXT",
                "WEB CONVERSE",
                "WEB OPEN",
                "WEB SEND",
            },
        )

    def test_condition_trigger_hashes_and_readq_dependency_are_preserved(self) -> None:
        readq = None
        condition_count = 0
        for projection in self.projections():
            for row in projection["rows"]:
                if row["label"] == "READQ TS":
                    readq = row
                for dimension in row["dimensions"]:
                    for candidate in dimension["candidates"]:
                        if candidate["kind"] != "source-condition":
                            continue
                        value = candidate["candidate_value"]
                        self.assertEqual(
                            len(value["trigger_fragment_sha256s"]),
                            value["definition_count"],
                        )
                        condition_count += 1
        self.assertGreater(condition_count, 0)
        self.assertIsNotNone(readq)
        options = next(
            dimension for dimension in readq["dimensions"] if dimension["name"] == "options"
        )
        length = next(
            candidate for candidate in options["candidates"] if candidate["key"] == "LENGTH"
        )
        self.assertEqual(length["candidate_value"]["option_legality"], "bounded-prose")
        self.assertIn(
            "prose-option-legality-not-structured",
            {issue["code"] for issue in options["issues"]},
        )

    def test_catalog_discriminators_alias_and_web_context_stay_explicit(self) -> None:
        rows = {
            row["label"]: row
            for projection in self.projections()
            for row in projection["rows"]
        }
        expected = {
            "ACQUIRE ACTIVITYID": ("ACQUIRE", "catalog-qualified", "ACTIVITYID"),
            "ASKTIME ABSTIME": ("ASKTIME", "catalog-qualified", "ABSTIME"),
            "SEND MAP": ("SEND", "catalog-qualified", "MAP"),
            "WRITE FILE": ("WRITE", "catalog-qualified", "FILE"),
            "WAIT": ("GDS WAIT", "documented-alias", None),
        }
        for label, (head, relation, discriminator) in expected.items():
            values = [
                candidate["candidate_value"]
                for dimension in rows[label]["dimensions"]
                for candidate in dimension["candidates"]
                if candidate["kind"] == "source-syntax"
                and candidate["candidate_value"]["panel"]["role"] == "command-head"
            ]
            self.assertTrue(values)
            self.assertTrue(
                any(
                    value["syntax_head"] == head
                    and value["identity_relation"] == relation
                    and (
                        discriminator is None
                        or {"name": discriminator, "state": "present"}
                        in value["identity_discriminators"]
                    )
                    for value in values
                )
            )
        review = json.loads(
            (ROOT / module.BATCHES["c"].review_path).read_text(encoding="utf-8")
        )
        scope = {
            item["official_row"]: set(item["dimensions"])
            for item in review["ambiguity_scope"]
        }
        self.assertIn(
            "execution-context",
            scope["ibm-cics-ts-6x-2026-08-31:api-commands:0246"],
        )

    def test_link_activity_shared_page_does_not_leak_acqactivity_branch(self) -> None:
        rows = {
            row["label"]: row
            for projection in self.projections()
            for row in projection["rows"]
        }
        row = rows["LINK ACTIVITY"]
        syntax_keywords = {
            token["value"].rstrip("(")
            for dimension in row["dimensions"]
            for candidate in dimension["candidates"]
            if candidate["kind"] == "source-syntax"
            for token in candidate["candidate_value"]["tokens"]
            if token["kind"] == "keyword"
        }
        option_and_direction_keys = {
            candidate["key"]
            for dimension in row["dimensions"]
            for candidate in dimension["candidates"]
            if candidate["kind"] in {"source-option", "source-operand-direction"}
        }
        self.assertIn("ACTIVITY", syntax_keywords)
        self.assertNotIn("ACQACTIVITY", syntax_keywords)
        self.assertIn("ACTIVITY", option_and_direction_keys)
        self.assertNotIn("ACQACTIVITY", option_and_direction_keys)

    def test_dimension_issue_bound_covers_measured_source_closure(self) -> None:
        schema = json.loads(
            (
                ROOT
                / "conformance/subsystems/cics/application/schemas/cics-source-candidates.schema.json"
            ).read_text(encoding="utf-8")
        )
        self.assertEqual(schema["$defs"]["dimension"]["properties"]["issues"]["maxItems"], 128)
        measured = []
        for projection in self.projections():
            for row in projection["rows"]:
                for dimension in row["dimensions"]:
                    measured.append((len(dimension["issues"]), row["label"], dimension["name"]))
        self.assertEqual(max(measured), (97, "SEND", "options"))

    def test_shared_condition_applicability_is_fail_closed(self) -> None:
        issues = [
            issue
            for projection in self.projections()
            for issue in projection["blocking_issues"]
            if issue["code"] == "shared-condition-applicability-unresolved"
        ]
        self.assertEqual(len(issues), 88)
        self.assertEqual(len({issue["official_row"] for issue in issues}), 18)
        self.assertEqual(
            len(
                {
                    (
                        issue["evidence"][0]["topic_path"],
                        issue["evidence"][0]["structural_path"],
                    )
                    for issue in issues
                }
            ),
            50,
        )

    def test_retained_shared_catalog_discriminators_are_required(self) -> None:
        rows = {
            row["official_row"].rsplit(":", 1)[-1]: row
            for projection in self.projections()
            for row in projection["rows"]
        }
        expected = {
            "0017": "ACQACTIVITY",
            "0018": "ACQPROCESS",
            "0023": "ACQACTIVITY",
            "0139": "ACQACTIVITY",
            "0173": "ACQPROCESS",
            "0174": "ACTIVITY",
            "0183": "ACQACTIVITY",
            "0184": "ACQPROCESS",
            "0216": "ACQPROCESS",
            "0217": "ACTIVITY",
        }
        for number, discriminator in expected.items():
            syntax = next(
                candidate["candidate_value"]
                for dimension in rows[number]["dimensions"]
                for candidate in dimension["candidates"]
                if candidate["kind"] == "source-syntax"
                and candidate["candidate_value"]["panel"]["role"] == "command-head"
            )
            relations = {
                token["relation"]
                for token in syntax["tokens"]
                if token["kind"] == "keyword"
                and token["value"].rstrip("(") == discriminator
            }
            self.assertEqual(relations, {"required"})
            # A valued discriminator is excluded from syntax_head and therefore
            # must be carried explicitly. Bare keywords already form part of
            # the source-derived head, so duplicating them is unnecessary.
            if discriminator not in syntax["syntax_head"].split():
                self.assertIn(
                    {"name": discriminator, "state": "present"},
                    syntax["identity_discriminators"],
                )


if __name__ == "__main__":
    unittest.main()
