#!/usr/bin/env bash
# Print one healthy host Python interpreter without installing or modifying it.
set -euo pipefail

if [[ -n "${MAINFRAME_ENV_PYTHON:-}" ]]; then
  candidates=("$MAINFRAME_ENV_PYTHON")
else
  candidates=()
  [[ -n "${HOME:-}" ]] && candidates+=("$HOME/.local/bin/python3")
  candidates+=(/usr/bin/python3)
fi

for candidate in "${candidates[@]}"; do
  if [[ -x "$candidate" ]] \
    && "$candidate" -c 'import hashlib, math, ssl; hashlib.sha256(b"jenkins").digest()' \
      >/dev/null 2>&1; then
    printf '%s\n' "$candidate"
    exit 0
  fi
done

echo "no healthy Python interpreter found; set MAINFRAME_ENV_PYTHON to an absolute executable path" >&2
exit 1
