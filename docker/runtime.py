#!/usr/bin/env python3
"""Resolve file-mounted development secrets and check actual readiness."""
import base64
import json
import os
from pathlib import Path
import sys
import urllib.parse
import urllib.request


def main():
    if sys.argv[1] == 'health':
        with urllib.request.urlopen('http://127.0.0.1:10443/zosmf/info', timeout=3) as response:
            status = json.load(response)
        if status.get('ready') is not True:
            raise SystemExit('Application is not ready')
        return
    password = Path('/run/secrets/postgres_password').read_text().strip()
    admin = Path('/run/secrets/admin_password').read_bytes().strip()
    url = f'postgres://mainframe:{urllib.parse.quote(password, safe="")}@postgres:5432/mainframe_env'
    os.environ['MAINFRAME_ENV_SECRET_POSTGRES_URL'] = base64.b64encode(url.encode()).decode()
    os.environ['MAINFRAME_ENV_SECRET_BOOTSTRAP_ADMIN'] = base64.b64encode(admin).decode()
    # File secrets need root to cross the macOS bind-mount ownership boundary;
    # the application itself runs without root or supplementary groups.
    os.setgroups([])
    os.setgid(1000)
    os.setuid(1000)
    binary = '/releases/current/mainframe-env-server'
    os.execv(binary, [binary, '/opt/mainframe-env/docker/server.toml'])


if __name__ == '__main__':
    main()
