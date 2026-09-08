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

    def test_repository_generated_descriptors_are_fresh_and_exhaustive(self):
        cics_descriptors.check(ROOT)
        source = cics_descriptors.render(ROOT)
        self.assertEqual(source.count("CicsOperation::"), 50)
        self.assertIn("CicsCommandFamily::TaskControl", source)
        self.assertIn("CicsCommandFamily::Recovery", source)

    def test_check_rejects_stale_generated_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            cics_descriptors.generate(root)
            cics_descriptors.check(root)
            output = root / cics_descriptors.OUTPUT_PATH
            output.write_text(output.read_text() + "// stale\n")
            with self.assertRaisesRegex(cics_descriptors.DescriptorError, "is stale"):
                cics_descriptors.check(root)

    def test_catalog_rows_must_match_the_official_authority(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.fixture(root)
            path = root / cics_descriptors.CATALOG_PATH
            catalog = json.loads(path.read_text())
            catalog["commands"][0]["syntax"] = "NOT OFFICIAL"
            path.write_text(json.dumps(catalog))
            with self.assertRaisesRegex(cics_descriptors.DescriptorError, "official row"):
                cics_descriptors.render(root)


if __name__ == "__main__":
    unittest.main()
