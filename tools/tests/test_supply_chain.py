import hashlib
import base64
import copy
import io
import importlib.util
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest import mock
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
                "container_images": ci_lock["tracked_remote_inputs"]["container_images"],
                "package_install_commands": [],
            },
        )
        self.assertEqual(len(ci_lock["tracked_remote_inputs"]["container_images"]), 2)
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

    def test_docker_stages_require_immutable_external_bases(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / "tools/sandbox/Dockerfile"
            path.parent.mkdir(parents=True)
            image = "registry.example/runtime@sha256:" + "a" * 64
            path.write_text(f"FROM {image} AS build\nFROM build AS runtime\n")
            observed = supply_chain.scan_external_inputs(root, ["tools/sandbox/Dockerfile"])
            self.assertEqual(observed["container_images"], [image])
            for source in ("FROM runtime:latest\n", "FROM ${BASE}\n"):
                path.write_text(source)
                with self.assertRaises(supply_chain.SupplyChainError):
                    supply_chain.scan_external_inputs(root, ["tools/sandbox/Dockerfile"])

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


PROFILE = "public-client-linux-x86_64"
LIBRARIES = ("loader", "libdl", "libstdcxx", "libm", "libgcc", "libpthread", "libc", "libnss_files")
TREE_GOLDEN = "22b67180f6f257fd0071863afcf12ddad2243aec14d982b39b4b2f7873ce467f"


def inert_tar(path, members, compression):
    with tarfile.open(path, "w:" + compression) as output:
        for name, data, mode, kind in members:
            info = tarfile.TarInfo(name)
            info.mode = mode
            info.type = kind
            if kind in (tarfile.SYMTYPE, tarfile.LNKTYPE):
                info.linkname = data.decode()
            else:
                info.size = len(data)
            output.addfile(info, io.BytesIO(data) if kind == tarfile.REGTYPE else None)


class DevelopmentInputTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.files = {}
        def pin(role, data, mode=0o644):
            path = self.root / role
            path.write_bytes(data)
            path.chmod(mode)
            self.files[role] = path
            return {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
        binary = pin("node", b"inert node\n", 0o755)
        binary["mode"] = 0o755
        license_bytes = b"inert license\n"
        self.node_members = [
            ("node-v24.19.0-linux-x64/bin/node", b"inert node\n", 0o755, tarfile.REGTYPE),
            ("node-v24.19.0-linux-x64/LICENSE", license_bytes, 0o644, tarfile.REGTYPE),
        ]
        node_archive = self.root / "node-archive"
        inert_tar(node_archive, self.node_members, "xz")
        node_archive.chmod(0o644)
        self.files["node-archive"] = node_archive
        self.tree = self.root / "tree"
        self.tree.mkdir()
        self.payloads = {
            "package/package.json": b'{"name":"@zowe/cli","version":"8.39.0"}',
            "package/lib/main.js": b"fixture-only\n",
            "package/empty": b"",
        }
        for name, data in self.payloads.items():
            path = self.tree / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            path.chmod(0o644)
        self.zowe_members = [(name, data, 0o644, tarfile.REGTYPE) for name, data in reversed(list(self.payloads.items()))]
        self.files["zowe-archive"] = self.root / "zowe-archive"
        inert_tar(self.files["zowe-archive"], self.zowe_members, "gz")
        self.files["zowe-archive"].chmod(0o644)
        def archive_pin(role, url):
            data = self.files[role].read_bytes()
            return {"url": url, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
        self.profile = {
            "platform": "linux-x86_64", "purpose": "optional-development-client",
            "node": {
                "version": "24.19.0", "binary": binary,
                "license": {"bytes": len(license_bytes), "sha256": hashlib.sha256(license_bytes).hexdigest()},
                "archive": archive_pin("node-archive", "https://nodejs.org/dist/v24.19.0/node-v24.19.0-linux-x64.tar.xz"),
            },
            "zowe": {
                "version": "8.39.0",
                "archive": archive_pin("zowe-archive", "https://registry.npmjs.org/@zowe/cli/-/cli-8.39.0.tgz"),
                "tree": {"sha256": TREE_GOLDEN, "files": 3, "bytes": 52, "max_file_bytes": 39, "max_depth": 3, "modes": [420, 493]},
            },
            "bubblewrap": {"version": "0.12.0", "mode": 0o755, **pin("bubblewrap", b"inert bwrap\n", 0o755)},
            "libraries": {role: pin(role, (role + "\n").encode()) for role in LIBRARIES},
        }
        self.repin_zowe()
        self.lock = {"development_profiles": {PROFILE: self.profile}}

    def repin_zowe(self):
        data = self.files["zowe-archive"].read_bytes()
        self.profile["zowe"]["archive"].update(
            bytes=len(data), sha256=hashlib.sha256(data).hexdigest(),
            integrity="sha512-" + base64.b64encode(hashlib.sha512(data).digest()).decode(),
        )

    def check(self):
        with mock.patch.object(supply_chain.subprocess, "check_output", side_effect=AssertionError("no native execution")), mock.patch.object(supply_chain.urllib.request, "urlopen", side_effect=AssertionError("no network")):
            return supply_chain.validate_development_inputs(self.lock, PROFILE, self.files, self.tree)

    def rejects(self):
        with self.assertRaises(supply_chain.SupplyChainError):
            self.check()

    def test_independent_archive_tree_golden_and_empty_leaf(self):
        result = self.check()
        self.assertEqual(result["identities"]["tree"], {"sha256": TREE_GOLDEN, "files": 3, "bytes": 52})
        self.assertEqual(result["files"], self.files)
        self.assertEqual(result["tree"], self.tree)
        self.assertEqual((self.tree / "package/empty").read_bytes(), b"")

    def test_retained_v1_and_deliberate_v2_dispatch(self):
        lock = json.loads((ROOT / supply_chain.CI_LOCK_PATH).read_text())
        lock.pop("development_profiles", None)
        lock["schema_version"] = "mainframe-env.ci-input-lock@1"
        (self.root / "tools").mkdir()
        path = self.root / supply_chain.CI_LOCK_PATH
        path.write_text(json.dumps(lock))
        self.assertEqual(supply_chain.validate_ci_lock(self.root), lock)
        lock["development_profiles"] = {PROFILE: self.profile}
        path.write_text(json.dumps(lock))
        with self.assertRaises(supply_chain.SupplyChainError):
            supply_chain.validate_ci_lock(self.root)
        lock["schema_version"] = "mainframe-env.ci-input-lock@2"
        path.write_text(json.dumps(lock))
        self.assertEqual(supply_chain.validate_ci_lock(self.root), lock)
        for mutation in ("unknown", "missing-tool", "extra-tool"):
            altered = copy.deepcopy(lock)
            if mutation == "unknown": altered["schema_version"] = "mainframe-env.ci-input-lock@3"
            if mutation == "missing-tool": del altered["tools"]["git"]
            if mutation == "extra-tool": altered["tools"]["node"] = {"version": "24.19.0"}
            path.write_text(json.dumps(altered))
            with self.subTest(mutation=mutation), self.assertRaises(supply_chain.SupplyChainError):
                supply_chain.validate_ci_lock(self.root)

    def test_duplicate_json_keys_are_rejected_at_every_depth(self):
        path = self.root / "duplicates.json"
        for data in ('{"a":1,"a":2}', '{"a":{"x":1,"x":1}}'):
            path.write_text(data)
            with self.subTest(data=data), self.assertRaises(supply_chain.SupplyChainError):
                supply_chain.load_json(path)

    def test_profile_grammar_refuses_unknown_fields_bad_scalars_and_caps(self):
        for section, key, value in (
            (self.profile, "argv", ["--ro-bind", "/", "/"]),
            (self.profile["node"], "version", "latest"),
            (self.profile["node"]["archive"], "url", "https://other.example/archive"),
            (self.profile["zowe"]["tree"], "files", True),
            (self.profile["zowe"]["tree"], "max_depth", 13),
            (self.profile["zowe"]["tree"], "sha256", "A" * 64),
            (self.profile["zowe"]["archive"], "integrity", "sha512-AAAA"),
        ):
            existed = key in section
            old = section.get(key)
            section[key] = value
            with self.subTest(key=key, value=value): self.rejects()
            if existed: section[key] = old
            else: del section[key]

    def test_platform_and_profile_selection_refuse_without_execution(self):
        with mock.patch.object(supply_chain.sys, "platform", "darwin"): self.rejects()
        with mock.patch.object(supply_chain.platform, "machine", return_value="aarch64"): self.rejects()
        with self.assertRaises(supply_chain.SupplyChainError):
            supply_chain.validate_development_inputs(self.lock, "unknown", self.files, self.tree)

    def test_mode_grammar_requires_two_exact_integers(self):
        for modes in (
            [420.0, 493], [420, 493.0], [420.0, 493.0],
            [True, 493], [420, False], [420], [420, 493, 493], (420, 493),
        ):
            self.profile["zowe"]["tree"]["modes"] = modes
            with self.subTest(modes=modes), self.assertRaisesRegex(supply_chain.SupplyChainError, "Zowe tree modes"):
                supply_chain.validate_development_profile(self.profile)
        self.profile["zowe"]["tree"]["modes"] = [0o644, 0o755]
        supply_chain.validate_development_profile(self.profile)

    def test_missing_extra_relative_and_role_substitution_bindings(self):
        saved = dict(self.files)
        del self.files["libm"]
        self.rejects()
        self.files = {**saved, "argv": self.root / "node"}
        self.rejects()
        self.files = {**saved, "node": Path("node")}
        self.rejects()
        self.files = {**saved, "node": saved["bubblewrap"]}
        self.rejects()

    def test_symlink_special_unresolved_and_control_character_files(self):
        original = self.files["node"]
        symlink = self.root / "link"
        symlink.symlink_to(original)
        fifo = self.root / "fifo"
        os.mkfifo(fifo)
        for path in (symlink, fifo, self.root / "tree", self.root / "missing", self.root / "tree/../node", self.root / "bad\nname"):
            self.files["node"] = path
            with self.subTest(path=str(path)): self.rejects()

    def test_byte_mode_privilege_and_untrusted_write_drift(self):
        node = self.files["node"]
        for data in (b"changed!!!\n", b"longer changed bytes\n"):
            node.write_bytes(data)
            self.rejects()
        node.write_bytes(b"inert node\n")
        for mode in (0o644, 0o777, 0o4755, 0o2755, 0o1755):
            node.chmod(mode)
            with self.subTest(mode=mode): self.rejects()

    def test_file_capabilities_are_refused(self):
        with mock.patch.object(supply_chain.os, "getxattr", return_value=b"elevated"):
            self.rejects()

    def test_selected_archive_hash_and_sri_are_independent(self):
        archive = self.files["zowe-archive"]
        original = archive.read_bytes()
        archive.write_bytes(original[:-1] + bytes([original[-1] ^ 1]))
        self.rejects()
        archive.write_bytes(original)
        self.profile["zowe"]["archive"]["integrity"] = "sha512-" + base64.b64encode(b"x" * 64).decode()
        self.rejects()

    def test_node_selected_members_bytes_mode_duplicates_and_paths(self):
        for member in (
            (self.node_members[0][0], b"different!\n", 0o755, tarfile.REGTYPE),
            (self.node_members[0][0], b"inert node\n", 0o644, tarfile.REGTYPE),
            ("node-v24.19.0-linux-x64/../outside", b"", 0o644, tarfile.REGTYPE),
            (self.node_members[0][0], b"LICENSE", 0o755, tarfile.SYMTYPE),
        ):
            inert_tar(self.files["node-archive"], [member, self.node_members[1]], "xz")
            self.profile["node"]["archive"].update(bytes=self.files["node-archive"].stat().st_size, sha256=digest(self.files["node-archive"]))
            with self.subTest(member=member[0:1]): self.rejects()
        inert_tar(self.files["node-archive"], self.node_members + [self.node_members[0]], "xz")
        self.profile["node"]["archive"].update(bytes=self.files["node-archive"].stat().st_size, sha256=digest(self.files["node-archive"]))
        self.rejects()

    def test_zowe_member_paths_types_duplicates_and_file_ancestors(self):
        for name, kind, data in (
            ("/absolute", tarfile.REGTYPE, b""), ("package/../outside", tarfile.REGTYPE, b""),
            ("package//double", tarfile.REGTYPE, b""), ("package/./dot", tarfile.REGTYPE, b""),
            ("package/control\x7f", tarfile.REGTYPE, b""), ("other/file", tarfile.REGTYPE, b""),
            ("package/link", tarfile.SYMTYPE, b"package/empty"), ("package/hard", tarfile.LNKTYPE, b"package/empty"),
            ("package/fifo", tarfile.FIFOTYPE, b""), ("package/empty/child", tarfile.REGTYPE, b""),
            ("package/empty", tarfile.REGTYPE, b""),
        ):
            inert_tar(self.files["zowe-archive"], self.zowe_members + [(name, data, 0o644, kind)], "gz")
            self.repin_zowe()
            self.profile["zowe"]["tree"].update(files=4, max_depth=4)
            with self.subTest(name=name): self.rejects()

    def test_archive_count_payload_member_and_depth_bounds(self):
        for key, value in (("files", 2), ("bytes", 51), ("max_file_bytes", 38), ("max_depth", 2)):
            old = self.profile["zowe"]["tree"][key]
            self.profile["zowe"]["tree"][key] = value
            with self.subTest(key=key): self.rejects()
            self.profile["zowe"]["tree"][key] = old

    def test_tree_missing_extra_mode_content_links_and_empty_directories(self):
        leaf = self.tree / "package/empty"
        leaf.unlink()
        self.rejects()
        leaf.write_bytes(b"")
        leaf.chmod(0o644)
        for mutate, restore in (
            (lambda: leaf.chmod(0o755), lambda: leaf.chmod(0o644)),
            (lambda: leaf.write_bytes(b"x"), lambda: leaf.write_bytes(b"")),
            (lambda: (self.tree / "extra").write_bytes(b""), lambda: (self.tree / "extra").unlink()),
            (lambda: (self.tree / "empty-dir").mkdir(), lambda: (self.tree / "empty-dir").rmdir()),
            (lambda: (self.tree / "link").symlink_to(leaf), lambda: (self.tree / "link").unlink()),
        ):
            mutate()
            self.rejects()
            restore()

    def test_manifest_identity_and_tree_digest_remain_independent(self):
        self.profile["zowe"]["tree"]["sha256"] = "0" * 64
        self.rejects()
        self.profile["zowe"]["tree"]["sha256"] = TREE_GOLDEN
        members = [(name, b'{"name":"other","version":"8.39.0"}' if name == "package/package.json" else data, mode, kind) for name, data, mode, kind in self.zowe_members]
        inert_tar(self.files["zowe-archive"], members, "gz")
        self.repin_zowe()
        self.rejects()

    def test_explicit_cli_bindings_and_no_profile_fallback(self):
        args = supply_chain.parser().parse_args(["check", "--development-profile", PROFILE, "--profile-file", "node=/tmp/file=part", "--profile-tree", str(self.tree)])
        self.assertEqual(args.profile_file, ["node=/tmp/file=part"])
        for bindings in (["node=/tmp/a", "node=/tmp/a"], ["bad=/tmp/a"], ["node=relative"], ["not-a-binding"]):
            with self.subTest(bindings=bindings), self.assertRaises(supply_chain.SupplyChainError):
                supply_chain.profile_bindings(bindings)
        with mock.patch.object(supply_chain, "check_repository", return_value=(self.lock, {"plugins": []})), mock.patch.object(supply_chain, "validate_development_inputs", side_effect=AssertionError("implicit profile")):
            self.assertEqual(supply_chain.main(["check"]), 0)
            self.assertEqual(supply_chain.main(["check", "--profile-tree", str(self.tree)]), 1)

    def test_existing_runtime_selection_never_adds_optional_inputs(self):
        lock = json.loads((ROOT / supply_chain.CI_LOCK_PATH).read_text())
        values = {"git": "git version 2.50.1", "cargo": "cargo-deny 0.20.2", "java": 'openjdk version "21.0.12.1"', "postgres": "postgres (PostgreSQL) 18.6", "gh": "gh version 2.92.0"}
        def output(argv, **kwargs):
            if argv[:2] == ["cargo", "fuzz"]: return "cargo-fuzz 0.13.2"
            if argv[:2] == ["cargo", "llvm-cov"]: return "cargo-llvm-cov 0.9.1"
            return values[argv[0]]
        with mock.patch.object(supply_chain, "command_output", side_effect=output) as called, mock.patch.object(supply_chain, "verify_rust"), mock.patch.object(supply_chain, "verify_active_rust"), mock.patch.object(supply_chain, "java_executable", return_value="java"):
            supply_chain.verify_runtime(lock, "all")
            self.assertEqual({args[0][0] for args, _ in called.call_args_list}, {"git", "cargo", "java", "postgres", "gh"})

    def test_node_non_directory_ancestor_is_rejected(self):
        members = self.node_members + [("node-v24.19.0-linux-x64/bin", b"x", 0o644, tarfile.REGTYPE)]
        inert_tar(self.files["node-archive"], members, "xz")
        self.profile["node"]["archive"].update(bytes=self.files["node-archive"].stat().st_size, sha256=digest(self.files["node-archive"]))
        self.rejects()

    def test_node_member_count_payload_member_size_and_license_bounds(self):
        for constant, value in (("NODE_MEMBERS", 1), ("NODE_PAYLOAD", 16), ("NODE_MEMBER_BYTES", 10)):
            with self.subTest(constant=constant), mock.patch.object(supply_chain, constant, value):
                self.rejects()
        inert_tar(self.files["node-archive"], [self.node_members[0]], "xz")
        self.profile["node"]["archive"].update(bytes=self.files["node-archive"].stat().st_size, sha256=digest(self.files["node-archive"]))
        self.rejects()

    def test_hash_consistent_wrong_manifest_identity_is_rejected(self):
        name = "package/package.json"
        self.payloads[name] = b'{"name":"not-zowe","version":"8.39.0"}'
        (self.tree / name).write_bytes(self.payloads[name])
        members = [(key, data, 0o644, tarfile.REGTYPE) for key, data in self.payloads.items()]
        inert_tar(self.files["zowe-archive"], members, "gz")
        self.repin_zowe()
        independent = [{"path": key, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(), "mode": 420} for key, data in sorted(self.payloads.items())]
        self.profile["zowe"]["tree"].update(bytes=sum(len(data) for data in self.payloads.values()), sha256=hashlib.sha256(json.dumps(independent, sort_keys=True, separators=(",", ":")).encode()).hexdigest())
        with self.assertRaisesRegex(supply_chain.SupplyChainError, "package identity"):
            self.check()

    def test_changed_input_during_read_is_rejected(self):
        original = supply_chain.stream_hash
        node = self.files["node"]
        def hash_then_change(source, maximum):
            value = original(source, maximum)
            if os.fstat(source.fileno()).st_ino == node.stat().st_ino:
                node.write_bytes(b"other node\n")
            return value
        with mock.patch.object(supply_chain, "stream_hash", side_effect=hash_then_change), self.assertRaisesRegex(supply_chain.SupplyChainError, "changed during read"):
            self.check()

    def test_extra_directory_archive_modes_and_directory_bound(self):
        members = self.zowe_members + [("package/unused", b"", 0o755, tarfile.DIRTYPE)]
        inert_tar(self.files["zowe-archive"], members, "gz")
        self.repin_zowe()
        self.rejects()
        inert_tar(self.files["zowe-archive"], self.zowe_members, "gz")
        self.repin_zowe()
        with mock.patch.object(supply_chain, "ZOWE_DIRECTORIES", 1): self.rejects()
        members = [(name, data, 0o777 if name == "package/empty" else mode, kind) for name, data, mode, kind in self.zowe_members]
        inert_tar(self.files["zowe-archive"], members, "gz")
        self.repin_zowe()
        self.rejects()

    def test_cli_selected_check_is_read_only_and_returns_fixed_source_roles(self):
        node_with_equals = self.root / "node=literal"
        node_with_equals.write_bytes(self.files["node"].read_bytes())
        node_with_equals.chmod(0o755)
        self.files["node"] = node_with_equals
        args = ["check", "--development-profile", PROFILE, "--profile-tree", str(self.tree)]
        for role, path in self.files.items(): args += ["--profile-file", role + "=" + str(path)]
        before = {str(path): (path.read_bytes(), path.stat().st_mode) for path in self.files.values()}
        with mock.patch.object(supply_chain, "check_repository", return_value=(self.lock, {"plugins": []})), mock.patch.object(supply_chain, "verify_runtime", side_effect=AssertionError("implicit native runtime")), mock.patch.object(supply_chain.tarfile.TarFile, "extractall", side_effect=AssertionError("no extraction")), mock.patch.object(supply_chain.subprocess, "check_output", side_effect=AssertionError("no execution")), mock.patch.object(supply_chain.urllib.request, "urlopen", side_effect=AssertionError("no network")), mock.patch("sys.stdout", new_callable=io.StringIO):
            self.assertEqual(supply_chain.main(args), 0)
        after = {str(path): (path.read_bytes(), path.stat().st_mode) for path in self.files.values()}
        self.assertEqual(before, after)
        with self.assertRaisesRegex(supply_chain.SupplyChainError, "duplicate"):
            supply_chain.profile_bindings(["node=" + str(node_with_equals)] * 2)
        self.profile["libraries"]["loader"]["destination"] = "/"
        self.rejects()

    def test_archive_extended_metadata_has_a_finite_bound(self):
        path = self.files["zowe-archive"]
        with tarfile.open(path, "w:gz", format=tarfile.PAX_FORMAT) as archive_file:
            info = tarfile.TarInfo("package/empty")
            info.pax_headers = {"comment": "x" * 32768}
            archive_file.addfile(info, io.BytesIO(b""))
        self.repin_zowe()
        with mock.patch.object(supply_chain, "MAX_JSON_BYTES", 16384), self.assertRaisesRegex(supply_chain.SupplyChainError, "extended metadata"):
            self.check()

    def test_retained_path_component_order_differs_from_string_order(self):
        self.payloads.update({"package/collide/file": b"x", "package/collide.txt": b"y"})
        for name in ("package/collide/file", "package/collide.txt"):
            path = self.tree / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(self.payloads[name])
            path.chmod(0o644)
        inert_tar(self.files["zowe-archive"], [(name, data, 0o644, tarfile.REGTYPE) for name, data in self.payloads.items()], "gz")
        self.repin_zowe()
        self.profile["zowe"]["tree"].update(files=5, bytes=54, sha256="7e14367156ac13e5f1812bf663fb76b7a6c7e466c1b98293b16e82e92acb8035")
        self.assertEqual(self.check()["identities"]["tree"]["sha256"], "7e14367156ac13e5f1812bf663fb76b7a6c7e466c1b98293b16e82e92acb8035")


if __name__ == "__main__":
    unittest.main()
