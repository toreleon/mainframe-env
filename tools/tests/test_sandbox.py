"""Sandbox boundary regressions; real application acceptance lives in sandbox.verify."""
from __future__ import annotations

import base64
import http.client
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import select
import struct
import subprocess
import sys
import tempfile
import threading
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from tools.sandbox.client import Client
from tools.sandbox.instance import Instance, copy_tree, write_json
from tools.sandbox.operations import decode_fields, validate, Operations, TOOLS
from tools.sandbox.server import Server
from tools.sandbox.runtime import Runtime
from tools.sandbox.setup import prepare_reference, setup


class ReferenceSetupTests(unittest.TestCase):
    def test_fetch_is_shallow_pinned_and_rejects_reused_dirty_or_wrong_checkout(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            corpus = root / "corpus"
            subprocess.run(["git", "init", "--quiet", str(corpus)], check=True)
            command = ["git", "-C", str(corpus)]
            def commit(text):
                (corpus / "source").write_text(text)
                subprocess.run(command + ["add", "source"], check=True)
                subprocess.run(command + ["-c", "user.name=Sandbox test", "-c", "user.email=sandbox@example.invalid",
                                         "-c", "commit.gpgsign=false", "commit", "--quiet", "-m", text], check=True)
            commit("pinned")
            identity = {"repository": "https://example.invalid/reference.git",
                        "commit": subprocess.check_output(command + ["rev-parse", "HEAD"], text=True).strip(),
                        "tree": subprocess.check_output(command + ["rev-parse", "HEAD^{tree}"], text=True).strip()}
            commit("newer upstream")
            reference = root / "bundle/reference"
            prepare_reference(reference, corpus, identity)
            self.assertEqual((reference / "source").read_text(), "pinned")
            self.assertEqual(subprocess.check_output(["git", "-C", str(reference), "rev-parse", "--is-shallow-repository"], text=True).strip(), "true")
            prepare_reference(reference, None, identity)
            (reference / "source").write_text("dirty")
            with self.assertRaisesRegex(ValueError, "dirty"):
                prepare_reference(reference, None, identity)
            subprocess.run(["git", "-C", str(reference), "checkout", "--", "source"], check=True)
            with self.assertRaisesRegex(ValueError, "commit or tree"):
                prepare_reference(reference, None, {**identity, "tree": "0" * 40})
            rejected = root / "rejected/reference"
            with self.assertRaisesRegex(ValueError, "commit or tree"):
                prepare_reference(rejected, corpus, {**identity, "tree": "0" * 40})
            self.assertFalse(rejected.exists(), "invalid checkouts must not be published")
            self.assertEqual(list(rejected.parent.iterdir()), [])
            subprocess.run(["git", "-C", str(reference), "remote", "set-url", "origin", str(corpus)], check=True)
            with self.assertRaisesRegex(ValueError, "origin"):
                prepare_reference(reference, None, identity)

    def test_reference_symlinks_are_rejected_even_when_dangling(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            reference = root / "reference"
            reference.symlink_to(root / "missing", target_is_directory=True)
            with self.assertRaisesRegex(ValueError, "symlink"):
                prepare_reference(reference, None, {})
            self.assertTrue(reference.is_symlink())

    def test_failed_setup_cleans_only_checkout_target(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            checkout = root / "checkout"
            checkout.mkdir()
            shared = root / "shared-target"
            shared.mkdir()
            sentinel = shared / "keep"
            sentinel.write_text("unrelated artifacts")
            commands = []

            def run(command, **kwargs):
                commands.append((command, kwargs))
                if command[1] == "build":
                    raise subprocess.CalledProcessError(1, command)
                self.assertEqual(command, ["cargo", "clean"])
                self.assertEqual(kwargs["cwd"], checkout)
                self.assertEqual(kwargs["env"]["CARGO_TARGET_DIR"], str(checkout / "target"))

            with patch("tools.sandbox.setup.ROOT", checkout), patch("tools.sandbox.setup.subprocess.run", side_effect=run):
                with patch.dict("os.environ", {"CARGO_TARGET_DIR": str(shared)}):
                    with self.assertRaises(subprocess.CalledProcessError):
                        setup(root / "bundle", None, False)
            self.assertEqual(len(commands), 2)
            self.assertEqual(sentinel.read_text(), "unrelated artifacts")
            self.assertFalse(json.loads((root / "bundle/bundle.json").read_text())["ready"])


class CompilerProcessTests(unittest.TestCase):
    def runtime(self, root, script):
        instance = Instance(root / "instance", "cobol", None)
        compiler = root / "compiler"
        compiler.write_text("#!" + sys.executable + "\n" + script)
        compiler.chmod(0o700)
        return Runtime(instance, compiler, root / "runner", None, root / "inventory")

    def test_completed_compiler_stderr_still_counts_toward_output_limit(self):
        with tempfile.TemporaryDirectory() as temporary:
            runtime = self.runtime(Path(temporary), 'import sys\nsys.stderr.write("x" * 8192)\nprint(\'{"ok":true}\')\n')
            with patch("tools.sandbox.runtime.MAX_OUTPUT", 4096):
                with self.assertRaisesRegex(ValueError, "output limit"):
                    runtime.compile("run", {"path": "HELLO.cbl"})
            self.assertEqual(runtime.compilers, set())

    def test_hung_compiler_is_reaped_at_deadline(self):
        with tempfile.TemporaryDirectory() as temporary:
            runtime = self.runtime(Path(temporary), "import time\ntime.sleep(10)\n")
            with patch("tools.sandbox.runtime.COMPILER_TIMEOUT", 0.03):
                with self.assertRaisesRegex(ValueError, "wall time"):
                    runtime.compile("run", {"path": "HELLO.cbl"})
            self.assertEqual(runtime.compilers, set())

    def test_fast_compiler_returns_structured_result_without_polling_sleep(self):
        with tempfile.TemporaryDirectory() as temporary:
            runtime = self.runtime(Path(temporary), 'print(\'{"ok":true,"output":"HELLO"}\')\n')
            result = runtime.compile("run", {"path": "HELLO.cbl"})
            self.assertEqual(result, {"ok": True, "output": "HELLO"})
            self.assertEqual(runtime.compilers, set())

    def test_non_object_compiler_output_is_rejected_and_reaped(self):
        with tempfile.TemporaryDirectory() as temporary:
            runtime = self.runtime(Path(temporary), "print('[]')\n")
            with self.assertRaisesRegex(ValueError, "must be an object"):
                runtime.compile("run", {"path": "HELLO.cbl"})
            self.assertEqual(runtime.compilers, set())


class WorkspaceBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.instance = Instance(self.root / "instance", "cobol", None)

    def tearDown(self):
        self.temporary.cleanup()

    def test_unowned_directory_is_not_overwritten(self):
        path = self.root / "unowned"
        path.mkdir()
        (path / "keep").write_text("important")
        with self.assertRaises(ValueError):
            Instance(path, "cobol", None)
        self.assertEqual((path / "keep").read_text(), "important")

    def test_traversal_and_symlinks_are_rejected(self):
        for value in ("../outside", "/etc/passwd", ""):
            with self.assertRaises(ValueError):
                self.instance.source_path(value)
        (self.instance.workspace / "escape").symlink_to(self.root, target_is_directory=True)
        with self.assertRaises(ValueError):
            self.instance.source_path("escape/private")
        with self.assertRaises(ValueError):
            self.instance.snapshot()
        self.assertEqual(list(self.instance.generations.iterdir()), [])

    def test_deployed_source_is_independent_of_workspace_edits(self):
        generation = self.instance.snapshot()
        self.instance.select(generation)
        self.instance.source_path("HELLO.cbl").write_text("edited")
        deployed = self.instance.generation(generation) / "source/HELLO.cbl"
        self.assertIn("Hello from Mainframe Sandbox", deployed.read_text())
        reopened = Instance(self.instance.path, "cobol", None)
        self.assertEqual(reopened.state["active"], generation)

    def test_oversized_snapshot_leaves_no_generation(self):
        with patch("tools.sandbox.instance.MAX_TREE", 8):
            with self.assertRaises(ValueError):
                self.instance.snapshot()
        self.assertEqual(list(self.instance.generations.iterdir()), [])

    def test_directory_only_snapshot_is_bounded(self):
        self.instance.source_path("HELLO.cbl").unlink()
        for index in range(4):
            (self.instance.workspace / f"empty-{index}").mkdir()
        with patch("tools.sandbox.instance.MAX_ENTRIES", 3):
            with self.assertRaisesRegex(ValueError, "entry limit"):
                self.instance.snapshot()
        self.assertEqual(list(self.instance.generations.iterdir()), [])

    def test_generation_identity_cannot_escape_instance(self):
        with self.assertRaises(ValueError):
            self.instance.generation("../other")

    def test_connection_cannot_redirect_token_to_remote_host(self):
        for url in ("https://example.com", "http://user@127.0.0.1:8080",
                    "http://127.0.0.1:8080?redirect=other", "http://127.0.0.1:8080#fragment"):
            write_json(self.instance.path / "connection.json", {"url": url, "token": "secret"})
            with self.assertRaises(ValueError):
                Client(self.instance.path)

    def test_cube_readiness_requires_envd_and_profile_readiness(self):
        compiler = self.root / "compiler"
        compiler.write_text("placeholder")
        with patch.dict("os.environ", {"MAINFRAME_SANDBOX_REQUIRE_ENVD": "1"}):
            runtime = Runtime(self.instance, compiler, self.root / "runner", None, self.root / "inventory")
        with patch("tools.sandbox.runtime.request", return_value=(503, {}, b"")):
            self.assertFalse(runtime.ready())
        with patch("tools.sandbox.runtime.request", return_value=(204, {}, b"")):
            self.assertTrue(runtime.ready())
            compiler.unlink()
            self.assertFalse(runtime.ready())

    def test_clone_rekey_invalidates_old_client_without_exposing_new_token(self):
        runtime = SimpleNamespace(lock=threading.RLock(), instance=self.instance)
        server = Server(("127.0.0.1", 0), runtime, "old-controller-token")
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        write_json(self.instance.path / "connection.json", {
            "url": f"http://127.0.0.1:{server.server_address[1]}", "token": server.token})
        old = Client(self.instance.path)
        try:
            with self.assertRaises(ValueError):
                old.request("/sandbox/v1/rekey", {"token": "short"})
            result = old.request("/sandbox/v1/rekey", {"token": "fresh_" + "x" * 40})
            self.assertEqual(result, {"rekeyed": True})
            with self.assertRaises(ValueError):
                old.request("/sandbox/v1/tools")
            fresh = Client(self.instance.path)
            self.assertEqual(len(fresh.request("/sandbox/v1/tools")["tools"]), 7)
            self.assertEqual((self.instance.path / "connection.json").stat().st_mode & 0o777, 0o600)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)


class ProtocolBoundaryTests(unittest.TestCase):
    def test_profile_capabilities_do_not_depend_on_tool_order(self):
        runtime = SimpleNamespace(instance=SimpleNamespace(state={"profile": "cobol"}))
        with patch("tools.sandbox.operations.TOOLS", list(reversed(TOOLS))):
            names = {tool["name"] for tool in Operations(runtime).tools()}
        self.assertEqual(names, {"sandbox_status", "workspace_read", "workspace_write", "cobol_inspect",
                                 "cobol_compile", "cobol_run", "sandbox_generations"})

    def test_controller_redirects_do_not_forward_credentials(self):
        received = []

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                if self.path == "/redirect":
                    self.send_response(302)
                    self.send_header("Location", f"http://127.0.0.1:{self.server.server_port}/target")
                else:
                    received.append(self.headers.get("Authorization"))
                    self.send_response(200)
                self.end_headers()
                self.wfile.write(b"[]")

            def log_message(self, *args):
                pass

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with tempfile.TemporaryDirectory() as temporary:
                instance = Path(temporary)
                write_json(instance / "connection.json", {
                    "url": f"http://127.0.0.1:{server.server_port}", "token": "secret"})
                client = Client(instance)
                with self.assertRaisesRegex(ValueError, "redirects are forbidden"):
                    client.request("/redirect")
                self.assertEqual(received, [], "redirect destination must receive no request")
                with self.assertRaisesRegex(ValueError, "must be an object"):
                    client.request("/target")
                self.assertEqual(received, ["Bearer secret"])
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)

    def test_stdio_lifecycle_errors_and_profile_discovery(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            instance = Instance(root / "instance", "cobol", None)
            compiler = root / "compiler"
            compiler.touch()
            with patch.dict("os.environ", {"MAINFRAME_SANDBOX_REQUIRE_ENVD": "0"}):
                runtime = Runtime(instance, compiler, root / "runner", None, root / "inventory")
            server = Server(("127.0.0.1", 0), runtime, "private-token")
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            write_json(instance.path / "connection.json", {
                "url": f"http://127.0.0.1:{server.server_address[1]}", "token": server.token})
            executable = Path(__file__).resolve().parents[2] / "bin/mainframe-sandbox"
            process = subprocess.Popen([sys.executable, str(executable), "mcp", "--instance", str(instance.path)],
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            def exchange(message):
                process.stdin.write(message if isinstance(message, bytes) else json.dumps(message).encode() + b"\n")
                process.stdin.flush()
                self.assertTrue(select.select([process.stdout], [], [], 5)[0], "MCP response deadline")
                return json.loads(process.stdout.readline())
            try:
                self.assertEqual(exchange(b"{invalid\n")["error"]["code"], -32700)
                self.assertEqual(exchange({"jsonrpc": "2.0", "id": 0, "method": "tools/list"})["error"]["code"], -32600)
                initialized = exchange({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                    "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}})
                self.assertEqual(initialized["result"]["protocolVersion"], "2025-06-18")
                process.stdin.write(b'{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
                process.stdin.flush()
                tools = exchange({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})["result"]["tools"]
                self.assertNotIn("terminal_open", {tool["name"] for tool in tools})
                result = exchange({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                                   "params": {"name": "sandbox_status", "arguments": {}}})["result"]
                self.assertTrue(result["structuredContent"]["ready"])
                denied = exchange({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
                                   "params": {"name": "terminal_open", "arguments": {}}})
                self.assertEqual(denied["error"]["code"], -32602)
                self.assertEqual(exchange(b" " * (1024 * 1024 + 1))["error"]["code"], -32700)
                self.assertEqual(process.wait(timeout=3), 0)
                self.assertTrue(runtime.ready(), "adapter exit must leave the controller available")
            finally:
                if process.poll() is None:
                    process.terminate()
                    process.wait(timeout=3)
                for stream in (process.stdin, process.stdout, process.stderr):
                    stream.close()
                server.shutdown()
                server.server_close()
                thread.join(timeout=2)

    def test_terminal_decoder_rejects_truncation_and_duplicate_names(self):
        record = struct.pack(">I", 4) + b"NAME" + struct.pack(">I", 5) + b"value"
        self.assertEqual(decode_fields(base64.b64encode(record).decode()), {"NAME": "value"})
        for data in (record[:-1], record + record, b"\xff\xff\xff\xff"):
            with self.assertRaises(ValueError):
                decode_fields(base64.b64encode(data).decode())

    def test_tools_reject_unknown_fields_and_invalid_aids(self):
        schema = next(t["inputSchema"] for t in TOOLS if t["name"] == "terminal_send")
        for value in ({"session": "s", "aid": "SHELL"}, {"session": "s", "command": "rm"}, {"session": []}):
            with self.assertRaises(ValueError):
                validate(value, schema)

    def test_http_controller_authentication_origin_and_size(self):
        ready = [True]
        runtime = SimpleNamespace(ready=lambda: ready[0], instance=SimpleNamespace(state={"profile": "cobol"}))
        server = Server(("127.0.0.1", 0), runtime, "private-token")
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        def get(path, headers=None, method="GET", body=None):
            connection = http.client.HTTPConnection(*server.server_address, timeout=2)
            try:
                connection.request(method, path, body, headers or {})
                response = connection.getresponse()
                response.read()
                return response.status
            finally:
                connection.close()
        try:
            self.assertEqual(get("/readyz"), 200)
            ready[0] = False
            self.assertEqual(get("/readyz"), 503)
            self.assertEqual(get("/sandbox/v1/tools"), 401)
            headers = {"Authorization": "Bearer private-token"}
            self.assertEqual(get("/sandbox/v1/tools", headers), 200)
            self.assertEqual(get("/sandbox/v1/tools", {**headers, "Origin": "https://attacker.example"}), 403)
            self.assertEqual(get("/readyz", {"Host": "attacker.example"}), 403)
            self.assertEqual(get("/sandbox/v1/call", {**headers, "Content-Length": "1048577"}, "POST"), 400)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)


if __name__ == "__main__":
    unittest.main()
