import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import zipfile


TOOL = Path(__file__).resolve().parents[1] / "supply_chain.py"
ROOT = TOOL.parent.parent
SPEC = importlib.util.spec_from_file_location("supply_chain", TOOL)
supply_chain = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(supply_chain)


def archive(path: Path, fields: dict[str, str]) -> None:
    manifest = "\r\n".join(f"{name}: {value}" for name, value in fields.items()) + "\r\n\r\n"
    with zipfile.ZipFile(path, "w") as output:
        output.writestr("META-INF/MANIFEST.MF", manifest)


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


class SupplyChainTests(unittest.TestCase):
    def test_repository_lock_closes_tracked_inputs_and_full_msrv(self):
        ci_lock, jenkins_lock = supply_chain.check_repository(ROOT)
        self.assertEqual(ci_lock["rust"]["msrv"]["version"], "1.95.0")
        self.assertEqual(ci_lock["rust"]["fuzz"]["toolchain"], "nightly-2026-09-01")
        self.assertEqual(ci_lock["tools"]["cargo-fuzz"]["version"], "0.13.2")
        self.assertEqual(ci_lock["tools"]["cargo-llvm-cov"]["version"], "0.9.1")
        self.assertEqual(len(jenkins_lock["plugins"]), 63)
        self.assertEqual(
            ci_lock["tracked_remote_inputs"],
            {
                "github_actions": [],
                "container_images": [
                    "docker.io/jenkins/jenkins@sha256:c1e4c349365f6d16d88595b2c5f7e8ff39b8ae1d061f62420bac193b4b9616d0",
                    "docker.io/library/debian@sha256:5ae3c39ebd15e229dcedd5cee596b2497182493d41ff162e824ba13fc1b2b867",
                    "docker.io/library/docker@sha256:51e23845f5caff1e688a2fae003b0c69d635c9200ad544731db1593731df1d3a",
                    "docker.io/library/postgres@sha256:1c59e2c3c818eaa0f0628f695b36e7c9e362d6b219b36a54a32df645cbd7e1af",
                    "docker.io/library/python@sha256:3cd9086bdb30f7c9bc08a3fa621d9842e0d3f6f9291aeb4677e0547817c10b12",
                    "docker.io/library/rust@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922",
                ],
                "package_install_commands": [],
            },
        )
        tracked = set(supply_chain.tracked_files(ROOT))
        self.assertTrue(set(ci_lock["unsupported_local_inputs"]).isdisjoint(tracked))

    def test_scanner_accepts_only_digest_pinned_remote_inputs(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            workflow = root / ".github/workflows/ci.yml"
            workflow.parent.mkdir(parents=True)
            sha = "a" * 40
            image = "registry.example/tool@sha256:" + "b" * 64
            workflow.write_text(
                f"runs-on: [self-hosted, linux]\nsteps:\n  - uses: owner/action@{sha}\ncontainer:\n  image: {image}\n"
            )
            observed = supply_chain.scan_external_inputs(root, [".github/workflows/ci.yml"])
            self.assertEqual(observed["github_actions"], [f"owner/action@{sha}"])
            self.assertEqual(observed["container_images"], [image])

            workflow.write_text("runs-on: ubuntu-latest\nsteps:\n  - uses: owner/action@main\n")
            with self.assertRaises(supply_chain.SupplyChainError):
                supply_chain.scan_external_inputs(root, [".github/workflows/ci.yml"])

    def test_scanner_rejects_ambient_package_installation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            script = root / "tools/jenkins/setup.sh"
            script.parent.mkdir(parents=True)
            script.write_text("#!/bin/bash\nbrew install jenkins-lts\n")
            observed = supply_chain.scan_external_inputs(root, ["tools/jenkins/setup.sh"])
            self.assertEqual(
                observed["package_install_commands"],
                ["tools/jenkins/setup.sh:2:brew install jenkins-lts"],
            )

    def test_compose_local_tags_require_their_tracked_build_recipe(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'docker').mkdir()
            compose = root / 'docker/compose.yaml'
            recipe = 'docker/runtime.Dockerfile'
            (root / recipe).write_text('FROM example/base@sha256:' + 'a' * 64 + '\n')
            (root / 'docker/inputs.lock.json').write_text(json.dumps({
                'local_images': {'mainframe-env-runtime:dev': recipe}}))
            text = ('services:\n  server:\n    image: mainframe-env-runtime:dev\n'
                    '    build:\n      context: ..\n      dockerfile: docker/runtime.Dockerfile\n')
            compose.write_text(text)
            tracked = ['docker/compose.yaml', recipe]
            supply_chain.scan_external_inputs(root, tracked)
            for bad in (text.replace('    build:', '    ignored:'),
                        text.replace('mainframe-env-runtime:dev', 'postgres:latest')):
                compose.write_text(bad)
                with self.assertRaises(supply_chain.SupplyChainError):
                    supply_chain.scan_external_inputs(root, tracked)

    def test_jenkins_artifacts_are_hash_version_and_closure_bound(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            controller = root / "jenkins.war"
            plugins = root / "plugins"
            plugins.mkdir()
            archive(controller, {"Jenkins-Version": "1.2.3"})
            archive(
                plugins / "root.jpi",
                {
                    "Short-Name": "root",
                    "Plugin-Version": "4.5.6",
                    "Plugin-Dependencies": "dependency:1.0",
                },
            )
            archive(
                plugins / "dependency.jpi",
                {"Short-Name": "dependency", "Plugin-Version": "1.0"},
            )
            lock = {
                "controller": {"version": "1.2.3", "sha256": digest(controller)},
                "plugins": [
                    {"id": "dependency", "version": "1.0", "sha256": digest(plugins / "dependency.jpi")},
                    {"id": "root", "version": "4.5.6", "sha256": digest(plugins / "root.jpi")},
                ],
            }
            supply_chain.verify_jenkins_artifacts(lock, controller, plugins)
            (plugins / "dependency.jpi").write_bytes(b"changed")
            with self.assertRaises(supply_chain.SupplyChainError):
                supply_chain.verify_jenkins_artifacts(lock, controller, plugins)

    def test_offline_tree_identity_is_deterministic_and_content_sensitive(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "crate").mkdir()
            source = root / "crate/lib.rs"
            source.write_bytes(b"one\n")
            first = supply_chain.tree_identity(root)
            self.assertEqual(first, supply_chain.tree_identity(root))
            source.write_bytes(b"two\n")
            self.assertNotEqual(first["sha256"], supply_chain.tree_identity(root)["sha256"])

    def test_locks_are_bounded_json_with_terminal_newline(self):
        for relative in [supply_chain.CI_LOCK_PATH, supply_chain.JENKINS_LOCK_PATH]:
            path = ROOT / relative
            value = json.loads(path.read_text())
            self.assertIsInstance(value, dict)
            self.assertTrue(path.read_text().endswith("\n"))
            self.assertLess(path.stat().st_size, supply_chain.MAX_JSON_BYTES)


if __name__ == "__main__":
    unittest.main()
