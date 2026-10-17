"""Retain actual native setup output independently of disposable Cargo artifacts."""
from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import uuid

from .instance import write_json


def digest(path: Path) -> str:
    if any(component.is_symlink() for component in (path, *path.parents)) or not path.is_file():
        raise ValueError(f"setup artifact must be a regular file: {path}")
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def source_state(root: Path) -> dict:
    command = ["git", "-C", str(root)]
    def output(*arguments):
        return subprocess.check_output(command + list(arguments))
    untracked = []
    for name in output("ls-files", "--others", "--exclude-standard", "-z").split(b"\0"):
        if name:
            relative = os.fsdecode(name)
            path = root / relative
            untracked.append({"path": relative, "sha256": digest(path)})
    return {
        "commit": output("rev-parse", "HEAD").decode("ascii").strip(),
        "tree": output("rev-parse", "HEAD^{tree}").decode("ascii").strip(),
        "status": output("status", "--porcelain", "--untracked-files=all").decode("utf-8"),
        "tracked_diff_sha256": hashlib.sha256(output("diff", "--binary", "HEAD")).hexdigest(),
        "untracked": untracked,
        "cargo_lock_sha256": digest(root / "Cargo.lock"),
    }


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


class Evidence:
    """A new receipt owns only its unique output directory and command logs."""
    def __init__(self, path: Path, root: Path, destination: Path, target: Path):
        path.mkdir(parents=True, exist_ok=True)
        self.path = path / "setup-receipt.json"
        self.output = path / ("setup-" + uuid.uuid4().hex)
        self.state = {"schema_version": "mainframe-sandbox.setup-receipt@1",
                      "started_at": now(), "finished_at": None, "status": "running",
                      "ready": False, "checkout": str(root), "bundle": str(destination),
                      "target_directory": str(target), "output_directory": str(self.output),
                      "commands": [], "artifacts": {}, "errors": []}
        try:
            descriptor = os.open(self.path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        except FileExistsError as error:
            raise ValueError("producer evidence already contains a setup receipt; choose a fresh directory") from error
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            json.dump(self.state, stream, indent=2)
            stream.write("\n")
        try:
            self.output.mkdir(mode=0o700)
        except OSError as error:
            self.state["errors"].append({"stage": "evidence-allocation", "error": str(error)})
            self.finish(False)
            raise

    def save(self):
        write_json(self.path, self.state)

    def start(self, root: Path):
        self.state["source_start"] = source_state(root)
        self.state["tools"] = {"python": sys.version, "python_executable": sys.executable}
        for name, command in (("cargo", ["cargo", "--version"]),
                              ("rustc", ["rustc", "--version", "--verbose"]),
                              ("git", ["git", "--version"])):
            self.state["tools"][name] = subprocess.check_output(command, text=True).strip()
        self.save()

    def run(self, name: str, command: list[str], **kwargs):
        log = self.output / (name + ".log")
        entry = {"name": name, "argv": command, "cwd": str(kwargs.get("cwd", Path.cwd())),
                 "started_at": now(), "finished_at": None, "returncode": None,
                 "log": str(log), "environment": {
                     key: kwargs.get("env", {}).get(key)
                     for key in ("CARGO_TARGET_DIR", "CARGO_NET_OFFLINE")}}
        self.state["commands"].append(entry)
        self.save()
        try:
            with log.open("xb") as stream:
                result = subprocess.run(command, stdout=stream, stderr=subprocess.STDOUT, **kwargs)
            entry["returncode"] = result.returncode
            return result
        except subprocess.CalledProcessError as error:
            entry["returncode"] = error.returncode
            raise
        except OSError as error:
            entry["error"] = str(error)
            raise
        finally:
            entry["finished_at"] = now()
            self.save()

    def retain(self, target: Path, destination: Path):
        artifacts = self.output / "artifacts"
        artifacts.mkdir(mode=0o700)
        candidates = [(name, target / "release" / name) for name in
                      ("mainframe-env", "mainframe-sandbox-runtime", "xtask")]
        if any(entry["name"] == "license-notices" for entry in self.state["commands"]):
            candidates.append(("THIRD-PARTY-NOTICES.md", destination / "THIRD-PARTY-NOTICES.md"))
        for name, source in candidates:
            if source.exists() or source.is_symlink():
                identity = digest(source)
                retained = artifacts / name
                shutil.copy2(source, retained, follow_symlinks=False)
                if digest(retained) != identity:
                    raise ValueError("retained setup artifact differs from its producer")
                self.state["artifacts"][name] = {"path": str(retained), "sha256": identity,
                                                 "bytes": retained.stat().st_size}
        self.save()

    def finish(self, ready: bool):
        self.state.update({"finished_at": now(), "status": "passed" if ready else "failed",
                           "ready": ready})
        self.save()
