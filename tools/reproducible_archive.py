#!/usr/bin/env python3
"""Create a byte-reproducible gzip archive in one immutable GNU-tar image."""
from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile


REPRODUCIBLE_ARCHIVE_IMAGE = "docker.io/library/debian@sha256:5ae3c39ebd15e229dcedd5cee596b2497182493d41ff162e824ba13fc1b2b867"
REPRODUCIBLE_ARCHIVE_PLATFORM = "linux/amd64"
MAX_FILES = 200_000
MAX_BYTES = 4 * 1024 * 1024 * 1024
SAFE_ROOT_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,191}\Z")


class ArchiveError(RuntimeError):
    pass


def _verify_archive_runtime() -> None:
    checker = Path(__file__).with_name("supply_chain.py")
    try:
        subprocess.run(
            [sys.executable, "-B", str(checker), "check", "--runtime", "offline"],
            check=True,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        raise ArchiveError("the locked archive runtime verification failed") from error


def _digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def _validate_tree(source: Path) -> None:
    source = source.resolve(strict=True)
    if not source.is_dir():
        raise ArchiveError("archive source is not a directory")
    files = 0
    size = 0
    for path in source.rglob("*"):
        files += 1
        if files > MAX_FILES:
            raise ArchiveError("archive source has too many entries")
        if path.is_symlink():
            try:
                target = path.resolve(strict=True)
            except OSError as error:
                raise ArchiveError(f"archive source has a broken symlink: {path}") from error
            if source != target and source not in target.parents:
                raise ArchiveError(f"archive source symlink escapes its root: {path}")
        elif path.is_file():
            size += path.stat().st_size
            if size > MAX_BYTES:
                raise ArchiveError("archive source is too large")
        elif not path.is_dir():
            raise ArchiveError(f"archive source has an unsupported entry: {path}")


def _perturb_mtimes(root: Path, tick: int) -> None:
    paths = sorted(root.rglob("*"), reverse=True)
    paths.append(root)
    for path in paths:
        if not path.is_symlink():
            os.utime(path, (tick, tick), follow_symlinks=False)


def _run_container(input_directory: Path, output_directory: Path, member: str) -> None:
    for path in (input_directory, output_directory):
        if any(character in str(path) for character in (",", "\n", "\r")):
            raise ArchiveError("container mount path contains an unsupported character")
    command = [
        "docker",
        "run",
        "--rm",
        "--pull=always",
        "--platform",
        REPRODUCIBLE_ARCHIVE_PLATFORM,
        "--network",
        "none",
        "--read-only",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges",
        "--user",
        f"{os.getuid()}:{os.getgid()}",
        "--mount",
        f"type=bind,source={input_directory},target=/input,readonly",
        "--mount",
        f"type=bind,source={output_directory},target=/output",
        "--env",
        f"ARCHIVE_MEMBER={member}",
        REPRODUCIBLE_ARCHIVE_IMAGE,
        "sh",
        "-ceu",
        """
test "$(tar --version | sed -n '1p')" = 'tar (GNU tar) 1.34'
test "$(gzip --version | sed -n '1p')" = 'gzip 1.12'
cd /input
LC_ALL=C TZ=UTC tar \
  --format=ustar \
  --sort=name \
  --mtime=@0 \
  --numeric-owner \
  --owner=0 \
  --group=0 \
  --mode='u+rwX,go+rX,go-w' \
  --no-acls \
  --no-selinux \
  --no-xattrs \
  -cf - -- "$ARCHIVE_MEMBER" | gzip -9 -n > /output/archive.tar.gz
""",
    ]
    try:
        subprocess.run(command, check=True)
    except (OSError, subprocess.CalledProcessError) as error:
        raise ArchiveError("the pinned GNU-tar archive environment failed") from error


def _install_immutable(candidate: Path, destination: Path, expected_digest: str) -> None:
    if destination.exists():
        if not destination.is_file() or _digest(destination) != expected_digest:
            raise ArchiveError(f"refusing to replace different existing output: {destination}")
        return
    try:
        os.link(candidate, destination)
    except FileExistsError:
        if not destination.is_file() or _digest(destination) != expected_digest:
            raise ArchiveError(f"concurrent output differs: {destination}")
    except OSError as error:
        raise ArchiveError(f"cannot publish archive atomically: {destination}") from error


def create_archive(source: Path, output: Path, root_name: str | None = None) -> str:
    source = source.resolve(strict=True)
    output = output.resolve(strict=False)
    if output.suffixes[-2:] != [".tar", ".gz"]:
        raise ArchiveError("archive output must end in .tar.gz")
    if root_name is not None and SAFE_ROOT_NAME.fullmatch(root_name) is None:
        raise ArchiveError("archive root name is invalid")
    output.parent.mkdir(parents=True, exist_ok=True)
    if source == output.parent or source in output.parent.parents:
        raise ArchiveError("archive output must not contain its source")
    _validate_tree(source)

    with tempfile.TemporaryDirectory(prefix="reproducible-archive.", dir=output.parent) as temporary:
        temporary_root = Path(temporary)
        candidates: list[Path] = []
        for run, tick in ((1, 1), (2, 2_000_000_000)):
            run_root = temporary_root / f"run-{run}"
            input_root = run_root / "input"
            output_root = run_root / "output"
            input_root.mkdir(parents=True)
            output_root.mkdir()
            if root_name is None:
                copied = input_root / "payload"
                member = "."
                shutil.copytree(source, copied, symlinks=True)
                mounted_input = copied
            else:
                copied = input_root / root_name
                member = root_name
                shutil.copytree(source, copied, symlinks=True)
                mounted_input = input_root
            _perturb_mtimes(copied, tick)
            _run_container(mounted_input, output_root, member)
            candidate = output_root / "archive.tar.gz"
            if not candidate.is_file() or candidate.stat().st_size == 0:
                raise ArchiveError("archive environment did not produce an archive")
            candidates.append(candidate)

        digests = [_digest(candidate) for candidate in candidates]
        if digests[0] != digests[1]:
            raise ArchiveError("clean archive reproductions have different SHA-256 digests")
        checksum = output.with_name(output.name + ".sha256")
        expected_checksum = f"{digests[0]}  {output.name}\n".encode("ascii")
        if checksum.exists() and checksum.read_bytes() != expected_checksum:
            raise ArchiveError(f"refusing to replace different existing output: {checksum}")
        _install_immutable(candidates[0], output, digests[0])
        checksum_candidate = temporary_root / "archive.sha256"
        checksum_candidate.write_bytes(expected_checksum)
        _install_immutable(
            checksum_candidate,
            checksum,
            hashlib.sha256(expected_checksum).hexdigest(),
        )
        return digests[0]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--root-name")
    args = parser.parse_args()
    try:
        _verify_archive_runtime()
        digest = create_archive(args.source, args.output, args.root_name)
    except (ArchiveError, OSError) as error:
        parser.exit(1, f"reproducible archive: {error}\n")
    print(f"sha256:{digest}  {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
