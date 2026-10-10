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
    def test_archive_name_refuses_control_boundaries_and_preserves_unicode(self):
        for character in ("\x00", "\x09", "\x1f", "\x7f", "\x80", "\x85", "\x9f"):
            with self.subTest(codepoint=ord(character)):
                with self.assertRaises(supply_chain.SupplyChainError):
                    supply_chain.archive_name("package/a" + character + "b", "package")
        for character in (" ", "~", "\xa0", "é", "界", "😀"):
            name = "package/a" + character + "b"
            with self.subTest(codepoint=ord(character)):
                self.assertEqual(supply_chain.archive_name(name, "package"), name)

    def test_archive_name_preserves_components_and_utf8_byte_boundaries(self):
        for name in ("package/.env", "package/..name", "package/a b/é",
                     "package/" + "x" * 4088, "package/" + "é" * 2044):
            with self.subTest(name=name):
                self.assertEqual(supply_chain.archive_name(name, "package"), name)
        self.assertEqual(supply_chain.archive_name("package/", "package", True), "package")
        for name in ("", "package", "/package/file", "package//file", "package/./file",
                     "package/../file", "package/file/", "other/file", "package/a\\b",
                     "package/a\x00b", "package/a\x7fb", "package/a\x80b",
                     "package/" + "x" * 4089, "package/" + "é" * 2045):
            with self.subTest(name=name):
                with self.assertRaises(supply_chain.SupplyChainError):
                    supply_chain.archive_name(name, "package")

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


