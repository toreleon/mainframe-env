"""Bounded workspace snapshots and atomically selected application generations."""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shutil
import uuid

SCHEMA = "mainframe-sandbox.instance@1"
MAX_FILE = 16 * 1024 * 1024
MAX_TREE = 256 * 1024 * 1024
MAX_ENTRIES = 10000
GENERATION = re.compile(r"[a-f0-9]{32}\Z")


def write_json(path: Path, value: dict) -> None:
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}")
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as stream:
            json.dump(value, stream, indent=2)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def read_json(path: Path) -> dict:
    if path.is_symlink() or path.stat().st_size > 1024 * 1024:
        raise ValueError(f"invalid metadata: {path}")
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError("metadata must be an object")
    return value


def copy_tree(source: Path, destination: Path) -> None:
    """Copy regular files only, with bounded aggregate size and no symlink traversal."""
    if source.is_symlink() or not source.is_dir():
        raise ValueError("source must be a regular directory")
    count = total = 0
    destination.mkdir(mode=0o700)
    for root, directories, files in os.walk(source, followlinks=False):
        relative = Path(root).relative_to(source)
        for name in directories:
            count += 1
            if count > MAX_ENTRIES:
                raise ValueError("workspace snapshot exceeds its entry limit")
            if (Path(root) / name).is_symlink():
                raise ValueError("workspace contains a directory symlink")
            (destination / relative / name).mkdir(mode=0o700)
        for name in files:
            item = Path(root) / name
            if item.is_symlink() or not item.is_file():
                raise ValueError("workspace contains a non-regular file")
            size = item.stat().st_size
            count += 1
            total += size
            if size > MAX_FILE or total > MAX_TREE or count > MAX_ENTRIES:
                raise ValueError("workspace snapshot exceeds its entry or byte limit")
            shutil.copyfile(item, destination / relative / name, follow_symlinks=False)


class Instance:
    def __init__(self, path: Path, profile: str, reference: Path | None):
        if path.is_symlink():
            raise ValueError("instance directory cannot be a symlink")
        self.path = path.resolve()
        self.path.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.manifest = self.path / "instance.json"
        self.workspace = self.path / "workspace"
        self.generations = self.path / "generations"
        if self.manifest.exists():
            self.state = read_json(self.manifest)
            if self.state.get("schema_version") != SCHEMA or self.state.get("profile") != profile:
                raise ValueError("instance profile or schema differs")
        else:
            if any(self.path.iterdir()):
                raise ValueError("refusing to initialize a nonempty unowned directory")
            self.workspace.mkdir(mode=0o700)
            self.generations.mkdir(mode=0o700)
            if profile == "carddemo-online":
                if reference is None:
                    raise ValueError("CardDemo requires a verified reference checkout")
                copy_tree(reference / "app", self.workspace / "app")
            else:
                (self.workspace / "HELLO.cbl").write_text(
                    'identification division.\nprogram-id. HELLO.\nprocedure division.\n'
                    'display "Hello from Mainframe Sandbox".\nstop run.\n', encoding="utf-8")
            (self.workspace / "AGENTS.md").write_text(
                "# Mainframe Sandbox workspace\n\n"
                "Use mainframe-sandbox operations or MCP tools to compile and run sources.\n"
                "Edit files in this workspace. The reference checkout is immutable.\n"
                "CardDemo deploy starts fresh seed data and preserves the previous generation.\n"
                "Restart retains deployed code and application data; rollback selects a retained generation.\n"
                "Consult sandbox capabilities before using a subsystem.\n", encoding="utf-8")
            self.state = {"schema_version": SCHEMA, "profile": profile, "active": None}
            write_json(self.manifest, self.state)
        for item in (self.workspace, self.generations):
            if item.is_symlink() or not item.is_dir():
                raise ValueError("invalid instance directory layout")

    def generation(self, identity: str) -> Path:
        if not isinstance(identity, str) or not GENERATION.fullmatch(identity):
            raise ValueError("invalid generation identity")
        path = self.generations / identity
        if path.is_symlink():
            raise ValueError("generation cannot be a symlink")
        return path

    def snapshot(self, source: Path | None = None) -> str:
        if len(list(self.generations.iterdir())) >= 32:
            raise ValueError("retained generation limit reached; export data and use a fresh instance")
        identity = uuid.uuid4().hex
        path = self.generation(identity)
        path.mkdir(mode=0o700)
        try:
            copy_tree(source or self.workspace, path / "source")
            write_json(path / "generation.json", {"identity": identity, "profile": self.state["profile"]})
        except BaseException:
            shutil.rmtree(path)
            raise
        return identity

    def select(self, identity: str) -> None:
        path = self.generation(identity)
        if not (path / "generation.json").is_file():
            raise ValueError("generation does not exist")
        state = {**self.state, "active": identity}
        write_json(self.manifest, state)
        self.state = state

    def source_path(self, relative: str) -> Path:
        path = Path(relative)
        if not relative or path.is_absolute() or ".." in path.parts:
            raise ValueError("source path must be workspace-relative")
        result = self.workspace / path
        current = result
        while current != self.workspace:
            if current.is_symlink():
                raise ValueError("source path contains a symlink")
            current = current.parent
        if not result.resolve().is_relative_to(self.workspace.resolve()):
            raise ValueError("source path escaped workspace")
        return result
