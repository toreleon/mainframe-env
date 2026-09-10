import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest


TOOL = Path(__file__).resolve().parents[1] / "generate_cics_descriptors.py"
ROOT = TOOL.parent.parent
SPEC = importlib.util.spec_from_file_location("generate_cics_descriptors", TOOL)
cics_descriptors = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cics_descriptors)


class CicsDescriptorTests(unittest.TestCase):
    def fixture(self, root: Path) -> None:
        for relative in [
            cics_descriptors.CATALOG_PATH,
            Path("conformance/0.2/catalogs/cics.json"),
        ]:
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)

    def catalog(self, root: Path) -> tuple[Path, dict]:
        path = root / cics_descriptors.CATALOG_PATH
        return path, json.loads(path.read_text())

    def write_catalog(self, path: Path, catalog: dict) -> None:
        path.write_text(json.dumps(catalog, indent=2) + "\n")

    def source_fixture(self, root: Path) -> tuple[Path, Path]:
        _, _, _, projection_relative, review_relative = cics_descriptors.CONTRACT_BATCHES[0]
        review = json.loads((ROOT / review_relative).read_text())
        relatives = [
            projection_relative,
            review_relative,
            *(
                Path(review["review_contract"][name]["path"])
                for name in ("schema", "checker", "independent_verifier")
            ),
        ]
        for relative in relatives:
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)

        projection_path = root / projection_relative
        review_path = root / review_relative
        review = json.loads(review_path.read_text())
        review["review_status"] = "blocked"
        review["counts"]["blocking_findings"] = max(
            1, review["counts"]["blocking_findings"]
        )
        review["inputs"]["candidate_projection_sha256"] = (
            "sha256:" + hashlib.sha256(projection_path.read_bytes()).hexdigest()
        )
        for name in ("schema", "checker", "independent_verifier"):
            binding = review["review_contract"][name]
            binding["file_sha256"] = (
                "sha256:" + hashlib.sha256((root / binding["path"]).read_bytes()).hexdigest()
            )
        review["review_sha256"] = cics_descriptors._source_review_digest(review)
        review_path.write_text(json.dumps(review, indent=2, sort_keys=True) + "\n")
        return projection_path, review_path

    def test_repository_generated_descriptors_are_fresh_and_exhaustive(self):
        cics_descriptors.check(ROOT)
        catalog = cics_descriptors.load_catalog(ROOT)
        provider = cics_descriptors.render_provider(ROOT)
        host = cics_descriptors.render_host(ROOT)
        contracts = cics_descriptors.build_contracts(ROOT)
        self.assertEqual(provider.count("CicsOperation::"), 50)
        self.assertIn("CicsCommandFamily::TaskControl", provider)
        self.assertIn("CicsCommandFamily::Recovery", provider)
        self.assertEqual(len(catalog["_application_commands"]), 263)
        self.assertEqual(len(catalog["_runtime_operations"]), 25)
        self.assertEqual(host.count("official_row:"), 263)
        self.assertIn(
            'CICS_APPLICATION_COMMAND_IDENTITY_SET_SHA256: &str =',
            host,
        )
        self.assertEqual([batch["command_count"] for batch in contracts["batches"]], [88, 88, 87])
        self.assertEqual(contracts["counts"]["commands"], 263)
        self.assertFalse(contracts["automatic_registration"])
        self.assertFalse(contracts["semantic_authority"])
        self.assertEqual(contracts["coverage_credit"], 0)
        self.assertEqual(contracts["semantic_credit"], 0)
        self.assertEqual(contracts["counts"]["implemented_commands"], 23)
        self.assertEqual(contracts["counts"]["registered_commands"], 23)
        contract_rows = [
            command for batch in contracts["batches"] for command in batch["commands"]
        ]
        self.assertEqual(
            [row["official_row"] for row in contract_rows],
            [row["official_row"] for row in catalog["_application_commands"]],
        )
        expected_runtime = {
            row["official_row"]: row["operation"]
            for row in catalog["_runtime_operations"]
            if row["interface"] == "api"
        }
        observed_runtime = {
            row["official_row"]: row["existing_runtime_operation"]
            for row in contract_rows
            if row["implementation_status"] == "existing-runtime"
        }
        self.assertEqual(observed_runtime, expected_runtime)
        self.assertEqual(len(observed_runtime), 23)
        self.assertEqual(
            sum(row["implementation_status"] == "unimplemented" for row in contract_rows),
            240,
        )
        self.assertTrue(
            all(
                row["registration_status"]
                == (
                    "existing-runtime"
                    if row["official_row"] in expected_runtime
                    else "unregistered"
                )
                for row in contract_rows
            )
        )
        self.assertEqual(contracts["contract_sha256"], cics_descriptors.contract_digest(contracts))

    def test_identity_digest_is_logical_stable_and_field_sensitive(self):
        catalog = cics_descriptors.load_catalog(ROOT)
        commands = catalog["_application_commands"]
        expected = "sha256:a18057164564781563252d53586a8afbc401f2783950db456b3b98177fc60b94"
        self.assertEqual(cics_descriptors.application_identity_digest(commands), expected)
        self.assertEqual(
            cics_descriptors.application_identity_digest(list(reversed(commands))),
            expected,
        )
        changed = copy.deepcopy(commands)
        changed[0]["label"] = "CHANGED"
        self.assertNotEqual(
            cics_descriptors.application_identity_digest(changed),
            expected,
        )

    def test_shared_eibfn_codes_are_valid_and_not_command_identities(self):
        commands = cics_descriptors.load_catalog(ROOT)["_application_commands"]
        rows_by_code: dict[str, list[str]] = {}
        for command in commands:
            rows_by_code.setdefault(command["eibfn"], []).append(command["label"])
        self.assertEqual(len(rows_by_code), 258)
        self.assertEqual(
            {code: labels for code, labels in rows_by_code.items() if len(labels) > 1},
            {
                "1008": ["START", "START ATTACH", "START BREXIT"],
                "3602": ["DEFINE COMPOSITE EVENT", "DEFINE INPUT EVENT"],
                "3810": ["EXTRACT WEB", "WEB EXTRACT"],
                "5602": ["SPOOLOPEN INPUT", "SPOOLOPEN OUTPUT"],
            },
        )

    def test_check_rejects_each_stale_generated_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            cics_descriptors.generate(root)
            cics_descriptors.check(root)
            for relative in [
                cics_descriptors.OUTPUT_PATH,
                cics_descriptors.HOST_OUTPUT_PATH,
                cics_descriptors.CONTRACT_OUTPUT_PATH,
            ]:
                with self.subTest(relative=relative):
                    output = root / relative
                    original = output.read_text()
                    output.write_text(original + "// stale\n")
                    with self.assertRaisesRegex(
                        cics_descriptors.DescriptorError, "is stale"
                    ):
                        cics_descriptors.check(root)
                    output.write_text(original)

    def test_contract_batches_carry_projected_facts_and_bound_missing_sources(self):
        contracts = cics_descriptors.build_contracts(ROOT)
        sources_a, sources_b, sources_c = contracts["batches"]
        self.assertIsNotNone(sources_a["source_input"]["projection"])
        self.assertGreater(contracts["counts"]["source_fact_groups"], 0)
        self.assertGreater(contracts["counts"]["source_candidate_references"], 0)
        self.assertTrue(
            any(
                dimension["facts"]
                for command in sources_a["commands"]
                for dimension in command["source_dimensions"]
            )
        )
        for batch in (sources_b, sources_c):
            self.assertIsNone(batch["source_input"]["projection"])
            self.assertIsNone(batch["source_input"]["review"])
            self.assertTrue(
                all(
                    command["source_status"] == "not-projected"
                    and command["unresolved_dimensions"]
                    == list(cics_descriptors.CONTRACT_DIMENSIONS)
                    for command in batch["commands"]
                )
            )

    def test_contract_source_mutation_makes_bound_review_stale(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            projection_path, _ = self.source_fixture(root)
            before = cics_descriptors.build_contracts(root)
            self.assertTrue(
                all(
                    command["source_status"] == "blocked-review"
                    for command in before["batches"][0]["commands"]
                )
            )

            projection = json.loads(projection_path.read_text())
            candidate = next(
                candidate
                for dimension in projection["rows"][0]["dimensions"]
                for candidate in dimension["candidates"]
                if candidate["kind"] != "source-context"
            )
            candidate["candidate_value"]["test_mutation"] = True
            projection_path.write_text(json.dumps(projection, indent=2) + "\n")

            after = cics_descriptors.build_contracts(root)
            self.assertTrue(
                all(
                    command["source_status"] == "stale-review"
                    for command in after["batches"][0]["commands"]
                )
            )
            self.assertNotEqual(before["contract_sha256"], after["contract_sha256"])

    def test_contract_rejects_incomplete_or_reordered_source_batch(self):
        for mutation in ("missing", "reordered"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                self.fixture(root)
                projection_path, _ = self.source_fixture(root)
                projection = json.loads(projection_path.read_text())
                if mutation == "missing":
                    projection["rows"].pop()
                else:
                    projection["rows"][0], projection["rows"][1] = (
                        projection["rows"][1],
                        projection["rows"][0],
                    )
                projection_path.write_text(json.dumps(projection, indent=2) + "\n")
                with self.assertRaises(cics_descriptors.DescriptorError):
                    cics_descriptors.build_contracts(root)

    def test_application_rows_reject_missing_duplicate_reordered_and_foreign_rows(self):
        mutators = {
            "missing": lambda rows: rows.pop(),
            "duplicate": lambda rows: rows.__setitem__(1, copy.deepcopy(rows[0])),
            "reordered": lambda rows: rows.reverse(),
            "foreign": lambda rows: rows.__setitem__(
                0,
                {
                    "official_row": (
                        "ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0001"
                    ),
                    "label": "CREATE ATOMSERVICE",
                    "eibfn": "8602",
                },
            ),
        }
        for name, mutate in mutators.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                self.fixture(root)
                path, catalog = self.catalog(root)
                mutate(catalog["application_catalog"]["commands"])
                self.write_catalog(path, catalog)
                with self.assertRaises(cics_descriptors.DescriptorError):
                    cics_descriptors.load_catalog(root)

    def test_application_rows_reject_malformed_eibfn_or_label(self):
        for field, value in [("eibfn", "0e0c"), ("eibfn", "000"), ("label", "")]:
            with self.subTest(field=field, value=value), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                self.fixture(root)
                path, catalog = self.catalog(root)
                catalog["application_catalog"]["commands"][0][field] = value
                self.write_catalog(path, catalog)
                with self.assertRaises(cics_descriptors.DescriptorError):
                    cics_descriptors.load_catalog(root)

    def test_application_catalog_cannot_register_or_claim_coverage(self):
        for field, value in [
            ("automatic_registration", True),
            ("generated_coverage_credit", 1),
            ("command_count", 262),
        ]:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                self.fixture(root)
                path, catalog = self.catalog(root)
                catalog["application_catalog"][field] = value
                self.write_catalog(path, catalog)
                with self.assertRaisesRegex(
                    cics_descriptors.DescriptorError, "unregistered"
                ):
                    cics_descriptors.load_catalog(root)

    def test_runtime_operations_remain_exactly_23_api_and_2_spi(self):
        catalog = cics_descriptors.load_catalog(ROOT)
        counts = {"api": 0, "spi-compatibility": 0}
        for operation in catalog["_runtime_operations"]:
            counts[operation["interface"]] += 1
        self.assertEqual(counts, {"api": 23, "spi-compatibility": 2})

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            path, changed = self.catalog(root)
            changed["runtime"]["operations"][0]["interface"] = "spi-compatibility"
            self.write_catalog(path, changed)
            with self.assertRaises(cics_descriptors.DescriptorError):
                cics_descriptors.load_catalog(root)

    def test_runtime_rejects_fepi_leakage_and_unknown_family(self):
        cases = [("official_row", "ibm-cics-ts-6x-2026-08-31:fepi-commands:0001")]
        cases.append(("family", "unowned"))
        for field, value in cases:
            with self.subTest(field=field), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                self.fixture(root)
                path, catalog = self.catalog(root)
                catalog["runtime"]["operations"][0][field] = value
                self.write_catalog(path, catalog)
                with self.assertRaises(cics_descriptors.DescriptorError):
                    cics_descriptors.load_catalog(root)

    def test_official_catalog_digest_and_reviewed_locator_anomalies_are_frozen(self):
        catalog = cics_descriptors.load_catalog(ROOT)
        self.assertEqual(
            catalog["official_catalog_sha256"],
            cics_descriptors.OFFICIAL_CATALOG_DIGEST,
        )

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            official_path = root / "conformance/0.2/catalogs/cics.json"
            official = json.loads(official_path.read_text())
            api = next(unit for unit in official["units"] if unit["id"] == "api-commands")
            api["rows"][192]["source_locator"] = (
                "html-table:dfha8mf__eibfn_table_cmds_api;"
                "eibfn:C404;family:API"
            )
            official_path.write_text(json.dumps(official, indent=2) + "\n")
            with self.assertRaisesRegex(
                cics_descriptors.DescriptorError, "digest drifted"
            ):
                cics_descriptors.load_catalog(root)


if __name__ == "__main__":
    unittest.main()
