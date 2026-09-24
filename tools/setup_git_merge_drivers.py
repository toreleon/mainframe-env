#!/usr/bin/env python3
"""Install repository-local merge drivers used by parallel feature worktrees."""
from __future__ import annotations

from pathlib import Path
import subprocess


def main() -> int:
    root = Path(
        subprocess.check_output(
            ["git", "rev-parse", "--show-toplevel"], text=True
        ).strip()
    )
    settings = {
        "merge.mainframe-env-doc-manifest.name": "mainframe-env documentation manifest merger",
        "merge.mainframe-env-doc-manifest.driver": (
            "python3 -B tools/merge_documentation_manifest.py %O %A %B"
        ),
        "merge.mainframe-env-doc-manifest.recursive": "binary",
        "rerere.enabled": "true",
        "rerere.autoupdate": "true",
    }
    for key, value in settings.items():
        subprocess.run(
            ["git", "config", "--local", key, value],
            cwd=root,
            check=True,
        )
    print("installed mainframe-env merge drivers and automatic rerere")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
