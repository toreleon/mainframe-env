from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest


REPOSITORY = Path(__file__).resolve().parents[2]
TOOL = REPOSITORY / "tools/assurance_gates.py"
SPEC = importlib.util.spec_from_file_location("assurance_gates", TOOL)
assert SPEC is not None and SPEC.loader is not None
GATES = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = GATES
SPEC.loader.exec_module(GATES)


class AssuranceGateTests(unittest.TestCase):
    def fixture(self) -> Path:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        registry = json.loads((REPOSITORY / GATES.REGISTRY).read_text())
        paths = [GATES.REGISTRY, "fuzz/Cargo.toml", registry["model"]["test_target"]]
        for target in registry["fuzz"]["targets"]:
            paths.append(target["source"])
            source = REPOSITORY / target["corpus"]
            destination = root / target["corpus"]
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copytree(source, destination)
        for raw in paths:
            source = REPOSITORY / raw
            destination = root / raw
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, destination)
        for package in registry["coverage"]["package_roots"]:
            (root / package / "src").mkdir(parents=True, exist_ok=True)
        return root

    def coverage_report(self, root: Path) -> dict:
        registry = GATES.load_registry(root)
        coverage = registry["coverage"]
        covered_lines = coverage["minimum_covered_lines"]
        total_lines = max(coverage["minimum_total_lines"], covered_lines + 1)
        covered_functions = coverage["minimum_covered_functions"]
        total_functions = covered_functions + 1
        files = [
            {"filename": str(root / package / "src/lib.rs"), "summary": {}}
            for package in registry["coverage"]["package_roots"]
        ]
        return {
            "type": "llvm.coverage.json.export",
            "version": "2.0.1",
            "data": [
                {
                    "files": files,
                    "totals": {
                        "lines": {
                            "count": total_lines,
                            "covered": covered_lines,
                            "percent": covered_lines * 100 / total_lines,
                        },
                        "functions": {
                            "count": total_functions,
                            "covered": covered_functions,
                            "percent": covered_functions * 100 / total_functions,
                        },
                    },
                }
            ],
        }

    def test_repository_inventory_has_bounded_parser_decoder_and_model_gates(self):
        inventory = GATES.validate_inventory(REPOSITORY)
        self.assertEqual(inventory["fuzz_targets"], 2)
        self.assertGreaterEqual(inventory["corpus_files"], 2)
        self.assertGreaterEqual(inventory["model_tests"], 3)
        self.assertEqual(inventory["coverage_packages"], 4)

    def test_missing_corpus_and_empty_model_fail_closed(self):
        root = self.fixture()
        registry = GATES.load_registry(root)
        corpus = root / registry["fuzz"]["targets"][0]["corpus"]
        next(corpus.iterdir()).unlink()
        with self.assertRaisesRegex(ValueError, "empty corpus"):
            GATES.validate_inventory(root)

        root = self.fixture()
        model = root / GATES.load_registry(root)["model"]["test_target"]
        model.write_text("fn no_model() {}\n")
        with self.assertRaisesRegex(ValueError, "model gate is empty"):
            GATES.validate_inventory(root)

    def test_coverage_report_must_be_nonempty_and_include_every_package(self):
        root = self.fixture()
        report = root / "coverage.json"
        value = self.coverage_report(root)
        report.write_text(json.dumps(value))
        observed = GATES.validate_coverage(root, report)
        self.assertEqual(
            observed["covered_lines"],
            GATES.load_registry(root)["coverage"]["minimum_covered_lines"],
        )

        value["data"][0]["totals"]["lines"]["covered"] = 0
        value["data"][0]["totals"]["lines"]["percent"] = 0.0
        report.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError, "covered lines"):
            GATES.validate_coverage(root, report)

        value = self.coverage_report(root)
        value["data"][0]["files"].pop()
        report.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError, "omits required package"):
            GATES.validate_coverage(root, report)

    def test_ci_full_tier_receipts_every_material_assurance_gate(self):
        ci_spec = importlib.util.spec_from_file_location(
            "r24_ci_assurance", REPOSITORY / "tools/ci_assurance.py"
        )
        assert ci_spec is not None and ci_spec.loader is not None
        ci = importlib.util.module_from_spec(ci_spec)
        ci_spec.loader.exec_module(ci)
        expected = {"fuzz-smoke", "fuzz-periodic", "model-check", "coverage-baseline"}
        self.assertTrue(expected <= set(ci.FULL))
        pipeline = (REPOSITORY / "Jenkinsfile").read_text()
        for gate in expected:
            self.assertIn(f"--gate {gate}", pipeline)
        self.assertIn(
            "--gate model-check --expect-tests -- tools/run_model_assurance.sh",
            pipeline,
        )


if __name__ == "__main__":
    unittest.main()
