import copy
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

    def test_repository_generated_descriptors_are_fresh_and_exhaustive(self):
        cics_descriptors.check(ROOT)
        catalog = cics_descriptors.load_catalog(ROOT)
        provider = cics_descriptors.render_provider(ROOT)
        host = cics_descriptors.render_host(ROOT)
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
