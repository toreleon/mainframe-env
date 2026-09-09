#!/usr/bin/env python3
"""Upload release assets without ever replacing different remote bytes."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile
from typing import Callable


SAFE_TAG = re.compile(r"mainframe-env-v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\Z")
SAFE_ASSET = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,191}\Z")
MAX_ASSETS = 32
MAX_RESPONSE_BYTES = 4 * 1024 * 1024


class PublicationError(RuntimeError):
    pass


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            value.update(block)
    return value.hexdigest()


def verify_existing_assets(
    local_assets: list[Path],
    remote_names: set[str],
    download: Callable[[str, Path], None],
    directory: Path,
) -> list[Path]:
    missing = []
    for local in local_assets:
        if local.name not in remote_names:
            missing.append(local)
            continue
        remote = directory / local.name
        download(local.name, remote)
        if not remote.is_file() or digest(remote) != digest(local):
            raise PublicationError(
                f"refusing to replace release asset with different bytes: {local.name}"
            )
    return missing


def gh(arguments: list[str]) -> str:
    try:
        result = subprocess.run(
            ["gh", *arguments],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        detail = getattr(error, "stderr", b"")
        raise PublicationError(
            f"GitHub CLI failed: {detail.decode('utf-8', 'replace').strip() or error}"
        ) from error
    if len(result.stdout) > MAX_RESPONSE_BYTES:
        raise PublicationError("GitHub CLI response is too large")
    try:
        return result.stdout.decode("utf-8")
    except UnicodeError as error:
        raise PublicationError("GitHub CLI returned non-UTF-8 metadata") from error


def publish(tag: str, assets: list[Path]) -> tuple[list[str], list[str]]:
    if SAFE_TAG.fullmatch(tag) is None:
        raise PublicationError("release tag is invalid")
    if not 1 <= len(assets) <= MAX_ASSETS:
        raise PublicationError("release asset count is invalid")
    if any(path.is_symlink() for path in assets):
        raise PublicationError("release assets must be regular files")
    resolved = [path.resolve(strict=True) for path in assets]
    names = [path.name for path in resolved]
    if len(set(names)) != len(names) or any(
        SAFE_ASSET.fullmatch(name) is None for name in names
    ):
        raise PublicationError("release asset names must be safe and unique")
    if any(not path.is_file() for path in resolved):
        raise PublicationError("release assets must be regular files")

    try:
        metadata = json.loads(gh(["release", "view", tag, "--json", "assets"]))
        rows = metadata["assets"]
        remote = [row["name"] for row in rows]
    except (KeyError, TypeError, json.JSONDecodeError) as error:
        raise PublicationError("GitHub release asset metadata is malformed") from error
    if (
        not isinstance(rows, list)
        or len(rows) > 1024
        or any(not isinstance(name, str) for name in remote)
        or len(set(remote)) != len(remote)
    ):
        raise PublicationError("GitHub release asset metadata is invalid")

    with tempfile.TemporaryDirectory(prefix="release-assets.") as temporary:
        directory = Path(temporary)

        def download(name: str, destination: Path) -> None:
            target = destination.parent / name
            gh(["release", "download", tag, "--pattern", name, "--dir", str(destination.parent)])
            if target != destination:
                raise PublicationError("downloaded release asset path differs")

        missing = verify_existing_assets(resolved, set(remote), download, directory)
        if missing:
            gh(["release", "upload", tag, *[str(path) for path in missing]])
    return [path.name for path in resolved if path not in missing], [path.name for path in missing]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("assets", nargs="+")
    args = parser.parse_args()
    try:
        retained, uploaded = publish(args.tag, [Path(value) for value in args.assets])
    except (OSError, PublicationError) as error:
        parser.exit(1, f"release publication: {error}\n")
    for name in retained:
        print(f"identical remote asset retained: {name}")
    for name in uploaded:
        print(f"new remote asset uploaded: {name}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
