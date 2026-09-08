#!/usr/bin/env python3
"""Verify and maintain the capped filesystem used by local Jenkins.

The capacity check is deliberately fail-closed.  Cleanup can reduce usage, but
only a filesystem whose reported total capacity is at most 10 GiB supplies the
hard limit promised by the local Jenkins setup.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import sys

HARD_LIMIT_BYTES = 10 * 1024**3
DEFAULT_PRUNE_PERCENT = 75
DEFAULT_MINIMUM_FREE_BYTES = 2 * 1024**3


def filesystem_usage(path: Path) -> dict[str, int | float]:
    stats = os.statvfs(path)
    total = stats.f_blocks * stats.f_frsize
    free = stats.f_bavail * stats.f_frsize
    used = total - free
    return {
        'total_bytes': total,
        'used_bytes': used,
        'free_bytes': free,
        'used_percent': round((used / total) * 100, 2) if total else 100.0,
    }


def contained(root: Path, candidate: Path) -> bool:
    try:
        candidate.relative_to(root)
        return True
    except ValueError:
        return False


def parse_named_path(raw: str) -> tuple[str, Path]:
    name, separator, value = raw.partition('=')
    if not separator or not name or not value:
        raise ValueError('paths must use NAME=/absolute/path')
    path = Path(value)
    if not path.is_absolute():
        raise ValueError(f'{name} must be absolute: {value}')
    return name, path


def resolve_for_validation(path: Path) -> Path:
    if path.exists():
        return path.resolve(strict=True)
    missing: list[str] = []
    existing = path
    while not existing.exists():
        missing.append(existing.name)
        parent = existing.parent
        if parent == existing:
            raise ValueError(f'path has no existing ancestor: {path}')
        existing = parent
    resolved = existing.resolve(strict=True)
    for name in reversed(missing):
        resolved /= name
    return resolved


def verify(root_path: Path, named_paths: list[str], minimum_free: int) -> dict:
    root = root_path.resolve(strict=True)
    if not root.is_dir():
        raise ValueError(f'capped root is not a directory: {root}')
    usage = filesystem_usage(root)
    if usage['total_bytes'] > HARD_LIMIT_BYTES:
        raise ValueError(
            f'{root} reports {usage["total_bytes"]} bytes of capacity; '
            f'the hard limit is {HARD_LIMIT_BYTES} bytes'
        )

    root_device = root.stat().st_dev
    checked: dict[str, str] = {}
    for raw in named_paths:
        name, path = parse_named_path(raw)
        resolved = resolve_for_validation(path)
        if not contained(root, resolved):
            raise ValueError(f'{name} escapes capped root {root}: {resolved}')
        existing = resolved
        while not existing.exists():
            existing = existing.parent
        if existing.stat().st_dev != root_device:
            raise ValueError(f'{name} is not on the capped filesystem: {resolved}')
        checked[name] = str(resolved)

    if usage['free_bytes'] < minimum_free:
        raise ValueError(
            f'capped filesystem has only {usage["free_bytes"]} bytes free; '
            f'{minimum_free} bytes are required before a build'
        )
    return {'root': str(root), 'hard_limit_bytes': HARD_LIMIT_BYTES,
            'paths': checked, **usage}


def prune(root_path: Path, cargo_home_path: Path, threshold: int) -> dict:
    root = root_path.resolve(strict=True)
    cargo_home = resolve_for_validation(cargo_home_path)
    if not contained(root, cargo_home):
        raise ValueError(f'CARGO_HOME escapes capped root {root}: {cargo_home}')

    before = filesystem_usage(root)
    removed: list[str] = []
    if before['used_percent'] >= threshold:
        for relative in ('registry/cache', 'git/db', 'registry/src', 'git/checkouts'):
            candidate = cargo_home / relative
            if candidate.exists():
                resolved = candidate.resolve(strict=True)
                if not contained(cargo_home, resolved):
                    raise ValueError(f'refusing to prune path outside CARGO_HOME: {resolved}')
                shutil.rmtree(resolved)
                removed.append(str(resolved))
    return {'root': str(root), 'threshold_percent': threshold,
            'removed': removed, 'before': before, 'after': filesystem_usage(root)}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest='command', required=True)

    check = subparsers.add_parser('verify')
    check.add_argument('--root', type=Path, required=True)
    check.add_argument('--require', action='append', default=[])
    check.add_argument('--minimum-free-bytes', type=int, default=DEFAULT_MINIMUM_FREE_BYTES)

    cleanup = subparsers.add_parser('prune')
    cleanup.add_argument('--root', type=Path, required=True)
    cleanup.add_argument('--cargo-home', type=Path, required=True)
    cleanup.add_argument('--threshold-percent', type=int, default=DEFAULT_PRUNE_PERCENT)

    args = parser.parse_args()
    if args.command == 'verify':
        if args.minimum_free_bytes < 0:
            raise ValueError('minimum free bytes cannot be negative')
        result = verify(args.root, args.require, args.minimum_free_bytes)
    else:
        if not 1 <= args.threshold_percent <= 100:
            raise ValueError('prune threshold must be between 1 and 100')
        result = prune(args.root, args.cargo_home, args.threshold_percent)
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError) as error:
        raise SystemExit(f'Jenkins capped-storage guard failed closed: {error}')
