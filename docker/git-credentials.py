#!/usr/bin/env python3
"""Provision repository checkout from the host's existing Git credential helper."""
import json
import os
from pathlib import Path
import subprocess
import sys


def main():
    path = Path(sys.argv[1])
    if path.exists():
        if path.is_symlink() or path.stat().st_mode & 0o077:
            raise SystemExit('Git credential file must be private and not a symlink')
        return
    result = subprocess.run(
        ['git', 'credential', 'fill'],
        input='protocol=https\nhost=github.com\npath=toreleon/mainframe-env.git\n\n',
        text=True, capture_output=True,
        env={**os.environ, 'GIT_TERMINAL_PROMPT': '0'}, check=False,
    )
    fields = dict(line.split('=', 1) for line in result.stdout.splitlines() if '=' in line)
    credential = {}
    if result.returncode == 0 and fields.get('password'):
        credential = {'username': fields.get('username', 'x-access-token'),
                      'password': fields['password']}
    else:
        print('No stored GitHub credential; Jenkins will attempt anonymous checkout.')
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, 'w') as output:
        json.dump(credential, output)


if __name__ == '__main__':
    main()
