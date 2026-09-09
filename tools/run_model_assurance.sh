#!/usr/bin/env bash
# Execute the registered bounded Loom model and reject empty selection.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
python_bin="${MAINFRAME_ENV_PYTHON:-python3}"
plan="$("$python_bin" -B "$root/tools/assurance_gates.py" --root "$root" model-plan)"
IFS=$'\t' read -r package test_name minimum_tests <<< "$plan"
[[ -n "$package" && -n "$test_name" && "$minimum_tests" =~ ^[1-9][0-9]*$ ]] || {
  echo 'model plan is empty or malformed' >&2
  exit 1
}
cargo test --locked -p "$package" --test "$test_name"
