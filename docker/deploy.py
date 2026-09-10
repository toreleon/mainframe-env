#!/usr/bin/env python3
"""Promote one tested binary, check readiness, and retain one previous binary."""
from __future__ import annotations
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time

ROOT = Path('/releases')
CONTAINER = 'mainframe-env-server'
BINARY = Path('/target/release/mainframe-env-server')


def command(*args: str) -> str:
    return subprocess.check_output(args, text=True).strip()


def point(root: Path, name: str, target: str) -> None:
    if not re.fullmatch(r'[0-9a-f]{40}-[0-9]+', target):
        raise ValueError('invalid release identity')
    temporary = root / f'.{name}.next'
    temporary.unlink(missing_ok=True)
    temporary.symlink_to(target, target_is_directory=True)
    os.replace(temporary, root / name)


def wait_healthy() -> None:
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        state = json.loads(command('docker', 'inspect', CONTAINER))[0]['State']
        if state.get('Health', {}).get('Status') == 'healthy':
            return
        if state.get('Status') in ('dead', 'exited'):
            break
        time.sleep(3)
    raise RuntimeError('new deployment did not become ready within 120 seconds')


def deploy() -> dict:
    if os.environ.get('MAINFRAME_ENV_CI_BRANCH') != 'main':
        raise ValueError('automatic deployment is restricted to main')
    sha = command('git', 'rev-parse', 'HEAD')
    # Validate against the fetched main ref, not a user-controlled branch label.
    if sha != command('git', 'rev-parse', 'refs/remotes/origin/main'):
        raise ValueError('candidate is not the checked-out origin/main commit')
    build = os.environ['BUILD_NUMBER']
    if not re.fullmatch(r'[0-9a-f]{40}', sha) or not re.fullmatch(r'[0-9]+', build):
        raise ValueError('invalid candidate or build number')
    name = f'{sha}-{build}'
    binary = BINARY
    checksum = hashlib.sha256(binary.read_bytes()).hexdigest()
    current = ROOT / 'current'
    last_good = ROOT / 'last-good'
    previous = os.readlink(last_good) if last_good.is_symlink() else None
    destination = ROOT / name
    destination.mkdir()  # Never overwrite an existing deployment.
    shutil.copy2(binary, destination / binary.name)
    (destination / binary.name).chmod(0o755)
    point(ROOT, 'current', name)
    try:
        command('docker', 'restart', '--time', '30', CONTAINER)
        wait_healthy()
    except Exception:
        if previous:
            point(ROOT, 'current', previous)
            command('docker', 'restart', '--time', '30', CONTAINER)
            wait_healthy()
        else:
            command('docker', 'stop', '--time', '30', CONTAINER)
            current.unlink(missing_ok=True)
        shutil.rmtree(destination)
        raise
    if previous:
        point(ROOT, 'previous', previous)
    point(ROOT, 'last-good', name)
    for path in ROOT.iterdir():
        if path.is_dir() and not path.is_symlink() and re.fullmatch(r'[0-9a-f]{40}-[0-9]+', path.name):
            if path.name not in {name, previous}:
                shutil.rmtree(path)
    return {'commit': sha, 'build': build, 'sha256': checksum,
            'current': name, 'previous': previous, 'ready': True}


def main():
    with (ROOT / '.deploy.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        receipt = deploy()
        Path('deployment.json').write_text(json.dumps(receipt, indent=2) + '\n')
        print(json.dumps(receipt))


if __name__ == '__main__':
    main()
