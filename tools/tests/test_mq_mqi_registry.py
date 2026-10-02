import copy
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest


TOOL = Path(__file__).resolve().parents[1] / "generate_mq_mqi_registry.py"
ROOT = TOOL.parent.parent
SPEC = importlib.util.spec_from_file_location("generate_mq_mqi_registry", TOOL)
mq_registry = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(mq_registry)


class MqMqiRegistryTests(unittest.TestCase):
    def fixture(self, root: Path) -> None:
        for relative in [
            mq_registry.SOURCE_LIST_PATH,
            mq_registry.CONTRACT_CATALOG_PATH,
            mq_registry.OFFICIAL_CATALOG_PATH,
            mq_registry.TOPIC_MANIFEST_PATH,
            mq_registry.OUTPUT_PATH,
            mq_registry.CONTRACT_OUTPUT_PATH,
            mq_registry.wire.MANIFEST,
            mq_registry.wire.OUTPUT,
        ]:
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)

    def source(self, root: Path) -> tuple[Path, dict]:
        path = root / mq_registry.SOURCE_LIST_PATH
        return path, json.loads(path.read_text())

    def write_source(self, root: Path, source: dict) -> None:
        catalog_path = root / mq_registry.OFFICIAL_CATALOG_PATH
        source["official_catalog"]["sha256"] = mq_registry._sha256(catalog_path)
        (root / mq_registry.SOURCE_LIST_PATH).write_text(
            json.dumps(source, indent=2) + "\n"
        )

    def contract(self, root: Path) -> tuple[Path, dict]:
        path = root / mq_registry.CONTRACT_CATALOG_PATH
        return path, json.loads(path.read_text())

    def write_contract(self, root: Path, contract: dict) -> None:
        (root / mq_registry.CONTRACT_CATALOG_PATH).write_text(
            json.dumps(contract, indent=2) + "\n"
        )

    def assert_contract_mutation_is_stale(self, mutate) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            _, contract = self.contract(root)
            mutate(contract)
            self.write_contract(root, contract)
            with self.assertRaisesRegex(ValueError, "stale generated MQ MQI registry|historical MQ catalog"):
                mq_registry.check(root)

    def test_repository_registry_is_fresh_and_exact(self):
        mq_registry.check(ROOT)
        rows = mq_registry.load(ROOT)
        contracts = mq_registry.load_contract(ROOT)
        self.assertEqual(len(rows), 26)
        self.assertEqual(len(contracts), 26)
        self.assertEqual(rows[0]["label"], "MQBACK")
        self.assertEqual(rows[-1]["label"], "MQSUBRQ")
        self.assertEqual(
            next(row for row in rows if row["label"] == "MQMHBUF")[
                "source_positions"
            ],
            [18, 25],
        )
        self.assertEqual(sum(len(row["source_positions"]) for row in rows), 27)
        self.assertEqual(
            mq_registry.render(ROOT).count("MqMqiCallIdentityDescriptor {"), 26
        )
        self.assertEqual(
            sum(len(call["parameters"]) for call in contracts),
            169,
        )
        self.assertTrue(all(call["source_status"] == "verified" for call in contracts))
        mqinq = next(call for call in contracts if call["label"] == "MQINQ")
        self.assertEqual(mqinq["signature_status"], "source-verified")
        self.assertEqual(len(mqinq["parameters"]), 10)
        self.assertEqual(mqinq["parameters"][5]["direction"], "output")
        self.assertEqual(mqinq["parameters"][7]["direction"], "output")
        self.assertEqual(
            mq_registry.render_contract(ROOT).count("MqMqiParameterDescriptor {"),
            169,
        )
        mqbufmh = next(call for call in contracts if call["label"] == "MQBUFMH")
        hmsg = next(row for row in mqbufmh["parameters"] if row["name"] == "Hmsg")
        self.assertEqual(hmsg["data_type"], "MQHMSG")
        self.assertEqual(hmsg["symbolic_identities"], ["MQHMSG"])
        self.assertEqual(
            hmsg["source_spelling_anomaly"], mq_registry.MQBUFMH_HMSG_ANOMALY
        )
        self.assertEqual(
            sum(
                parameter["source_spelling_anomaly"] is not None
                for call in contracts
                for parameter in call["parameters"]
            ),
            1,
        )

    def test_signature_mutation_invalidates_generated_registry(self):
        def mutate(contract):
            call = next(row for row in contract["calls"] if row["label"] == "MQGET")
            parameter = next(row for row in call["parameters"] if row["name"] == "Buffer")
            parameter["data_type"] = "MQBYTExAlteredLength"

        self.assert_contract_mutation_is_stale(mutate)

    def test_option_and_selector_mutations_invalidate_generated_registry(self):
        def mutate_option(contract):
            call = next(row for row in contract["calls"] if row["label"] == "MQOPEN")
            parameter = next(row for row in call["parameters"] if row["name"] == "Options")
            parameter["symbolic_identities"] = ["MQOO_ALTERED"]

        def mutate_selector(contract):
            call = next(row for row in contract["calls"] if row["label"] == "MQSET")
            parameter = next(row for row in call["parameters"] if row["name"] == "Selectors")
            parameter["symbolic_identities"] = ["MQIA", "MQXA"]

        self.assert_contract_mutation_is_stale(mutate_option)
        self.assert_contract_mutation_is_stale(mutate_selector)

    def test_handle_role_mutation_invalidates_generated_registry(self):
        def mutate(contract):
            call = next(row for row in contract["calls"] if row["label"] == "MQOPEN")
            parameter = next(row for row in call["parameters"] if row["name"] == "Hobj")
            parameter["handle_action"] = "use"

        self.assert_contract_mutation_is_stale(mutate)

    def test_mqbufmh_source_spelling_normalization_is_required_and_unique(self):
        for mutation in ["missing", "bogus-canonical", "duplicate"]:
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.fixture(root)
                _, contract = self.contract(root)
                mqbufmh = next(
                    row for row in contract["calls"] if row["label"] == "MQBUFMH"
                )
                hmsg = next(
                    row for row in mqbufmh["parameters"] if row["name"] == "Hmsg"
                )
                if mutation == "missing":
                    del hmsg["source_spelling_anomaly"]
                    expected = "source-spelling anomaly is missing"
                elif mutation == "bogus-canonical":
                    hmsg["data_type"] = "MQHMQSG"
                    hmsg["symbolic_identities"] = ["MQHMQSG"]
                    expected = "source-spelling normalization differs"
                else:
                    mqcrtmh = next(
                        row for row in contract["calls"] if row["label"] == "MQCRTMH"
                    )
                    canonical_hmsg = next(
                        row for row in mqcrtmh["parameters"] if row["name"] == "Hmsg"
                    )
                    canonical_hmsg["source_spelling_anomaly"] = copy.deepcopy(
                        hmsg["source_spelling_anomaly"]
                    )
                    expected = "source-spelling normalization differs"
                self.write_contract(root, contract)
                with self.assertRaisesRegex(ValueError, expected):
                    mq_registry.load_contract(root)

    def test_structure_version_mutation_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            _, contract = self.contract(root)
            call = next(row for row in contract["calls"] if row["label"] == "MQPUT")
            parameter = next(row for row in call["parameters"] if row["name"] == "MsgDesc")
            parameter["structure_version_identity"] = "MQMD_WRONG_VERSION"
            self.write_contract(root, contract)
            with self.assertRaisesRegex(ValueError, "structure version identity differs"):
                mq_registry.load_contract(root)

    def test_completion_and_reason_mutations_are_rejected(self):
        for role, replacement, message in [
            ("completion", "MQCC_ALTERED", "completion identity differs"),
            ("reason", "MQRC_ALTERED", "reason identity differs"),
        ]:
            with self.subTest(role=role), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.fixture(root)
                _, contract = self.contract(root)
                call = next(row for row in contract["calls"] if row["label"] == "MQCMIT")
                parameter = next(row for row in call["parameters"] if role in row["roles"])
                parameter["symbolic_identities"] = [replacement]
                self.write_contract(root, contract)
                with self.assertRaisesRegex(ValueError, message):
                    mq_registry.load_contract(root)

    def test_verified_source_cannot_drop_its_signature(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            _, contract = self.contract(root)
            call = next(row for row in contract["calls"] if row["label"] == "MQINQ")
            call["parameters"] = []
            self.write_contract(root, contract)
            with self.assertRaisesRegex(ValueError, "verified MQ signature is empty"):
                mq_registry.load_contract(root)

    def test_duplicate_must_point_to_the_first_source_position(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            _, source = self.source(root)
            duplicate = next(
                row
                for row in source["rows"]
                if row["duplicate_of_source_position"] is not None
            )
            duplicate["duplicate_of_source_position"] = 17
            self.write_source(root, source)
            with self.assertRaisesRegex(ValueError, "duplicate provenance"):
                mq_registry.load(root)

    def test_normalized_source_order_must_match_the_official_catalog(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            _, source = self.source(root)
            source["rows"][0], source["rows"][1] = (
                copy.deepcopy(source["rows"][1]),
                copy.deepcopy(source["rows"][0]),
            )
            source["rows"][0]["source_position"] = 1
            source["rows"][1]["source_position"] = 2
            self.write_source(root, source)
            with self.assertRaisesRegex(ValueError, "normalized source order"):
                mq_registry.load(root)

    def test_source_topic_digest_must_match_the_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            _, source = self.source(root)
            source["source"]["sha256"] = "0" * 64
            self.write_source(root, source)
            with self.assertRaisesRegex(ValueError, "topic digest differs"):
                mq_registry.load(root)

    def test_catalog_bytes_are_bound_before_projection(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            catalog_path = root / mq_registry.OFFICIAL_CATALOG_PATH
            catalog = json.loads(catalog_path.read_text())
            catalog["units"][0]["rows"][0]["label"] = "MQALTERED"
            catalog_path.write_text(json.dumps(catalog, indent=2) + "\n")
            with self.assertRaisesRegex(ValueError, "official catalog binding differs"):
                mq_registry.load(root)


if __name__ == "__main__":
    unittest.main()
