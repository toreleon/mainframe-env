"""Build a relocatable bundle with pinned source inputs and no runtime downloads."""
from __future__ import annotations

import fcntl
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from .instance import read_json, write_json
from .setup_evidence import digest, Evidence, source_state

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


def verify_reference(reference: Path, identity: dict) -> None:
    command = ["git", "-C", str(reference)]
    if reference.is_symlink():
        raise ValueError("reference checkout cannot be a symlink")
    if subprocess.check_output(command + ["remote", "get-url", "origin"], text=True).strip() != identity["repository"]:
        raise ValueError("reference origin differs from the pinned input")
    for revision, expected in (("HEAD", identity["commit"]), ("HEAD^{tree}", identity["tree"])):
        if subprocess.check_output(command + ["rev-parse", revision], text=True).strip() != expected:
            raise ValueError("reference commit or tree differs from the pinned input")
    if subprocess.check_output(command + ["status", "--porcelain"], text=True).strip():
        raise ValueError("reference checkout is dirty")


def prepare_reference(reference: Path, corpus: Path | None, identity: dict) -> None:
    """Fetch only the pinned commit, then validate reused or newly prepared checkouts."""
    if reference.is_symlink():
        raise ValueError("reference checkout cannot be a symlink")
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
            verify_reference(staged, identity)
            os.replace(staged, reference)
    else:
        verify_reference(reference, identity)


def regular_path(path: Path, role: str) -> Path:
    """Resolve only after rejecting every symlink component, including dangling links."""
    path = Path(os.path.abspath(path))
    for component in (path, *path.parents):
        if component.is_symlink():
            raise ValueError(f"{role} cannot contain a symlink")
    return path.resolve()


def overlaps(first: Path, second: Path) -> bool:
    return first.is_relative_to(second) or second.is_relative_to(first)


