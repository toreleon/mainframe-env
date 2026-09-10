#!/usr/bin/env python3
"""Hard capacity checks and narrowly scoped, offline cache reclamation."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import secrets
import shutil

LIMIT = 50_000_000_000
HOST_LIMIT = 44 * 1024**3
RESERVE = 3 * 1024**3


def usage(path: Path, limit: int = LIMIT, reserve: int = RESERVE) -> dict:
    path = path.resolve(strict=True)
    stat = os.statvfs(path)
    total = stat.f_blocks * stat.f_frsize
    free = stat.f_bavail * stat.f_frsize
    if total <= 0 or total > limit:
        raise ValueError(f'{path}: capacity {total} exceeds the {limit}-byte boundary')
    if free < reserve:
        raise ValueError(f'{path}: {free} bytes free; {reserve} required. Reclaim caches before building.')
    return {'path': str(path), 'capacity_bytes': total, 'free_bytes': free}


def clean(cache: Path, target: Path) -> list[str]:
    # Call only while holding the cache's exclusive flock. These are disposable
    # Cargo paths, never Jenkins home, deployment artifacts or database volumes.
    if str(cache) != '/cache' or str(target) != '/target':
        raise ValueError('cleanup accepts only the container cache and target mounts')
    removed = []
    for base, relative in [(target, 'debug'), (target, 'release'), (target, 'doc'),
                           (cache, 'registry/cache'), (cache, 'registry/src'),
                           (cache, 'git/db'), (cache, 'git/checkouts')]:
        candidate = base / relative
        if candidate.is_symlink():
            raise ValueError(f'refusing symlink: {candidate}')
        if candidate.exists():
            resolved = candidate.resolve(strict=True)
            if not resolved.is_relative_to(base.resolve(strict=True)):
                raise ValueError(f'cache path escapes mount: {candidate}')
            shutil.rmtree(candidate)
            removed.append(str(candidate))
    return removed


def create_secrets(directory: Path) -> None:
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    for name in ('jenkins_password', 'postgres_password', 'admin_password'):
        path = directory / name
        try:
            fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        except FileExistsError:
            if path.is_symlink() or not path.is_file() or path.stat().st_mode & 0o077:
                raise ValueError(f'unsafe existing secret: {path}')
            continue
        with os.fdopen(fd, 'w') as stream:
            stream.write('Mfe9!' + secrets.token_urlsafe(24) + '\n')


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['host', 'check', 'clean', 'secrets'])
    parser.add_argument('paths', nargs='+', type=Path)
    parser.add_argument('--minimum-free-bytes', type=int, default=RESERVE)
    args = parser.parse_args()
    if args.command == 'secrets':
        create_secrets(args.paths[0])
    elif args.command == 'clean':
        print(json.dumps({'removed': clean(*args.paths)}))
    else:
        if args.command == 'host' and not os.path.ismount(args.paths[0]):
            raise ValueError('the dedicated capped filesystem is not mounted; run docker/dev init')
        limit = HOST_LIMIT if args.command == 'host' else LIMIT
        if args.minimum_free_bytes < 0:
            raise ValueError('minimum free bytes must not be negative')
        print(json.dumps([usage(path, limit, args.minimum_free_bytes) for path in args.paths]))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError) as error:
        raise SystemExit(f'Docker storage guard: {error}')
