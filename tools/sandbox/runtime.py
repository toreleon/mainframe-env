"""Own child processes, application generations and bounded compiler invocations."""
from __future__ import annotations

import http.client
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import threading
import time

from .instance import Instance

MAX_OUTPUT = 2 * 1024 * 1024
COMPILER_TIMEOUT = 30
MAX_STARTUP_LOG = 8192


def stop_process(process: subprocess.Popen) -> None:
    if process.poll() is not None:
        return
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=5)


def request(port: int, method: str, path: str, body: bytes | None = None,
            headers: dict | None = None, timeout: float = 5) -> tuple[int, dict, bytes]:
    connection = http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
    try:
        connection.request(method, path, body, headers or {})
        response = connection.getresponse()
        payload = response.read(MAX_OUTPUT + 1)
        if len(payload) > MAX_OUTPUT:
            raise ValueError("application response exceeds byte limit")
        return response.status, dict(response.getheaders()), payload
    finally:
        connection.close()


class Runtime:
    def __init__(self, instance: Instance, compiler: Path, runner: Path,
                 reference: Path | None, inventory: Path):
        self.instance = instance
        self.compiler = compiler.resolve()
        self.runner = runner.resolve()
        self.reference = reference.resolve() if reference else None
        self.inventory = inventory.resolve()
        self.process = None
        self.port = None
        self.sessions = {}
        self.lock = threading.RLock()
        self.compiler_slots = threading.BoundedSemaphore(2)
        self.processes_lock = threading.Lock()
        self.compilers = set()
        self.closing = threading.Event()
        self.require_envd = os.environ.get("MAINFRAME_SANDBOX_REQUIRE_ENVD") == "1"
        if not self.compiler.is_file():
            raise ValueError("compiler binary is missing; run setup first")
        if instance.state["profile"] == "carddemo-online":
            if not self.runner.is_file() or self.reference is None or not self.inventory.is_file():
                raise ValueError("CardDemo runtime, reference or inventory is missing")

    def start(self) -> None:
        if self.instance.state["profile"] == "cobol":
            return
        identity = self.instance.state["active"]
        if identity is None:
            identity = self.instance.snapshot()
            process, port = self.launch(identity)
            try:
                self.instance.select(identity)
            except BaseException:
                stop_process(process)
                raise
            self.process, self.port = process, port
        else:
            self.process, self.port = self.launch(identity)

    def launch(self, identity: str) -> tuple[subprocess.Popen, int]:
        directory = self.instance.generation(identity)
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        with (directory / "application.log").open("ab") as log:
            process = subprocess.Popen([
                str(self.runner), "--reference", str(self.reference),
                "--source", str(directory / "source"), "--inventory", str(self.inventory),
                "--state-dir", str(directory / "data"), "--listen", f"127.0.0.1:{port}",
            ], stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
        try:
            deadline = time.monotonic() + 60
            while time.monotonic() < deadline:
                if self.closing.is_set():
                    raise ValueError("controller is stopping")
                if process.poll() is not None:
                    with (directory / "application.log").open("rb") as log:
                        log.seek(0, os.SEEK_END)
                        log.seek(max(0, log.tell() - MAX_STARTUP_LOG))
                        detail = log.read(MAX_STARTUP_LOG).decode("utf-8", "replace")
                    raise ValueError(f"application startup failed: {detail}")
                try:
                    status, _, payload = request(port, "GET", "/readyz", timeout=1)
                    if status == 200 and json.loads(payload).get("ready") is True:
                        return process, port
                except (OSError, ValueError, http.client.HTTPException):
                    pass
                time.sleep(0.1)
            raise ValueError("application readiness timed out")
        except BaseException:
            stop_process(process)
            raise

    def ready(self) -> bool:
        if self.require_envd:
            try:
                if request(49983, "GET", "/health", timeout=1)[0] != 204:
                    return False
            except (OSError, ValueError, http.client.HTTPException):
                return False
        if self.instance.state["profile"] == "cobol":
            return self.compiler.is_file()
        if self.process is None or self.process.poll() is not None:
            return False
        try:
            status, _, payload = request(self.port, "GET", "/readyz", timeout=1)
            return status == 200 and json.loads(payload).get("ready") is True
        except (OSError, ValueError, http.client.HTTPException):
            return False

    def activate(self, identity: str) -> dict:
        with self.lock:
            if identity == self.instance.state["active"]:
                return {"generation": identity, "replayed": True}
            process, port = self.launch(identity)
            try:
                self.instance.select(identity)
            except BaseException:
                stop_process(process)
                raise
            previous = self.process
            self.process, self.port = process, port
            self.sessions.clear()
            if previous is not None:
                stop_process(previous)
            return {"generation": identity, "retained_previous": True}

    def deploy(self, reset: bool = False) -> dict:
        if self.instance.state["profile"] != "carddemo-online":
            raise ValueError("deployment requires the carddemo-online profile")
        with self.lock:
            source = self.instance.generation(self.instance.state["active"]) / "source" if reset else None
            return {**self.activate(self.instance.snapshot(source)), "fresh_seed_data": True}

    def compile(self, operation: str, args: dict) -> dict:
        path = self.instance.source_path(args["path"])
        if not path.is_file() or path.stat().st_size > 1024 * 1024:
            raise ValueError("source is missing or exceeds byte limit")
        default_format = "fixed" if self.instance.state["profile"] == "carddemo-online" else "free"
        command = [str(self.compiler), operation, str(path), "--json", "--format", args.get("format", default_format)]
        libraries = args.get("libraries")
        if libraries is None:
            libraries = [p for p in ("app/cpy", "app/cpy-bms") if (self.instance.workspace / p).is_dir()]
        for library in libraries:
            command += ["--library", str(self.instance.source_path(library))]
        with self.compiler_slots, tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
            with self.processes_lock:
                if self.closing.is_set():
                    raise ValueError("controller is stopping")
                process = subprocess.Popen(command, cwd=self.instance.workspace, stdin=subprocess.DEVNULL,
                                           stdout=output, stderr=errors, start_new_session=True)
                self.compilers.add(process)
            try:
                deadline = time.monotonic() + COMPILER_TIMEOUT
                while process.poll() is None:
                    if time.monotonic() >= deadline:
                        raise ValueError(f"compiler/execution wall time exceeded {COMPILER_TIMEOUT:g} seconds")
                    if os.fstat(output.fileno()).st_size + os.fstat(errors.fileno()).st_size > MAX_OUTPUT:
                        raise ValueError("compiler/execution output limit exceeded")
                    try:
                        process.wait(timeout=min(0.05, max(0, deadline - time.monotonic())))
                    except subprocess.TimeoutExpired:
                        pass
                if os.fstat(output.fileno()).st_size + os.fstat(errors.fileno()).st_size > MAX_OUTPUT:
                    raise ValueError("compiler/execution output limit exceeded")
                output.seek(0)
                payload = output.read(MAX_OUTPUT + 1)
                if len(payload) > MAX_OUTPUT:
                    raise ValueError("compiler/execution output limit exceeded")
                try:
                    result = json.loads(payload)
                except ValueError as error:
                    raise ValueError("compiler did not return structured output") from error
                if not isinstance(result, dict):
                    raise ValueError("compiler output must be an object")
                if process.returncode != 0 or result.get("ok") is not True:
                    raise ValueError(result.get("error", "compiler rejected source"))
                return result
            finally:
                stop_process(process)
                with self.processes_lock:
                    self.compilers.discard(process)

    def close(self) -> None:
        self.closing.set()
        with self.processes_lock:
            for process in self.compilers:
                stop_process(process)
        with self.lock:
            if self.process is not None:
                stop_process(self.process)
                self.process = None