class CommandInputValidationTests(unittest.TestCase):
    def setUp(self):
        self.fixture = DevelopmentInputTests()
        self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.files = self.fixture.files
        self.tree = self.fixture.tree
        self.lock = self.fixture.lock
        self.raw = json.dumps(self.lock).encode()
        self.addCleanup(mock.patch.stopall)
        mock.patch.object(supply_chain.subprocess, 'check_output', side_effect=AssertionError('no execution')).start()
        mock.patch.object(supply_chain.urllib.request, 'urlopen', side_effect=AssertionError('no network')).start()

    def session(self):
        return supply_chain._development_input_command(self.lock, PROFILE, self.files, self.tree, lock_bytes=self.raw)

    def post(self, post, **changes):
        return post(changes.get('lock', self.lock), changes.get('profile', PROFILE),
                    changes.get('files', self.files), changes.get('tree', self.tree),
                    lock_bytes=changes.get('raw', self.raw))

    def test_default_still_parses_every_validation_and_session_only_pre(self):
        with mock.patch.object(supply_chain.tarfile, 'open', wraps=supply_chain.tarfile.open) as opened:
            self.fixture.check(); self.fixture.check()
            self.assertEqual(opened.call_count, 4)
            opened.reset_mock()
            with self.session() as (before, post):
                after = self.post(post)
            self.assertEqual(opened.call_count, 2)
        self.assertEqual(before, after)
        self.assertEqual(after['identities']['tree'], {'sha256': TREE_GOLDEN, 'files': 3, 'bytes': 52})

    def test_post_is_single_use_and_context_bound(self):
        with self.session() as (_, post):
            self.post(post)
            with self.assertRaises(supply_chain.SupplyChainError): self.post(post)
        with self.session() as (_, expired): pass
        with self.assertRaises(supply_chain.SupplyChainError): self.post(expired)
        with self.session() as (_, fresh): self.post(fresh)

    def test_pre_lock_and_bindings_cannot_change_before_proof_mint(self):
        for kind in ('lock', 'binding'):
            with self.subTest(kind=kind):
                validator = supply_chain.validate_zowe_tree
                old = self.files['node']
                def changed(*args):
                    result = validator(*args)
                    if kind == 'lock':
                        self.lock['unexpected'] = True
                    else:
                        path = self.fixture.root / 'new-node'
                        path.write_bytes(old.read_bytes()); path.chmod(0o755)
                        self.files['node'] = path
                    return result
                try:
                    with mock.patch.object(supply_chain, 'validate_zowe_tree', side_effect=changed), \
                            self.assertRaisesRegex(supply_chain.SupplyChainError, 'PRE'):
                        with self.session(): self.fail('changed PRE minted a proof')
                finally:
                    self.lock.pop('unexpected', None)
                    self.files['node'] = old

    def test_returned_identity_cannot_supply_rows_or_forge_post(self):
        with self.session() as (before, post):
            before['identities']['tree'] = {'sha256': '0' * 64, 'files': 1, 'bytes': 0}
            after = self.post(post)
            self.assertEqual(after['identities']['tree']['sha256'], TREE_GOLDEN)
        with self.session() as (_, post):
            (self.tree / 'package/lib/main.js').write_bytes(b'changed-only\n')
            with self.assertRaises(supply_chain.SupplyChainError): self.post(post)

    def test_binding_profile_lock_and_raw_lock_rejections(self):
        for change in ('role', 'tree', 'profile', 'lock', 'raw'):
            with self.subTest(change=change), self.session() as (_, post):
                arguments = {}
                if change == 'role':
                    path = self.fixture.root / 'other-node'
                    path.write_bytes(self.files['node'].read_bytes()); path.chmod(0o755)
                    arguments['files'] = {**self.files, 'node': path}
                elif change == 'tree': arguments['tree'] = self.fixture.root
                elif change == 'profile': arguments['profile'] = 'other'
                elif change == 'raw': arguments['raw'] = self.raw + b' '
                else:
                    arguments['lock'] = copy.deepcopy(self.lock)
                    arguments['lock']['development_profiles'][PROFILE]['node']['version'] = '24.19.1'
                with self.assertRaises(supply_chain.SupplyChainError): self.post(post, **arguments)

    def test_current_archive_bytes_and_sri_are_read_again(self):
        for role in ('node-archive', 'zowe-archive'):
            path = self.files[role]; original = path.read_bytes()
            with self.subTest(role=role), self.session() as (_, post):
                path.write_bytes(original[:-1] + bytes([original[-1] ^ 1]))
                with self.assertRaises(supply_chain.SupplyChainError): self.post(post)
            path.write_bytes(original)
        # A wrong digest oracle must remain an independent SRI refusal after
        # successful PRE; unchanged bytes/sha256 cannot stand in for SHA-512.
        with self.session() as (_, post):
            original = supply_chain.stream_hash
            def wrong_sri(source, maximum):
                value = original(source, maximum)
                if os.fstat(source.fileno()).st_ino == self.files['zowe-archive'].stat().st_ino:
                    return value[0], b'x' * 64, value[2]
                return value
            with mock.patch.object(supply_chain, 'stream_hash', side_effect=wrong_sri), \
                    self.assertRaisesRegex(supply_chain.SupplyChainError, 'SRI'):
                self.post(post)

    def test_post_tree_membership_manifest_and_role_bytes_are_fresh(self):
        for kind in ('role', 'extra-directory', 'manifest'):
            with self.subTest(kind=kind), self.session() as (_, post):
                if kind == 'role':
                    path = self.files['libm']; original = path.read_bytes(); path.write_bytes(b'other\n')
                elif kind == 'manifest':
                    path = self.tree / 'package/package.json'; original = path.read_bytes(); path.write_bytes(b'{}')
                else:
                    path = self.tree / 'extra'; path.mkdir(); original = None
                with self.assertRaises(supply_chain.SupplyChainError): self.post(post)
                if original is None: path.rmdir()
                else: path.write_bytes(original)

    def test_both_archive_fences_survive_post_tree_and_close(self):
        for role in ('node-archive', 'zowe-archive'):
            for kind in ('bytes', 'replacement', 'mode'):
                path = self.files[role]; original = path.read_bytes(); mode = path.stat().st_mode & 0o7777
                with self.subTest(role=role, kind=kind), self.session() as (_, post):
                    validator = supply_chain.validate_zowe_tree
                    def mutate(*args):
                        result = validator(*args)
                        if kind == 'bytes': path.write_bytes(original[:-1] + bytes([original[-1] ^ 1]))
                        elif kind == 'replacement':
                            replacement = path.with_suffix('.replacement')
                            replacement.write_bytes(original); replacement.chmod(mode); replacement.replace(path)
                        else: path.chmod(0o600)
                        return result
                    with mock.patch.object(supply_chain, 'validate_zowe_tree', side_effect=mutate), \
                            self.assertRaisesRegex(supply_chain.SupplyChainError, 'changed during read'):
                        self.post(post)
                path.write_bytes(original); path.chmod(mode)

    def test_post_archive_mode_capabilities_and_read_refusal(self):
        for kind in ('mode', 'caps', 'owner', 'read'):
            path = self.files['node-archive']; mode = path.stat().st_mode & 0o7777
            with self.subTest(kind=kind), self.session() as (_, post):
                if kind == 'mode':
                    path.chmod(0o777)
                    with self.assertRaises(supply_chain.SupplyChainError): self.post(post)
                    path.chmod(mode)
                elif kind == 'caps':
                    with mock.patch.object(supply_chain.os, 'getxattr', return_value=b'capability'), \
                            self.assertRaises(supply_chain.SupplyChainError): self.post(post)
                elif kind == 'owner':
                    from types import SimpleNamespace
                    original = supply_chain.safe_input_mode
                    def foreign(metadata, context):
                        original(SimpleNamespace(st_uid=1234567, st_mode=metadata.st_mode), context)
                    with mock.patch.object(supply_chain, 'safe_input_mode', side_effect=foreign), \
                            self.assertRaisesRegex(supply_chain.SupplyChainError, 'untrusted'):
                        self.post(post)
                else:
                    with mock.patch.object(supply_chain.os, 'open', side_effect=PermissionError('read refused')), \
                            self.assertRaises(supply_chain.SupplyChainError): self.post(post)


