#!/usr/bin/env python3
"""Build exact Git/PostgreSQL sources without mutable package-manager inputs."""
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import urllib.request


def run(*args, cwd):
    subprocess.run(args, cwd=cwd, check=True)


def main():
    lock = json.loads(Path(__file__).with_name('inputs.lock.json').read_text())
    with tempfile.TemporaryDirectory(prefix='mainframe-tools-') as temporary:
        root = Path(temporary)
        for source in lock['sources']:
            if not source['url'].startswith('https://'):
                raise ValueError('source download must use HTTPS')
            path = root / f"{source['name']}.tar"
            digest = hashlib.sha256()
            size = 0
            with urllib.request.urlopen(source['url'], timeout=60) as response, path.open('wb') as output:
                if not response.geturl().startswith('https://'):
                    raise ValueError('source download left HTTPS')
                while chunk := response.read(1024 * 1024):
                    size += len(chunk)
                    if size > 256 * 1024**2:
                        raise ValueError('source archive exceeds its bound')
                    digest.update(chunk)
                    output.write(chunk)
            if digest.hexdigest() != source['sha256']:
                raise ValueError(f"source digest mismatch: {source['name']}")
            with tarfile.open(path) as archive:
                archive.extractall(root, filter='data')
            directory = root / f"{source['name']}-{source['version']}"
            if source['name'] in ('bison', 'flex'):
                run('./configure', '--prefix=/usr/local', '--disable-nls', cwd=directory)
                run('make', '-j2', cwd=directory)
                run('make', 'install', cwd=directory)
            elif source['name'] == 'git':
                run('make', '-j2', 'prefix=/usr/local', 'NO_GETTEXT=YesPlease',
                    'NO_TCLTK=YesPlease', 'NO_PERL=YesPlease', 'install', cwd=directory)
            else:
                run('./configure', '--prefix=/opt/postgresql', '--without-icu',
                    '--without-readline', '--with-openssl', cwd=directory)
                run('make', '-j2', cwd=directory)
                run('make', 'install', cwd=directory)


if __name__ == '__main__':
    main()
