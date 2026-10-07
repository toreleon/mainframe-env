"""Build a relocatable bundle with pinned source inputs and no runtime downloads."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from .instance import read_json, write_json

ROOT = Path(__file__).resolve().parents[2]
INVENTORY = Path("conformance/profiles/carddemo/inventory/carddemo-corpus.json")


def install_file(source: Path, destination: Path):
    descriptor, temporary = tempfile.mkstemp(dir=destination.parent)
    os.close(descriptor)
    try:
        shutil.copy2(source, temporary)
        os.replace(temporary, destination)
    finally:
        Path(temporary).unlink(missing_ok=True)


def prepare_reference(reference: Path, corpus: Path | None, identity: dict):
    """Fetch only the pinned commit, then validate reused or newly prepared checkouts."""
    if not reference.exists():
        reference.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix=".carddemo-", dir=reference.parent) as temporary:
            staged = Path(temporary) / "checkout"
            subprocess.run(["git", "init", "--quiet", str(staged)], check=True)
            command = ["git", "-C", str(staged)]
            subprocess.run(command + ["remote", "add", "origin", str(corpus.resolve()) if corpus else identity["repository"]], check=True)
            subprocess.run(command + ["fetch", "--quiet", "--depth=1", "--no-tags", "origin", identity["commit"]], check=True)
            subprocess.run(command + ["checkout", "--quiet", "--detach", identity["commit"]], check=True)
            subprocess.run(command + ["remote", "set-url", "origin", identity["repository"]], check=True)
            os.replace(staged, reference)
    command = ["git", "-C", str(reference)]
    for revision, expected in (("HEAD", identity["commit"]), ("HEAD^{tree}", identity["tree"])):
        if subprocess.check_output(command + ["rev-parse", revision], text=True).strip() != expected:
            raise ValueError("reference commit or tree differs from the pinned input")
    if subprocess.check_output(command + ["status", "--porcelain"], text=True).strip():
        raise ValueError("reference checkout is dirty")


def setup(destination: Path, corpus: Path | None, no_build: bool):
    if destination.is_symlink():
        raise ValueError("bundle destination cannot be a symlink")
    destination = destination.resolve()
    if destination == ROOT or destination.is_relative_to(ROOT / "crates"):
        raise ValueError("bundle destination cannot overwrite repository source")
    destination.mkdir(parents=True, exist_ok=True)
    manifest = destination / "bundle.json"
    if any(destination.iterdir()) and not manifest.exists():
        raise ValueError("bundle destination is nonempty and not owned by setup")
    if manifest.exists() and read_json(manifest).get("schema_version") != "mainframe-sandbox.bundle@1":
        raise ValueError("bundle schema differs")
    # Mark ownership before building so an interrupted setup can be resumed.
    write_json(manifest, {"schema_version": "mainframe-sandbox.bundle@1", "ready": False})
    bins = destination / "bin"
    bins.mkdir(exist_ok=True)
    if not no_build:
        try:
            subprocess.run(["cargo", "build", "--locked", "--release", "-p", "mainframe-env-cli", "-p", "mainframe-env-conformance", "-p", "xtask",
                            "--bin", "mainframe-env", "--bin", "mainframe-sandbox-runtime", "--bin", "xtask"], cwd=ROOT, check=True)
            metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], cwd=ROOT))
            for name in ("mainframe-env", "mainframe-sandbox-runtime"):
                install_file(Path(metadata["target_directory"]) / "release" / name, bins / name)
            subprocess.run([str(Path(metadata["target_directory"]) / "release/xtask"), "license-notices", "--output",
                            str(destination / "THIRD-PARTY-NOTICES.md")], cwd=ROOT, check=True)
        finally:
            subprocess.run(["cargo", "clean"], cwd=ROOT, check=True)
    else:
        for name in ("mainframe-env", "mainframe-sandbox-runtime"):
            install_file(ROOT / "target" / "release" / name, bins / name)
        subprocess.run([str(ROOT / "target/release/xtask"), "license-notices", "--output",
                        str(destination / "THIRD-PARTY-NOTICES.md")], cwd=ROOT, check=True)
    install_file(ROOT / "bin/mainframe-sandbox", bins / "mainframe-sandbox")
    shutil.copytree(ROOT / "tools/sandbox", destination / "tools/sandbox", dirs_exist_ok=True,
                    ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
    (destination / "share").mkdir(exist_ok=True)
    shutil.copy2(ROOT / INVENTORY, destination / "share/carddemo-corpus.json")
    for name in ("LICENSE", "NOTICE"):
        shutil.copy2(ROOT / name, destination / name)
    shutil.copytree(ROOT / "LICENSES", destination / "LICENSES", dirs_exist_ok=True)
    identity = read_json(ROOT / INVENTORY)
    reference = destination / "reference/carddemo"
    prepare_reference(reference, corpus, identity)
    subprocess.run([str(bins / "mainframe-sandbox-runtime"), "--help"], check=True, stdout=subprocess.DEVNULL)
    write_json(manifest, {"schema_version": "mainframe-sandbox.bundle@1", "ready": True,
                         "reference_commit": identity["commit"], "binaries": {
        p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in bins.iterdir() if p.is_file()}})
    return {"bundle": str(destination), "command": str(bins / "mainframe-sandbox")}