class TreeDirectoryAuthorityTests(unittest.TestCase):
    def setUp(self):
        self.fixture = DevelopmentInputTests(); self.fixture.setUp()
        self.addCleanup(self.fixture.doCleanups)
        self.tree = self.fixture.tree
        self.raw = json.dumps(self.fixture.lock).encode()
        self.addCleanup(mock.patch.stopall)
        mock.patch.object(supply_chain.subprocess, 'Popen', side_effect=AssertionError('no child')).start()
        mock.patch.object(supply_chain.subprocess, 'check_output', side_effect=AssertionError('no execution')).start()
        mock.patch.object(supply_chain.urllib.request, 'urlopen', side_effect=AssertionError('no network')).start()
        mock.patch.object(supply_chain.tarfile.TarFile, 'extractall', side_effect=AssertionError('no extraction')).start()

    def command_pair(self, between=None):
        raw = json.dumps(self.fixture.lock).encode()
        with supply_chain._development_input_command(self.fixture.lock, PROFILE,
                self.fixture.files, self.tree, lock_bytes=raw) as (before, post):
            if between: between()
            after = post(self.fixture.lock, PROFILE, self.fixture.files, self.tree, lock_bytes=raw)
        self.assertEqual(before, after)
        return after

    def nested(self):
        # Three literal members, unchanged bytes; only the zero-byte member moves.
        (self.tree / 'package/empty').unlink()
        path = self.tree / 'package/lib/nested/empty'; path.parent.mkdir()
        path.write_bytes(b''); path.chmod(0o644)
        self.fixture.payloads = {
            'package/package.json': b'{"name":"@zowe/cli","version":"8.39.0"}',
            'package/lib/main.js': b'fixture-only\n', 'package/lib/nested/empty': b''}
        inert_tar(self.fixture.files['zowe-archive'],
            [(name, data, 0o644, tarfile.REGTYPE) for name, data in self.fixture.payloads.items()], 'gz')
        self.fixture.repin_zowe()
        self.fixture.profile['zowe']['tree'].update(max_depth=4,
            sha256='bbd64e4b8a483021d10100daec34b841d86d296178f39cdfe949ddaa037c3006')

    def directory_audit(self):
        from contextlib import contextmanager
        @contextmanager
        def audited():
            opened, live, peak = set(), set(), [0]
            original_open, original_close = os.open, os.close
            def opening(path, flags, *args, **kwargs):
                descriptor = original_open(path, flags, *args, **kwargs)
                if flags & os.O_DIRECTORY:
                    opened.add(descriptor); live.add(descriptor); peak[0] = max(peak[0], len(live))
                return descriptor
            def closing(descriptor):
                original_close(descriptor); live.discard(descriptor)
            try:
                with mock.patch.object(os, 'open', side_effect=opening), mock.patch.object(os, 'close', side_effect=closing):
                    yield peak
            finally:
                self.assertFalse(live)
                for descriptor in opened:
                    with self.assertRaises(OSError): os.fstat(descriptor)
                self.assertLessEqual(peak[0], self.fixture.profile['zowe']['tree']['max_depth'])
        return audited()

    def test_literal_three_file_identity_and_default_path_unchanged(self):
        original = supply_chain.canonical_input
        observed = []
        def canonical(path):
            if path.is_relative_to(self.tree): observed.append(path.relative_to(self.tree).as_posix())
            return original(path)
        with mock.patch.object(supply_chain, 'canonical_input', side_effect=canonical):
            default = self.fixture.check()
        self.assertEqual(len(observed), 5)
        with self.directory_audit() as peak:
            selected = self.command_pair()
        self.assertEqual(default, selected)
        self.assertEqual(selected['identities']['tree'], {'sha256': TREE_GOLDEN, 'files': 3, 'bytes': 52})
        self.assertEqual(peak[0], 3)

    def test_tree_hardlink_policy_is_unchanged(self):
        os.link(self.tree / 'package/empty', self.fixture.root / 'same-empty')
        self.assertEqual(self.fixture.check()['identities']['tree']['sha256'], TREE_GOLDEN)
        with self.directory_audit(): self.command_pair()

    def test_root_directory_and_leaf_symlinks_and_fifo_refuse(self):
        for kind in ('root-link', 'directory-link', 'leaf-link', 'fifo'):
            original_tree = self.tree
            if kind == 'root-link':
                link = self.fixture.root / 'tree-link'; link.symlink_to(self.tree, target_is_directory=True); self.tree = link
            elif kind == 'directory-link':
                path = self.tree / 'package/lib'; saved = self.fixture.root / 'saved-lib'
                path.rename(saved); path.symlink_to(saved, target_is_directory=True)
            else:
                path = self.tree / 'package/empty'; path.unlink()
                if kind == 'leaf-link': path.symlink_to(self.tree / 'package/package.json')
                else: os.mkfifo(path, 0o644)
            try:
                with self.subTest(kind=kind), self.directory_audit(), self.assertRaises((supply_chain.SupplyChainError, OSError)):
                    self.command_pair()
            finally:
                if kind == 'root-link': self.tree = original_tree; link.unlink()
                elif kind == 'directory-link': path.unlink(); saved.rename(path)
                else: path.unlink(); path.write_bytes(b''); path.chmod(0o644)

    def test_same_byte_ancestor_rebind_during_leaf_read_refuses(self):
        import shutil
        original = supply_chain.stream_hash
        path = self.tree / 'package/lib'
        leaf_inode = (path / 'main.js').stat().st_ino
        def rebind(source, maximum):
            result = original(source, maximum)
            try: descriptor = source.fileno()
            except (AttributeError, io.UnsupportedOperation): return result
            if os.fstat(descriptor).st_ino == leaf_inode:
                path.rename(self.fixture.root / 'saved-lib')
                shutil.copytree(self.fixture.root / 'saved-lib', path)
            return result
        with self.directory_audit(), mock.patch.object(supply_chain, 'stream_hash', side_effect=rebind), \
                self.assertRaises((supply_chain.SupplyChainError, OSError)):
            self.command_pair()

    def test_processed_intermediate_rebind_during_fresh_manifest_refuses(self):
        from contextlib import contextmanager
        import shutil
        self.nested()
        original = supply_chain.checked_input
        changed = False
        @contextmanager
        def manifest_change(path, pin):
            nonlocal changed
            with original(path, pin) as value:
                if path == self.tree / 'package/package.json' and not changed:
                    changed = True
                    nested = self.tree / 'package/lib/nested'
                    nested.rename(self.fixture.root / 'saved-nested')
                    shutil.copytree(self.fixture.root / 'saved-nested', nested)
                yield value
        with self.directory_audit(), mock.patch.object(supply_chain, 'checked_input', side_effect=manifest_change), \
                self.assertRaisesRegex(supply_chain.SupplyChainError, 'directory.*changed'):
            self.command_pair()
        self.assertTrue(changed)

    def test_root_mode_change_during_manifest_refuses(self):
        from contextlib import contextmanager
        original = supply_chain.checked_input
        @contextmanager
        def manifest_change(path, pin):
            with original(path, pin) as value:
                if path == self.tree / 'package/package.json': self.tree.chmod(0o777)
                yield value
        with self.directory_audit(), mock.patch.object(supply_chain, 'checked_input', side_effect=manifest_change), \
                self.assertRaises((supply_chain.SupplyChainError, OSError)):
            self.command_pair()

    def test_above_root_symlink_rebind_during_manifest_refuses(self):
        from contextlib import contextmanager
        original = supply_chain.checked_input
        base = self.fixture.root
        saved = base.with_name(base.name + '-saved-ancestor')
        changed = False
        @contextmanager
        def manifest_change(path, pin):
            nonlocal changed
            with original(path, pin) as value:
                if path == self.tree / 'package/package.json' and not changed:
                    changed = True; base.rename(saved); base.symlink_to(saved, target_is_directory=True)
                yield value
        try:
            with self.directory_audit(), mock.patch.object(supply_chain, 'checked_input', side_effect=manifest_change), \
                    self.assertRaisesRegex(supply_chain.SupplyChainError, 'profile input is unavailable'):
                self.command_pair()
        finally:
            if changed: base.unlink(); saved.rename(base)
        self.assertTrue(changed)

    def test_leaf_modes_and_capabilities_remain_checked(self):
        path = self.tree / 'package/empty'
        for mode in (0o755, 0o666, 0o1644):
            path.chmod(mode)
            try:
                with self.subTest(mode=mode), self.directory_audit(), self.assertRaises(supply_chain.SupplyChainError):
                    self.command_pair()
            finally: path.chmod(0o644)
        original = os.getxattr
        def capability(descriptor, name):
            if os.fstat(descriptor).st_ino == path.stat().st_ino: return b'elevated'
            return original(descriptor, name)
        with self.directory_audit(), mock.patch.object(os, 'getxattr', side_effect=capability), \
                self.assertRaises(supply_chain.SupplyChainError): self.command_pair()

    def test_leaf_read_failure_closes_ancestors(self):
        original = supply_chain.stream_hash
        inode = (self.tree / 'package/lib/main.js').stat().st_ino
        def unreadable(source, maximum):
            try: descriptor = source.fileno()
            except (AttributeError, io.UnsupportedOperation): return original(source, maximum)
            if os.fstat(descriptor).st_ino == inode:
                raise OSError('controlled leaf read failure')
            return original(source, maximum)
        with self.directory_audit(), mock.patch.object(supply_chain, 'stream_hash', side_effect=unreadable), \
                self.assertRaises(supply_chain.SupplyChainError): self.command_pair()

    def test_above_root_rebind_during_final_resolution_refuses(self):
        import shutil
        original = supply_chain.canonical_input
        base = self.fixture.root
        saved = base.with_name(base.name + '-saved-final')
        root_calls = 0
        rows = supply_chain.zowe_archive_rows(self.fixture.files['zowe-archive'], self.fixture.profile['zowe'])
        def canonical(path):
            nonlocal root_calls
            if path == self.tree:
                root_calls += 1
                if root_calls == 2:
                    base.rename(saved)
                    shutil.copytree(saved, base)
            return original(path)
        try:
            with self.directory_audit(), mock.patch.object(supply_chain, 'canonical_input', side_effect=canonical), \
                    self.assertRaisesRegex(supply_chain.SupplyChainError, 'directory changed during read'):
                supply_chain.validate_zowe_tree(self.tree, rows, self.fixture.profile['zowe'], True)
        finally:
            if saved.exists(): shutil.rmtree(base); saved.rename(base)
        self.assertEqual(root_calls, 2)

    def test_leaf_stream_construction_failure_closes_raw_descriptor(self):
        original = os.fdopen
        inode = (self.tree / 'package/lib/main.js').stat().st_ino
        failed = []
        def construction(descriptor, *args, **kwargs):
            if os.fstat(descriptor).st_ino == inode:
                failed.append(descriptor)
                raise OSError('controlled leaf stream construction failure')
            return original(descriptor, *args, **kwargs)
        try:
            with self.directory_audit(), mock.patch.object(os, 'fdopen', side_effect=construction), \
                    self.assertRaises(supply_chain.SupplyChainError): self.command_pair()
            self.assertEqual(len(failed), 1)
            with self.assertRaises(OSError): os.fstat(failed[0])
        finally:
            for descriptor in failed:
                try: os.fstat(descriptor)
                except OSError: pass
                else: os.close(descriptor)

    def test_depth_bound_and_membership_remain_finite(self):
        for change in ('depth', 'extra-file', 'extra-directory', 'missing'):
            if change == 'depth':
                saved_depth = self.fixture.profile['zowe']['tree']['max_depth']; self.fixture.profile['zowe']['tree']['max_depth'] = 2
            elif change == 'extra-file': (self.tree / 'package/extra').write_bytes(b'')
            elif change == 'extra-directory': (self.tree / 'package/extra').mkdir()
            else: (self.tree / 'package/empty').unlink()
            try:
                with self.subTest(change=change), self.directory_audit(), self.assertRaises(supply_chain.SupplyChainError):
                    self.command_pair()
            finally:
                if change == 'depth': self.fixture.profile['zowe']['tree']['max_depth'] = saved_depth
                elif change == 'extra-file': (self.tree / 'package/extra').unlink()
                elif change == 'extra-directory': (self.tree / 'package/extra').rmdir()
                else: (self.tree / 'package/empty').write_bytes(b''); (self.tree / 'package/empty').chmod(0o644)


if __name__ == "__main__":
    unittest.main()