def setup(destination: Path, corpus: Path | None, no_build: bool,
          target_directory: Path | None = None, producer_evidence: Path | None = None):
    destination = regular_path(destination, "bundle destination")
    root = ROOT.resolve()
    if root.is_relative_to(destination) or destination == Path.home().resolve() or any(
            destination.is_relative_to(root / name) for name in
            ("crates", "tools", "docs", "conformance", "xtask", "bin", "fuzz", ".git", ".cargo", "changes", "LICENSES")):
        raise ValueError("bundle destination cannot overwrite repository source or a broad directory")
    target = regular_path(target_directory or root / "target", "Cargo target directory")
    if target_directory is not None:
        if target == root / "target" or not target.is_relative_to(root / "target"):
            raise ValueError("explicit Cargo target must be a fresh directory below this checkout's target/")
        if target.exists():
            raise ValueError("explicit Cargo target must be fresh")
    if overlaps(target, destination) or (corpus and overlaps(target, corpus.resolve())):
        raise ValueError("Cargo target must not overlap the bundle or reference input")
    if no_build and (target_directory is not None or producer_evidence is not None):
        raise ValueError("--no-build cannot supply a fresh target or producer evidence")
    if producer_evidence is not None and target_directory is None:
        raise ValueError("producer evidence requires an explicit fresh --target-directory")
    evidence_path = regular_path(producer_evidence, "producer evidence") if producer_evidence else None
    if evidence_path and any(overlaps(evidence_path, path) for path in (root, target, destination)):
        raise ValueError("producer evidence must be outside the checkout, target and bundle")
    if evidence_path and corpus and overlaps(evidence_path, corpus.resolve()):
        raise ValueError("producer evidence must not overlap the reference input")
    if evidence_path and (evidence_path / "setup-receipt.json").exists():
        raise ValueError("producer evidence already contains a setup receipt; choose a fresh directory")
    destination.mkdir(parents=True, exist_ok=True)
    manifest = destination / "bundle.json"
    if any(item.name != ".setup.lock" for item in destination.iterdir()) and not manifest.exists():
        raise ValueError("bundle destination is nonempty and not owned by setup")
    if manifest.exists() and read_json(manifest).get("schema_version") != "mainframe-sandbox.bundle@1":
        raise ValueError("bundle schema differs")
    # An owned bundle never grants permission to follow an internal link.
    for directory, directories, files in os.walk(destination, followlinks=False):
        if any((Path(directory) / name).is_symlink() for name in directories + files):
            raise ValueError("bundle contains a symlink")
    descriptor = os.open(destination / ".setup.lock", os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise ValueError("bundle setup already has an owner") from error
        return build_bundle(destination, corpus, no_build, target, target_directory is not None, evidence_path)
    finally:
        os.close(descriptor)


def build_bundle(destination: Path, corpus: Path | None, no_build: bool, target: Path,
                 fresh_target: bool, evidence_path: Path | None):
    manifest = destination / "bundle.json"
    evidence = Evidence(evidence_path, ROOT, destination, target) if evidence_path else None
    target_identity = None
    bins = destination / "bin"
    build_environment = {**os.environ, "CARGO_TARGET_DIR": str(target), "CARGO_NET_OFFLINE": "true"}
    run = evidence.run if evidence else lambda name, command, **kwargs: subprocess.run(command, **kwargs)
    error = None
    try:
        if fresh_target:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.mkdir()  # Atomic claim: never consume another producer's target.
            target_identity = (target.stat().st_dev, target.stat().st_ino)
        # Mark ownership before building so an interrupted setup can be resumed.
        write_json(manifest, {"schema_version": "mainframe-sandbox.bundle@1", "ready": False})
        bins.mkdir(exist_ok=True)
        # A failed attempt must never retain a previous producer's notice output.
        (destination / "THIRD-PARTY-NOTICES.md").unlink(missing_ok=True)
        if evidence:
            evidence.start(ROOT)
        if not no_build:
            run("build", ["cargo", "build", "--frozen", "--offline", "--release", "-p", "mainframe-env-cli", "-p", "mainframe-env-conformance", "-p", "xtask",
                          "--bin", "mainframe-env", "--bin", "mainframe-sandbox-runtime", "--bin", "xtask"],
                cwd=ROOT, env=build_environment, check=True)
        for name in ("mainframe-env", "mainframe-sandbox-runtime"):
            digest(target / "release" / name)
            install_file(target / "release" / name, bins / name)
        run("license-notices", [str(target / "release/xtask"), "license-notices", "--output",
                                str(destination / "THIRD-PARTY-NOTICES.md")],
            cwd=ROOT, env=build_environment, check=True)
        digest(destination / "THIRD-PARTY-NOTICES.md")
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
        run("runtime-help", [str(bins / "mainframe-sandbox-runtime"), "--help"],
            cwd=ROOT, env=build_environment, check=True)
        if evidence:
            evidence.state["reference_commit"] = identity["commit"]
    except (ValueError, OSError, KeyError, subprocess.SubprocessError) as failure:
        error = failure
    finally:
        if evidence and "source_start" in evidence.state:
            try:
                evidence.state["source_end"] = source_state(ROOT)
                if evidence.state["source_start"] != evidence.state["source_end"]:
                    error = error or ValueError("checkout source changed during setup; producer identity is not stable")
            except (ValueError, OSError, subprocess.SubprocessError) as failure:
                error = error or failure
        try:
            regular_path(target, "Cargo target cleanup")
            if fresh_target:
                if target_identity is not None:
                    if (target.stat().st_dev, target.stat().st_ino) != target_identity:
                        raise ValueError("Cargo target ownership changed; refusing cleanup")
                    if evidence:
                        evidence.retain(target, destination)
                    shutil.rmtree(target)
            else:
                run("clean", ["cargo", "clean"], cwd=ROOT, env=build_environment, check=True)
            if evidence:
                evidence.state["cleanup"] = {"target_directory": str(target),
                                             "passed": target_identity is not None,
                                             "claimed": target_identity is not None}
        except (ValueError, OSError, subprocess.SubprocessError) as failure:
            if evidence:
                evidence.state["errors"].append({"stage": "retention-or-cleanup", "error": str(failure)})
            error = error or failure
        if evidence:
            if error:
                evidence.state["errors"].append({"stage": "setup", "error": str(error)})
            evidence.finish(False)
    if error:
        raise error
    try:
        write_json(manifest, {"schema_version": "mainframe-sandbox.bundle@1", "ready": True,
                             "reference_commit": identity["commit"], "binaries": {
            p.name: digest(p) for p in bins.iterdir() if p.is_file()}})
        if evidence:
            evidence.finish(True)
    except (ValueError, OSError) as failure:
        try:
            write_json(manifest, {"schema_version": "mainframe-sandbox.bundle@1", "ready": False})
        finally:
            if evidence:
                evidence.state["errors"].append({"stage": "publication", "error": str(failure)})
                evidence.finish(False)
        raise
    result = {"bundle": str(destination), "command": str(bins / "mainframe-sandbox")}
    if evidence:
        result["producer_evidence"] = str(evidence.path)
    return result
