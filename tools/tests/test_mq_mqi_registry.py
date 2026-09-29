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
            mq_registry.OFFICIAL_CATALOG_PATH,
            mq_registry.TOPIC_MANIFEST_PATH,
            mq_registry.OUTPUT_PATH,
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

    def test_repository_registry_is_fresh_and_exact(self):
        mq_registry.check(ROOT)
        rows = mq_registry.load(ROOT)
        self.assertEqual(len(rows), 26)
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
