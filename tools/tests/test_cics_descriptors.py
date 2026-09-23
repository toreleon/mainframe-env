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
    def test_spoolwrite_page_choice_uses_verified_syntax(self):
        contracts = cics_descriptors.build_contracts(ROOT)
        row = next(
            command
            for batch in contracts["batches"]
            for command in batch["commands"]
            if command["label"] == "SPOOLWRITE"
        )
        options = row["contract"]["options"]
        self.assertIn("PAGE", [entry["name"] for entry in options["entries"]])
        self.assertIn(
            {"members": ["LINE", "PAGE"], "required": False},
            options["constraints"]["alternatives"],
        )
        self.assertIn(
            ["LINE", "PAGE"], options["constraints"]["mutual_exclusions"]
        )

    def fixture(self, root: Path) -> None:
        for relative in [
            cics_descriptors.CATALOG_PATH,
            cics_descriptors.LEGACY_EXECUTION_CATALOG_PATH,
            cics_descriptors.TYPED_EXECUTION_REGISTRATIONS_PATH,
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

    def legacy_execution_catalog(self, root: Path) -> tuple[Path, dict]:
        path = root / cics_descriptors.LEGACY_EXECUTION_CATALOG_PATH
        return path, json.loads(path.read_text())

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

    def frozen_source_fixture(self, root: Path) -> None:
        template = json.loads(
            (ROOT / cics_descriptors.CONTRACT_BATCHES[0][4]).read_text()
        )
        contract_paths = [
            Path(template["review_contract"][name]["path"])
            for name in ("schema", "checker", "independent_verifier")
        ]
        for relative in contract_paths:
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)

        for batch_id, _, _, projection_relative, review_relative in (
            cics_descriptors.CONTRACT_BATCHES
        ):
            projection_target = root / projection_relative
            projection_target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / projection_relative, projection_target)
            review = copy.deepcopy(template)
            review["work_package"] = f"CIC-901.{batch_id}-review"
            review["review_status"] = "auto-accepted"
            review["inputs"]["candidate_projection_sha256"] = (
                "sha256:" + hashlib.sha256(projection_target.read_bytes()).hexdigest()
            )
            review["counts"]["blocking_findings"] = 0
            review["candidate_dispositions"]["product-ambiguity"]["count"] = 0
            review["issue_dispositions"]["product-ambiguity"]["count"] = 0
            review["ambiguity_scope"] = []
            review["blocker_ids"] = []
            for name in ("schema", "checker", "independent_verifier"):
                binding = review["review_contract"][name]
                binding["file_sha256"] = (
                    "sha256:"
                    + hashlib.sha256((root / binding["path"]).read_bytes()).hexdigest()
                )
            review["review_sha256"] = cics_descriptors._source_review_digest(review)
            review_target = root / review_relative
            review_target.parent.mkdir(parents=True, exist_ok=True)
            review_target.write_text(json.dumps(review, indent=2, sort_keys=True) + "\n")

    def test_repository_generated_descriptors_are_fresh_and_exhaustive(self):
        cics_descriptors.check(ROOT)
        catalog = cics_descriptors.load_catalog(ROOT)
        provider = cics_descriptors.render_provider(ROOT)
        host = cics_descriptors.render_host(ROOT)
        compiler_spi = cics_descriptors.render_compiler_spi_compatibility(ROOT)
        contracts = cics_descriptors.build_contracts(ROOT)
        ir_registry = cics_descriptors.render_ir_registry(ROOT, contracts)
        self.assertEqual(provider.count("CicsOperation::"), 190)
        self.assertIn("pub(crate) const CICS_CONDITION_NAMES", provider)
        self.assertIn('"PGMIDERR"', provider)
        self.assertIn("CicsCommandFamily::TaskControl", provider)
        self.assertIn("CicsCommandFamily::Recovery", provider)
        self.assertIn("CicsCommandFamily::JournalControl", provider)
        self.assertEqual(len(catalog["_application_commands"]), 263)
        self.assertEqual(len(catalog["_runtime_operations"]), 95)
        self.assertEqual(host.count("official_row:"), 263)
        self.assertIn(
            'official_row: "ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0155"',
            compiler_spi,
        )
        self.assertIn('runtime_operation: "Inquire"', compiler_spi)
        self.assertNotIn("SetFileStatus", compiler_spi)
        self.assertNotIn("SET FILE", compiler_spi)
        self.assertIn(
            'CICS_APPLICATION_COMMAND_IDENTITY_SET_SHA256: &str =',
            host,
        )
        self.assertEqual([batch["command_count"] for batch in contracts["batches"]], [88, 88, 87])
        self.assertEqual(contracts["counts"]["commands"], 263)
        self.assertFalse(contracts["automatic_registration"])
        self.assertEqual(
            contracts["semantic_authority"], contracts["status"] != "source-pending"
        )
        self.assertFalse(contracts["execution_authority"])
        self.assertEqual(contracts["coverage_credit"], 0)
        self.assertEqual(contracts["semantic_credit"], 0)
        self.assertEqual(contracts["counts"]["runtime_backed_commands"], 93)
        self.assertEqual(contracts["counts"]["typed_runtime_commands"], 93)
        self.assertEqual(contracts["counts"]["legacy_compatibility_commands"], 0)
        self.assertEqual(contracts["counts"]["advertised_commands"], 93)
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
            if row["implementation_status"] != "unimplemented"
        }
        self.assertEqual(observed_runtime, expected_runtime)
        self.assertEqual(len(observed_runtime), 93)
        self.assertEqual(
            sum(row["implementation_status"] == "unimplemented" for row in contract_rows),
            170,
        )
        rows_by_label = {row["label"]: row for row in contract_rows}
        self.assertEqual(rows_by_label["ABEND"]["registration_status"], "typed-runtime")
        self.assertEqual(rows_by_label["ABEND"]["existing_runtime_operation"], "Abend")
        self.assertEqual(
            rows_by_label["HANDLE ABEND"]["registration_status"], "typed-runtime"
        )
        self.assertEqual(
            rows_by_label["HANDLE ABEND"]["existing_runtime_operation"],
            "HandleAbend",
        )
        self.assertEqual(rows_by_label["LINK"]["registration_status"], "typed-runtime")
        self.assertEqual(rows_by_label["LINK"]["existing_runtime_operation"], "Link")
        self.assertEqual(rows_by_label["XCTL"]["registration_status"], "typed-runtime")
        self.assertEqual(rows_by_label["XCTL"]["existing_runtime_operation"], "Xctl")
        self.assertEqual(rows_by_label["RETURN"]["registration_status"], "typed-runtime")
        self.assertEqual(rows_by_label["RETURN"]["existing_runtime_operation"], "Return")
        for label, operation in [
            ("STARTBR", "StartBrowse"),
            ("READNEXT", "ReadNext"),
            ("READPREV", "ReadPrev"),
            ("ENDBR", "EndBrowse"),
            ("DELETE", "Delete"),
            ("DELAY", "Delay"),
            ("WRITE FILE", "Write"),
            ("WRITEQ TD", "WriteTransientData"),
            ("WRITEQ TS", "WriteTemporaryStorage"),
            ("RECEIVE MAP", "ReceiveMap"),
            ("SEND MAP", "SendMap"),
            ("SEND TEXT", "SendText"),
            ("ASSIGN", "Assign"),
            ("CANCEL", "Cancel"),
            ("PURGE MESSAGE", "PurgeMessage"),
            ("START", "Start"),
            ("RETRIEVE", "Retrieve"),
            ("WAIT EVENT", "WaitEvent"),
            ("WAIT EXTERNAL", "WaitExternal"),
            ("SPOOLCLOSE", "SpoolClose"),
            ("SPOOLOPEN INPUT", "SpoolOpenInput"),
            ("SPOOLOPEN OUTPUT", "SpoolOpenOutput"),
            ("SPOOLREAD", "SpoolRead"),
            ("SPOOLWRITE", "SpoolWrite"),
        ]:
            self.assertEqual(rows_by_label[label]["registration_status"], "typed-runtime")
            self.assertEqual(rows_by_label[label]["existing_runtime_operation"], operation)
        self.assertEqual(
            rows_by_label["ASKTIME"]["registration_status"],
            "typed-runtime",
        )
        self.assertEqual(
            rows_by_label["ASKTIME"]["existing_runtime_operation"],
            "AsktimeEib",
        )
        self.assertEqual(rows_by_label["ASKTIME"]["contract"]["registry"]["family"], "time")
        self.assertEqual(
            rows_by_label["ASKTIME ABSTIME"]["registration_status"],
            "typed-runtime",
        )
        self.assertEqual(
            rows_by_label["ASKTIME ABSTIME"]["existing_runtime_operation"],
            "Asktime",
        )
        self.assertEqual(rows_by_label["FORMATTIME"]["registration_status"], "typed-runtime")
        self.assertEqual(
            rows_by_label["FORMATTIME"]["existing_runtime_operation"],
            "FormatTime",
        )
        self.assertEqual(ir_registry.count("CicsApplicationRegistryDescriptor {"), 263)
        self.assertIn("CICS_APPLICATION_REGISTRY_FROZEN", ir_registry)
        self.assertIn("CICS_APPLICATION_REGISTRY_SHA256", ir_registry)
        self.assertEqual(contracts["registry"]["shape_commands"], 263)
        self.assertEqual(contracts["registry"]["typed_handlers"], 93)
        self.assertEqual(contracts["registry"]["legacy_compatibility_handlers"], 0)
        self.assertEqual(contracts["registry"]["advertised_commands"], 93)
        self.assertEqual(contracts["registry"]["unready_handlers"], 170)
        self.assertIsNone(contracts["registry"]["default_handler"])
        self.assertEqual(contracts["participant_contract"]["status"], "bounded-ambiguity")
        self.assertEqual(contracts["participant_contract"]["execution_credit"], 0)
        self.assertTrue(
            all(
                row["registration_status"]
                == (
                    "typed-runtime"
                    if expected_runtime.get(row["official_row"])
                    in cics_descriptors.TYPED_RUNTIME_OPERATIONS
                    else "legacy-compatibility"
                    if row["official_row"] in expected_runtime
                    else "unready"
                )
                for row in contract_rows
            )
        )
        self.assertEqual(
            sum(row["contract"]["registry"]["advertised"] for row in contract_rows),
            93,
        )
        self.assertTrue(
            all(
                row["row_contract_sha256"] == cics_descriptors._row_contract_digest(row)
                for row in contract_rows
            )
        )
        self.assertTrue(
            all(
                row["contract"]["registry"]["handler_sha256"]
                == cics_descriptors._handler_digest(
                    {
                        key: value
                        for key, value in row["contract"]["registry"].items()
                        if key != "handler_sha256"
                    }
                )
                for row in contract_rows
            )
        )
        registry_material = [
            cics_descriptors._registry_row_material(
                row, row["source_dimensions"], row["contract"]
            )
            for row in contract_rows
        ]
        self.assertEqual(
            contracts["registry"]["registry_sha256"],
            cics_descriptors._registry_digest(registry_material),
        )
        self.assertTrue(
            all(
                row["contract"]["registry"]["unready_result"]
                == "explicit-unsupported"
                for row in contract_rows
                if row["registration_status"] == "unready"
            )
        )
        if contracts["status"] != "source-pending":
            self.assertEqual(contracts["counts"]["source_reviewed_commands"], 263)
            self.assertEqual(contracts["counts"]["closed_contract_commands"], 263)
            self.assertTrue(all(not row["unresolved_dimensions"] for row in contract_rows))
        self.assertEqual(contracts["contract_sha256"], cics_descriptors.contract_digest(contracts))

    def test_compiler_legacy_compatibility_includes_bare_send_bound_to_send_text(self):
        # toreleon/mainframe-env#177: row 0187 (bare `SEND FROM(...)`) is a
        # second generated compiler-only compatibility descriptor, bound to
        # the pre-existing `SendText` runtime operation and its own reviewed
        # runtime row (`api-commands:0192`, `SEND TEXT`). Row 0187 itself
        # stays `Unready` in the 263-row registry; this route never touches it.
        compiler_legacy = cics_descriptors.render_compiler_spi_compatibility(ROOT)
        self.assertIn(
            'official_row: "ibm-cics-ts-6x-2026-08-31:api-commands:0187"',
            compiler_legacy,
        )
        self.assertIn('runtime_operation: "SendText"', compiler_legacy)
        self.assertIn(
            'runtime_official_row: "ibm-cics-ts-6x-2026-08-31:api-commands:0192"',
            compiler_legacy,
        )
        self.assertIn('label_tokens: &["SEND"]', compiler_legacy)
        self.assertIn('required_value_options: &["FROM"]', compiler_legacy)
        self.assertIn('optional_value_options: &["LENGTH", "RESP", "RESP2"]', compiler_legacy)
        self.assertIn('optional_flag_options: &["ERASE", "NOHANDLE"]', compiler_legacy)
        self.assertIn(
            'application_discriminator_options: '
            '&["CONTROL", "MAP", "PAGE", "PARTNSET", "TEXT"]',
            compiler_legacy,
        )
        self.assertIn("reject_unknown_options: false", compiler_legacy)
        # The pre-existing INQUIRE PROGRAM compatibility entry is untouched.
        self.assertIn(
            'official_row: "ibm-cics-ts-6x-2026-08-31:spi-commands-unique:0155"',
            compiler_legacy,
        )
        self.assertIn('runtime_operation: "Inquire"', compiler_legacy)
        self.assertIn("reject_unknown_options: true", compiler_legacy)
        self.assertNotIn("SetFileStatus", compiler_legacy)
        self.assertNotIn("SET FILE", compiler_legacy)
        # Row 0187 itself remains Unready and unadvertised; this route does
        # not touch the 263-row application registry.
        ir_registry = cics_descriptors.render_ir_registry(ROOT)
        row_start = ir_registry.find('official_row: "ibm-cics-ts-6x-2026-08-31:api-commands:0187"')
        self.assertNotEqual(row_start, -1)
        row_end = ir_registry.find("CicsApplicationRegistryDescriptor {", row_start)
        row_text = ir_registry[row_start:row_end]
        self.assertIn("readiness: CicsApplicationHandlerReadiness::Unready", row_text)
        self.assertIn("advertised: false", row_text)

    def test_compiler_legacy_compatibility_cross_check_rejects_unbound_runtime_operation(self):
        catalog = cics_descriptors.load_catalog(ROOT)
        bogus = dict(cics_descriptors.COMPILER_SEND_COMPATIBILITY)
        bogus["operation"] = "NotAReviewedRuntimeOperation"
        with self.assertRaisesRegex(cics_descriptors.DescriptorError, "is not unique"):
            cics_descriptors._compiler_legacy_compatibility_runtime_operation(catalog, bogus)

        mismatched_row = dict(cics_descriptors.COMPILER_SEND_COMPATIBILITY)
        mismatched_row["runtime_official_row"] = (
            "ibm-cics-ts-6x-2026-08-31:api-commands:0189"
        )
        with self.assertRaisesRegex(
            cics_descriptors.DescriptorError, "differs from the reviewed runtime table"
        ):
            cics_descriptors._compiler_legacy_compatibility_runtime_operation(
                catalog, mismatched_row
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
                cics_descriptors.COMPILER_SPI_COMPAT_OUTPUT_PATH,
                cics_descriptors.IR_REGISTRY_OUTPUT_PATH,
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
            if batch["source_input"]["projection"] is None:
                self.assertIsNone(batch["source_input"]["review"])
                self.assertTrue(
                    all(
                        command["source_status"] == "not-projected"
                        and command["unresolved_dimensions"]
                        == list(cics_descriptors.CONTRACT_DIMENSIONS)
                        for command in batch["commands"]
                    )
                )
            else:
                self.assertEqual(len(batch["commands"]), batch["command_count"])

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

    def test_contract_freezes_only_after_all_three_source_reviews_are_current(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            pending = cics_descriptors.build_contracts(root)
            self.assertEqual(pending["status"], "source-pending")
            self.assertFalse(pending["semantic_authority"])

            self.frozen_source_fixture(root)
            frozen = cics_descriptors.build_contracts(root)
            self.assertEqual(frozen["status"], "frozen-with-bounded-ambiguities")
            self.assertTrue(frozen["semantic_authority"])
            self.assertFalse(frozen["execution_authority"])
            self.assertEqual(frozen["counts"]["source_reviewed_commands"], 263)
            self.assertEqual(frozen["counts"]["closed_contract_commands"], 263)
            rows = [row for batch in frozen["batches"] for row in batch["commands"]]
            self.assertTrue(all(not row["unresolved_dimensions"] for row in rows))
            self.assertTrue(
                all(
                    disposition["status"] != "pending"
                    for row in rows
                    for disposition in row["dimension_dispositions"]
                )
            )
            self.assertIn(
                "CICS_APPLICATION_REGISTRY_FROZEN: bool = true",
                cics_descriptors.render_ir_registry(root, frozen),
            )

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

    def test_runtime_operations_match_registered_api_and_spi(self):
        catalog = cics_descriptors.load_catalog(ROOT)
        counts = {"api": 0, "spi-compatibility": 0}
        for operation in catalog["_runtime_operations"]:
            counts[operation["interface"]] += 1
        self.assertEqual(counts, {"api": 93, "spi-compatibility": 2})

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            path, changed = self.catalog(root)
            changed["runtime"]["operations"][0]["interface"] = "spi-compatibility"
            self.write_catalog(path, changed)
            with self.assertRaises(cics_descriptors.DescriptorError):
                cics_descriptors.load_catalog(root)

    def test_legacy_execution_options_are_catalog_owned_and_source_reviewed(self):
        catalog = cics_descriptors.load_catalog(ROOT)
        legacy = [
            operation
            for operation in catalog["_runtime_operations"]
            if operation["legacy_execution_options"]
        ]
        self.assertEqual(legacy, [])
        self.assertTrue(
            all(
                operation["interface"] == "api"
                and operation["operation"]
                not in cics_descriptors.TYPED_RUNTIME_OPERATIONS
                and operation["legacy_execution_options"]
                == sorted(set(operation["legacy_execution_options"]))
                for operation in legacy
            )
        )

        contracts = cics_descriptors.build_contracts(ROOT)
        registry = {
            command["official_row"]: command["contract"]["registry"]
            for batch in contracts["batches"]
            for command in batch["commands"]
        }
        for operation in legacy:
            self.assertEqual(
                registry[operation["official_row"]]["legacy_execution_options"],
                operation["legacy_execution_options"],
            )

    def test_enqueue_lifetime_forms_are_normalized_from_the_verified_syntax(self):
        contracts = cics_descriptors.build_contracts(ROOT)
        rows = {
            command["label"]: command
            for batch in contracts["batches"]
            for command in batch["commands"]
        }
        for label in ("DEQ", "ENQ"):
            options = rows[label]["contract"]["options"]
            entries = {entry["name"]: entry for entry in options["entries"]}
            self.assertEqual(entries["MAXLIFETIME"]["value_shape"], "value")
            self.assertEqual(entries["MAXLIFETIME"]["directions"], ["input"])
            self.assertEqual(entries["RESOURCE"]["directions"], ["input"])
            self.assertEqual(entries["TASK"]["value_shape"], "flag")
            self.assertEqual(entries["UOW"]["value_shape"], "flag")
            self.assertIsNone(entries["LENGTH"]["source_max_value_bytes"])
            self.assertEqual(options["constraints"]["required"], ["RESOURCE"])
            self.assertIn(
                ["MAXLIFETIME", "TASK", "UOW"],
                options["constraints"]["mutual_exclusions"],
            )

    def test_legacy_execution_options_reject_missing_nonlegacy_and_invalid_entries(self):
        retrieve = {
            "official_row": "ibm-cics-ts-6x-2026-08-31:api-commands:0175",
            "runtime_operation": "Retrieve",
            "execution_options": ["INTO", "LENGTH"],
        }
        mutations = {
            "nonlegacy": lambda routes: routes.append(dict(retrieve)),
            "unknown": lambda routes: routes.append(
                {**retrieve, "runtime_operation": "Unknown"}
            ),
            "invalid": lambda routes: routes.append(
                {**retrieve, "execution_options": ["not-an-option"]}
            ),
        }
        for name, mutate in mutations.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                self.fixture(root)
                path, catalog = self.legacy_execution_catalog(root)
                mutate(catalog["routes"])
                self.write_catalog(path, catalog)
                with self.assertRaises(cics_descriptors.DescriptorError):
                    cics_descriptors.build_contracts(root)

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

    def test_registry_shape_is_api_only_unique_and_has_no_default_fallback(self):
        contracts = cics_descriptors.build_contracts(ROOT)
        rows = [row for batch in contracts["batches"] for row in batch["commands"]]
        registry = [row["contract"]["registry"] for row in rows]
        self.assertEqual(len(rows), 263)
        self.assertEqual(len({row["official_row"] for row in rows}), 263)
        self.assertEqual(len({row["handler_id"] for row in registry}), 263)
        self.assertTrue(all(":api-commands:" in row["official_row"] for row in rows))
        self.assertFalse(any(":spi-" in row["official_row"] for row in rows))
        self.assertFalse(any(":fepi-" in row["official_row"] for row in rows))
        self.assertEqual(sum(row["readiness"] == "typed-runtime" for row in registry), 93)
        self.assertEqual(
            sum(row["readiness"] == "legacy-compatibility" for row in registry), 0
        )
        self.assertEqual(sum(row["advertised"] for row in registry), 93)
        self.assertEqual(sum(row["readiness"] == "unready" for row in registry), 170)
        self.assertFalse(contracts["automatic_registration"])
        self.assertIsNone(contracts["registry"]["default_handler"])
        self.assertEqual(
            contracts["participant_contract"]["mutating_rows"],
            sum(row["contract"]["effect"]["mutating"] is True for row in rows),
        )
        self.assertEqual(contracts["participant_contract"]["mutating_rows"], 68)
        self.assertEqual(contracts["participant_contract"]["bounded_effect_rows"], 170)
        self.assertEqual(contracts["participant_contract"]["explicit_uow_boundary_rows"], 1)
        self.assertFalse(
            contracts["participant_contract"]["unknown_outcome"]["automatic_redispatch"]
        )
        self.assertEqual(
            contracts["participant_contract"]["backend_applicability"],
            ["memory", "sqlite", "postgresql"],
        )

    def test_contract_binds_options_responses_and_hardened_policies(self):
        rows = [
            row
            for batch in cics_descriptors.build_contracts(ROOT)["batches"]
            for row in batch["commands"]
        ]
        abend = rows[0]
        self.assertEqual(abend["contract"]["eib_response"]["eibfn_bytes"], [0x0E, 0x0C])
        self.assertEqual(
            [entry["name"] for entry in abend["contract"]["options"]["entries"]],
            ["ABCODE", "CANCEL", "NODUMP", "NOHANDLE", "RESP", "RESP2"],
        )
        self.assertEqual(
            abend["contract"]["options"]["entries"][0]["host_max_value_bytes"], 128
        )
        self.assertIsNone(
            abend["contract"]["options"]["entries"][0]["source_max_value_bytes"]
        )
        common = {
            entry["name"]: entry
            for entry in abend["contract"]["options"]["entries"]
            if entry["name"] in {"NOHANDLE", "RESP", "RESP2"}
        }
        self.assertEqual(set(common), {"NOHANDLE", "RESP", "RESP2"})
        self.assertTrue(
            all("global-command-format" in entry["authorities"] for entry in common.values())
        )
        for row in rows:
            contract = row["contract"]
            internal_only = contract["grammar"]["status"] == "not-applicable"
            self.assertEqual(
                contract["capability"]["route"],
                None if internal_only else "host.cics.execute",
            )
            self.assertEqual(
                contract["resource"]["authorization"],
                "not-applicable" if internal_only else "typed-saf-before-dispatch",
            )
            self.assertFalse(contract["recovery"]["automatic_redispatch"])
            condition_policy = contract["eib_response"]["condition_policy"]
            self.assertEqual(condition_policy["authority"], "global-command-format")
            source_not_applicable = any(
                dimension["name"] == "conditions"
                and dimension["source_projection_state"]
                == "source-backed-not-applicable"
                for dimension in row["source_dimensions"]
            )
            self.assertEqual(condition_policy["nohandle"], not source_not_applicable)
            self.assertEqual(condition_policy["resp"], not source_not_applicable)
            self.assertEqual(condition_policy["resp2"], not source_not_applicable)
            self.assertTrue(condition_policy["resp2_requires_resp"])
            self.assertEqual(
                contract["eib_response"]["normal_return"],
                None
                if source_not_applicable
                else {"condition": "NORMAL", "resp": 0, "resp2": 0},
            )
            self.assertFalse(
                {"SYMBOL-INVREQ", "SYMBOL-NOTAUTH"}
                & set(contract["eib_response"]["symbolic_fragments"])
            )
            if contract["effect"]["mutating"] is True:
                self.assertTrue(contract["recovery"]["durable_effect_intent"])
                self.assertEqual(contract["recovery"]["unknown_outcome"], "preserve")
                self.assertEqual(contract["cancellation"]["post_dispatch"], "unknown-outcome")

    def test_bounded_contracts_do_not_invent_grammar_bounds_or_effects(self):
        rows = {
            row["label"]: row
            for batch in cics_descriptors.build_contracts(ROOT)["batches"]
            for row in batch["commands"]
        }
        for label in ("ISSUE RESET", "WAIT JOURNALNUM", "WRITE JOURNALNUM"):
            with self.subTest(label=label):
                grammar = rows[label]["contract"]["grammar"]
                self.assertEqual(grammar["status"], "bounded-ambiguity")
                self.assertEqual(grammar["variants"], [])

        for label in (
            "GET CONTAINER",
            "WEB READ",
            "GDS EXTRACT ATTRIBUTES",
        ):
            with self.subTest(label=label):
                effect = rows[label]["contract"]["effect"]
                self.assertEqual(effect["status"], "bounded-ambiguity")
                self.assertIsNone(effect["mutating"])
        self.assertEqual(rows["WAIT JOURNALNAME"]["contract"]["effect"]["status"], "resolved")
        self.assertFalse(rows["WAIT JOURNALNAME"]["contract"]["effect"]["mutating"])
        self.assertEqual(rows["WAIT JOURNALNUM"]["contract"]["effect"]["status"], "resolved")
        self.assertFalse(rows["WAIT JOURNALNUM"]["contract"]["effect"]["mutating"])
        self.assertEqual(rows["WRITE JOURNALNAME"]["contract"]["effect"]["status"], "resolved")
        self.assertTrue(rows["WRITE JOURNALNAME"]["contract"]["effect"]["mutating"])
        self.assertEqual(rows["WRITE JOURNALNUM"]["contract"]["effect"]["status"], "resolved")
        self.assertTrue(rows["WRITE JOURNALNUM"]["contract"]["effect"]["mutating"])

        abend = copy.deepcopy(rows["ABEND"])
        self.assertEqual(abend["contract"]["options"]["bounds_status"], "bounded-ambiguity")
        abend["contract"]["options"]["bounds_status"] = "resolved"
        with self.assertRaisesRegex(cics_descriptors.DescriptorError, "host ceiling"):
            cics_descriptors._validate_semantic_contract(abend, abend["contract"])

    def test_ready_effects_preserve_memory_flow_and_typed_ir_authority(self):
        rows = {
            row["label"]: row
            for batch in cics_descriptors.build_contracts(ROOT)["batches"]
            for row in batch["commands"]
        }
        expected_typed = {
            "ADDRESS SET": {"memory-read", "memory-write", "condition"},
            "READ": {
                "dataset-read",
                "memory-read",
                "memory-write",
                "condition",
                "transaction",
            },
            "REWRITE": {
                "dataset-write",
                "memory-read",
                "memory-write",
                "condition",
                "transaction",
            },
            "SYNCPOINT": {"memory-write", "condition", "transaction"},
            "SET ASSOCIATION USERCORRDATA": {
                "memory-read",
                "memory-write",
                "condition",
            },
            "HANDLE CONDITION": {"memory-read", "memory-write", "condition"},
            "HANDLE ABEND": {"memory-read", "memory-write", "condition"},
            "HANDLE AID": {"memory-read", "memory-write", "condition"},
            "LINK": {
                "memory-read",
                "memory-write",
                "program-control",
                "condition",
                "transaction",
            },
            "RELEASE": {
                "memory-read",
                "memory-write",
                "program-control",
                "condition",
                "transaction",
            },
            "STARTBR": {
                "dataset-read",
                "memory-read",
                "memory-write",
                "condition",
                "transaction",
            },
            "READNEXT": {
                "dataset-read",
                "memory-read",
                "memory-write",
                "condition",
                "transaction",
            },
            "READPREV": {
                "dataset-read",
                "memory-read",
                "memory-write",
                "condition",
                "transaction",
            },
            "ENDBR": {
                "dataset-read",
                "memory-read",
                "memory-write",
                "condition",
                "transaction",
            },
            "DELETE": {
                "dataset-write",
                "memory-read",
                "memory-write",
                "condition",
                "transaction",
            },
            "WRITE FILE": {
                "dataset-write",
                "memory-read",
                "memory-write",
                "condition",
                "transaction",
            },
            "WRITEQ TD": {
                "memory-read",
                "memory-write",
                "condition",
                "transaction",
            },
            "DELETEQ TD": {
                "memory-read",
                "memory-write",
                "condition",
                "transaction",
            },
            "FREEMAIN": {
                "memory-read",
                "memory-write",
                "condition",
                "transaction",
            },
            "GETMAIN": {
                "memory-read",
                "memory-write",
                "condition",
                "transaction",
            },
            "RECEIVE MAP": {
                "memory-read",
                "memory-write",
                "terminal-read",
                "suspension",
                "condition",
                "transaction",
            },
            "SEND MAP": {
                "memory-read",
                "memory-write",
                "terminal-write",
                "condition",
                "transaction",
            },
            "SEND TEXT": {
                "memory-read",
                "memory-write",
                "terminal-write",
                "condition",
                "transaction",
            },
            "ASSIGN": {"memory-write", "condition", "transaction"},
            "PURGE MESSAGE": {"memory-write", "condition", "transaction"},
        }
        for label, typed_effects in expected_typed.items():
            effects = set(rows[label]["contract"]["effect"]["ir_effects"])
            self.assertEqual(effects - {"audit", "security"}, typed_effects)

        read = rows["READ"]["contract"]
        self.assertEqual(read["effect"]["status"], "bounded-ambiguity")
        self.assertIsNone(read["effect"]["mutating"])
        self.assertEqual(read["resource"]["access_intent"], "bounded-ambiguity")
        self.assertEqual(read["capability"]["status"], "bounded-ambiguity")
        self.assertEqual(read["recovery"]["status"], "bounded-ambiguity")
        self.assertEqual(
            read["option_sensitive_semantics"][0]["options"], ["TOKEN", "UPDATE"]
        )
        self.assertEqual(
            rows["SYNCPOINT"]["contract"]["option_sensitive_semantics"],
            [
                {
                    "options": ["ROLLBACK"],
                    "predicate": "presence-selects-rollback-otherwise-commit",
                    "affected_dimensions": ["recovery"],
                    "status": "resolved",
                }
            ],
        )

        for label in ("STARTBR", "READNEXT", "READPREV", "ENDBR"):
            contract = rows[label]["contract"]
            self.assertEqual(contract["effect"]["status"], "resolved")
            self.assertFalse(contract["effect"]["mutating"])
            self.assertEqual(contract["resource"]["status"], "resolved")
            self.assertEqual(contract["audit"]["status"], "resolved")

        ready = [
            row
            for row in rows.values()
            if row["registration_status"] != "unready"
        ]
        self.assertEqual(
            sum("memory-read" in row["contract"]["effect"]["ir_effects"] for row in ready),
            84,
        )
        self.assertEqual(
            sum("memory-write" in row["contract"]["effect"]["ir_effects"] for row in ready),
            93,
        )

    def test_resource_selectors_are_family_scoped_and_input_only(self):
        rows = {
            row["label"]: row
            for batch in cics_descriptors.build_contracts(ROOT)["batches"]
            for row in batch["commands"]
        }

        def exact(label):
            return {
                selector["option"]
                for selector in rows[label]["contract"]["resource"]["selectors"]
            }

        def ambiguous(label):
            return {
                selector["option"]
                for selector in rows[label]["contract"]["resource"][
                    "ambiguous_selectors"
                ]
            }

        self.assertEqual(exact("ASSIGN"), set())
        self.assertEqual(ambiguous("ASSIGN"), set())
        self.assertEqual(exact("DEQ"), {"RESOURCE"})
        self.assertEqual(exact("ENQ"), {"RESOURCE"})
        self.assertEqual(exact("GET COUNTER"), {"COUNTER", "POOL"})
        self.assertEqual(
            exact("TRANSFORM DATATOJSON"),
            {"CHANNEL", "INCONTAINER", "OUTCONTAINER", "TRANSFORMER"},
        )
        self.assertEqual(exact("GET CONTAINER"), {"ACTIVITY", "CHANNEL", "CONTAINER"})
        self.assertEqual(exact("DELETE CHANNEL"), {"CHANNEL"})
        self.assertEqual(exact("CHECK TIMER"), {"TIMER"})
        self.assertEqual(exact("ADD SUBEVENT"), {"EVENT", "SUBEVENT"})
        self.assertEqual(
            exact("QUERY SECURITY"),
            {"RESCLASS", "RESID", "RESTYPE", "USERID"},
        )
        self.assertEqual(
            exact("INVOKE APPLICATION"),
            {
                "APPLICATION",
                "MAJORVERSION",
                "MINORVERSION",
                "OPERATION",
                "PLATFORM",
            },
        )
        self.assertEqual(
            exact("INVOKE SERVICE"),
            {"CHANNEL", "OPERATION", "SERVICE", "URI", "URIMAP"},
        )
        self.assertEqual(ambiguous("SPOOLCLOSE"), {"TOKEN"})
        self.assertIsNone(
            rows["SPOOLCLOSE"]["contract"]["resource"]["fallback_selector"]
        )
        self.assertIsNone(
            rows["CHECK TIMER"]["contract"]["resource"]["fallback_selector"]
        )
        self.assertIn(
            "ACTIVITYID",
            rows["DEFINE ACTIVITY"]["contract"]["resource"][
                "excluded_output_options"
            ],
        )
        self.assertIn(
            "HOST",
            rows["WEB PARSE URL"]["contract"]["resource"][
                "excluded_output_options"
            ],
        )
        for row in rows.values():
            for selector in row["contract"]["resource"]["selectors"]:
                self.assertTrue(
                    {"input", "input-output"} & set(selector["directions"])
                )
            resource = row["contract"]["resource"]
            if (
                row["registration_status"] == "unready"
                and resource["status"] != "not-applicable"
            ):
                self.assertIsNone(resource["fallback_selector"])
                self.assertEqual(resource["scope"], "bounded-ambiguity")
                self.assertEqual(resource["access_intent"], "bounded-ambiguity")
                self.assertEqual(resource["selector_completeness"], "bounded-ambiguity")
                self.assertEqual(resource["selector_authority"], "source-candidate-only")
            if resource["fallback_selector"] is not None:
                self.assertNotEqual(row["registration_status"], "unready")
                self.assertEqual(resource["status"], "resolved")

    def test_internal_only_and_registry_readiness_are_truthful(self):
        contracts = cics_descriptors.build_contracts(ROOT)
        rows = {
            row["label"]: row
            for batch in contracts["batches"]
            for row in batch["commands"]
        }
        internal = rows["CICSMESSAGE"]["contract"]
        for name in (
            "resource",
            "capability",
            "effect",
            "cancellation",
            "audit",
            "recovery",
        ):
            self.assertEqual(internal[name]["status"], "not-applicable")
        self.assertEqual(internal["registry"]["readiness"], "unready")

        typed = {
            row["label"]
            for row in rows.values()
            if row["contract"]["registry"]["readiness"] == "typed-runtime"
        }
        self.assertEqual(
            typed,
            {
                "ABEND",
                "ADDRESS",
                "ADDRESS SET",
                "ASKTIME",
                "ASKTIME ABSTIME",
                "CHANGE TASK",
                "DEFINE COUNTER",
                "DEFINE DCOUNTER",
                "DELETE COUNTER",
                "DELETE DCOUNTER",
                "GET COUNTER",
                "GET DCOUNTER",
                "QUERY COUNTER",
                "QUERY DCOUNTER",
                "REWIND COUNTER",
                "REWIND DCOUNTER",
                "UPDATE COUNTER",
                "UPDATE DCOUNTER",
                "DEQ",
                "DEFINE INPUT EVENT",
                "DOCUMENT CREATE",
                "DOCUMENT DELETE",
                "DOCUMENT INSERT",
                "DOCUMENT RETRIEVE",
                "DOCUMENT SET",
                "ENQ",
                "FORMATTIME",
                "HANDLE ABEND",
                "HANDLE AID",
                "HANDLE CONDITION",
                "IGNORE CONDITION",
                "INVOKE APPLICATION",
                "INVOKE SERVICE",
                "LINK",
                "LOAD",
                "RELEASE",
                "POP HANDLE",
                "PUSH HANDLE",
                "READ",
                "READQ TS",
                "REWRITE",
                "SET ASSOCIATION USERCORRDATA",
                "SOAPFAULT ADD",
                "SOAPFAULT CREATE",
                "SOAPFAULT DELETE",
                "SPOOLCLOSE",
                "SPOOLOPEN INPUT",
                "SPOOLOPEN OUTPUT",
                "SPOOLREAD",
                "SPOOLWRITE",
                "SUSPEND",
                "WAIT EVENT",
                "WAIT EXTERNAL",
                "SYNCPOINT",
                "TRANSFORM DATATOJSON",
                "TRANSFORM DATATOXML",
                "TRANSFORM JSONTODATA",
                "TRANSFORM XMLTODATA",
                "XCTL",
                "RETURN",
                "STARTBR",
                "RESETBR",
                "UNLOCK",
                "READNEXT",
                "READPREV",
                "READQ TD",
                "ENDBR",
                "DELETE",
                "DELETEQ TD",
                "DELETEQ TS",
                "DELAY",
                "FREEMAIN",
                "FREEMAIN64",
                "GETMAIN",
                "GETMAIN64",
                "WRITE FILE",
                "WRITEQ TD",
                "WRITEQ TS",
                "RECEIVE MAP",
                "SEND MAP",
                "SEND TEXT",
                "ASSIGN",
                "CANCEL",
                "PURGE MESSAGE",
                "RETRIEVE",
                "START",
                "WAIT JOURNALNAME",
                "WAIT JOURNALNUM",
                "WRITE JOURNALNAME",
                "WRITE JOURNALNUM",
                "WSACONTEXT BUILD",
                "WSACONTEXT DELETE",
                "WSACONTEXT GET",
                "WSAEPR CREATE",
            },
        )
        self.assertEqual(
            rows["BIF DEEDIT"]["contract"]["registry"]["family"],
            "builtin-function-control",
        )
        self.assertEqual(
            rows["BIF DIGEST"]["contract"]["registry"]["family"],
            "builtin-function-control",
        )

    def test_registry_recognition_uses_source_heads_and_discriminators(self):
        contracts = cics_descriptors.build_contracts(ROOT)
        rows = {
            row["label"]: cics_descriptors._registry_row_material(
                row, row["source_dimensions"], row["contract"]
            )
            for batch in contracts["batches"]
            for row in batch["commands"]
        }
        self.assertEqual(rows["WAIT"]["recognition_heads"], [["GDS", "WAIT"]])
        self.assertNotIn(["WAIT"], rows["WAIT"]["recognition_heads"])
        self.assertEqual(rows["ACQUIRE ACTIVITYID"]["recognition_heads"], [["ACQUIRE"]])
        self.assertIn("ACTIVITYID", rows["ACQUIRE ACTIVITYID"]["discriminator_options"])
        self.assertEqual(
            rows["ACQUIRE ACTIVITYID"]["required_discriminator_options"],
            ["ACTIVITYID"],
        )
        self.assertIn("ABSTIME", rows["ASKTIME ABSTIME"]["discriminator_options"])
        self.assertEqual(
            rows["ASKTIME ABSTIME"]["required_discriminator_options"], ["ABSTIME"]
        )
        self.assertEqual(rows["ASKTIME"]["forbidden_discriminator_options"], ["ABSTIME"])
        self.assertEqual(rows["WRITE FILE"]["recognition_heads"], [["WRITE"]])
        self.assertIn("FILE", rows["WRITE FILE"]["discriminator_options"])
        self.assertEqual(rows["WRITE FILE"]["required_discriminator_options"], ["FILE"])
        self.assertEqual(rows["REQUEST PASSTICKET"]["recognition_heads"], [["REQUEST"]])
        self.assertEqual(
            rows["REQUEST PASSTICKET"]["discriminator_options"], ["PASSTICKET"]
        )

        command_rows = {
            row["label"]: row
            for batch in contracts["batches"]
            for row in batch["commands"]
        }
        false_inferred_discriminators = {
            "HANDLE ABEND": "CANCEL",
            "ISSUE ERASEAUP": "WAIT",
            "SEND PAGE": "RELEASE",
            "TRACE": "ON",
        }
        for label, option in false_inferred_discriminators.items():
            self.assertNotIn(option, rows[label]["discriminator_options"])
            self.assertNotIn(option, rows[label]["required_discriminator_options"])
            self.assertNotIn(
                option,
                command_rows[label]["contract"]["options"]["constraints"][
                    "required"
                ],
            )

        required_valued_identity_qualifiers = {
            "ASKTIME ABSTIME": "ABSTIME",
            "DELETE CHANNEL": "CHANNEL",
            "ENTER TRACENUM": "TRACENUM",
            "EXTRACT CERTIFICATE": "CERTIFICATE",
            "QUERY CHANNEL": "CHANNEL",
            "REQUEST PASSTICKET": "PASSTICKET",
            "SET ASSOCIATION USERCORRDATA": "USERCORRDATA",
            "VERIFY PASSWORD": "PASSWORD",
        }
        for label, option in required_valued_identity_qualifiers.items():
            self.assertIn(option, rows[label]["discriminator_options"])
            self.assertIn(option, rows[label]["required_discriminator_options"])
            self.assertIn(
                option,
                command_rows[label]["contract"]["options"]["constraints"][
                    "required"
                ],
            )

        for label, label_operand in (
            ("HANDLE CONDITION", "optional"),
            ("IGNORE CONDITION", "forbidden"),
        ):
            self.assertEqual(rows[label]["recognition_heads"], [label.split()])
            self.assertNotIn(
                "CONDITION-NAME",
                [option["name"] for option in rows[label]["options"]],
            )
            self.assertEqual(
                rows[label]["condition_clauses"]["label_operand"], label_operand
            )
            self.assertEqual(
                rows[label]["condition_clauses"]["minimum_occurrences"], 1
            )
            self.assertEqual(
                rows[label]["condition_clauses"]["maximum_occurrences"], 16
            )

        self.assertEqual(rows["HANDLE AID"]["recognition_heads"], [["HANDLE", "AID"]])
        self.assertIn(
            'pub const CICS_APPLICATION_AID_NAMES: &[&str] =',
            (ROOT / "crates/foundation/mainframe-env-ir/src/generated/cics_application_registry.rs").read_text(),
        )

        activity_constraints = command_rows["ACQUIRE ACTIVITYID"]["contract"]["options"][
            "constraints"
        ]
        self.assertEqual(activity_constraints["required"], ["ACTIVITYID"])
        self.assertEqual(activity_constraints["alternatives"], [])
        self.assertEqual(activity_constraints["mutual_exclusions"], [])
        self.assertEqual(
            command_rows["ACQUIRE PROCESS"]["contract"]["options"]["constraints"][
                "required"
            ],
            ["PROCESS", "PROCESSTYPE"],
        )

        signatures = {}
        for label, registry in rows.items():
            constraints = registry["constraints"]
            for head in registry["recognition_heads"]:
                signature = (
                    tuple(head),
                    tuple(registry["required_discriminator_options"]),
                    tuple(registry["forbidden_discriminator_options"]),
                    tuple(registry["discriminator_options"]),
                    tuple(constraints["required"]),
                    tuple(
                        tuple(group["members"])
                        for group in constraints["alternatives"]
                    ),
                )
                self.assertNotIn(signature, signatures, (label, signatures.get(signature)))
                signatures[signature] = label

    def test_condition_name_authority_is_complete_and_domain_bound(self):
        authority = cics_descriptors.build_contracts(ROOT)[
            "condition_name_authority"
        ]
        pairs = [[item["name"], item["code"]] for item in authority["conditions"]]
        self.assertEqual(len(pairs), 121)
        self.assertEqual(pairs, sorted(pairs))
        self.assertEqual(authority["allowed_names"], [name for name, _ in pairs])
        self.assertEqual(
            hashlib.sha256(
                json.dumps(pairs, separators=(",", ":")).encode("utf-8")
            ).hexdigest(),
            "33e16a2f60928dbd2168e1e3ad6e5e5b441fdca1c493c55b493d6452bf4d27e0",
        )
        codes = {item["name"]: item["code"] for item in authority["conditions"]}
        self.assertEqual(
            {name: codes[name] for name in ("NORMAL", "ERROR", "BUSY")},
            {"NORMAL": 0, "ERROR": 1, "BUSY": 128},
        )

    def test_applicability_preserves_language_dpl_and_context_predicates(self):
        rows = [
            row
            for batch in cics_descriptors.build_contracts(ROOT)["batches"]
            for row in batch["commands"]
        ]
        self.assertEqual(
            {state: sum(row["contract"]["applicability"]["dpl_server"] == state for row in rows)
             for state in ("allowed", "restricted", "not-applicable")},
            {"allowed": 206, "restricted": 56, "not-applicable": 1},
        )
        self.assertEqual(
            {state: sum(row["contract"]["applicability"]["threadsafe"] == state for row in rows)
             for state in ("yes", "conditional", "no", "not-applicable")},
            {"yes": 101, "conditional": 20, "no": 141, "not-applicable": 1},
        )
        self.assertEqual(
            {state: sum(row["contract"]["applicability"]["cobol"] == state for row in rows)
             for state in ("allowed", "not-applicable")},
            {"allowed": 245, "not-applicable": 18},
        )
        by_label = {row["label"]: row for row in rows}
        self.assertEqual(
            by_label["WEB READ"]["contract"]["applicability"]["dpl_restriction"][
                "resp2"
            ],
            1,
        )
        self.assertEqual(
            by_label["EXTRACT TCPIP"]["contract"]["applicability"][
                "dpl_restriction"
            ]["resp2"],
            5,
        )
        self.assertEqual(
            by_label["CONNECT PROCESS"]["contract"]["applicability"][
                "dpl_restriction"
            ]["restriction_predicates"],
            [
                "principal-facility",
                "connect-process-principal-facility-error-is-dpl",
            ],
        )
        send_restriction = by_label["SEND"]["contract"]["applicability"][
            "dpl_restriction"
        ]
        self.assertEqual(send_restriction["restriction_match"], "any-of")
        self.assertTrue(send_restriction["prohibited_options"])
        self.assertEqual(send_restriction["restriction_predicates"], ["principal-facility"])
        self.assertEqual(
            by_label["WAIT TERMINAL"]["contract"]["applicability"][
                "dpl_restriction"
            ]["restriction_predicates"],
            ["principal-facility"],
        )
        self.assertTrue(
            by_label["WEB READ"]["contract"]["applicability"]["context_predicates"]
        )
        self.assertEqual(
            by_label["GDS EXTRACT ATTRIBUTES"]["contract"]["applicability"][
                "dpl_restriction"
            ]["kind"],
            "bounded-ambiguity",
        )
        self.assertTrue(
            by_label["GDS EXTRACT ATTRIBUTES"]["contract"]["applicability"][
                "context_predicates"
            ]
        )
        self.assertEqual(
            by_label["GDS EXTRACT ATTRIBUTES"]["contract"]["applicability"]["cobol"],
            "not-applicable",
        )

    def test_resp2_trigger_evidence_is_preserved_without_claiming_predicates(self):
        rows = [
            row
            for batch in cics_descriptors.build_contracts(ROOT)["batches"]
            for row in batch["commands"]
        ]
        outcomes = [
            (row, outcome)
            for row in rows
            for condition in row["contract"]["eib_response"]["conditions"]
            for outcome in condition["outcomes"]
            if outcome["resp2"] is not None
        ]
        self.assertTrue(outcomes)
        for row, outcome in outcomes:
            self.assertTrue(outcome["trigger_fragment_sha256s"])
            self.assertTrue(outcome["candidate_ids"])
            self.assertIsNone(outcome["trigger_predicate"])
            self.assertEqual(outcome["trigger_status"], "bounded-ambiguity")
            self.assertEqual(
                row["contract"]["eib_response"]["status"],
                "bounded-ambiguity",
            )

    def test_unknown_eibfn_processor_and_registry_mutations_fail_closed(self):
        with self.assertRaisesRegex(cics_descriptors.DescriptorError, "unmapped EIBFN"):
            cics_descriptors._contract_family(
                {
                    "official_row": "ibm-cics-ts-6x-2026-08-31:api-commands:9999",
                    "label": "UNKNOWN",
                    "eibfn": "FF00",
                },
                None,
            )

        contracts = cics_descriptors.build_contracts(ROOT)
        changed = copy.deepcopy(contracts)
        changed["batches"][0]["commands"][0]["contract"]["registry"]["advertised"] = False
        self.assertNotEqual(
            contracts["contract_sha256"], cics_descriptors.contract_digest(changed)
        )

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
