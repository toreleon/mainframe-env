#!/usr/bin/env bash
# Record and validate bounded source coverage for parser/decoder/durable contracts.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
python_bin="${MAINFRAME_ENV_PYTHON:-python3}"
target_root="${CARGO_TARGET_DIR:?CARGO_TARGET_DIR must be set}"
report="$target_root/coverage/summary.json"

export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
cargo llvm-cov --version | grep -Fx 'cargo-llvm-cov 0.9.1' >/dev/null || {
  echo 'cargo-llvm-cov 0.9.1 is required' >&2
  exit 1
}
mkdir -p "$(dirname "$report")"
export CARGO_LLVM_COV_TARGET_DIR="$target_root/llvm-cov-target"
packages=()
while IFS= read -r package; do
  [[ -n "$package" ]] || { echo 'coverage plan contains an empty package' >&2; exit 1; }
  packages+=(-p "$package")
done < <("$python_bin" -B "$root/tools/assurance_gates.py" --root "$root" coverage-plan)
[[ "${#packages[@]}" -gt 0 ]] || { echo 'coverage plan selected zero packages' >&2; exit 1; }
cargo llvm-cov --locked --json --summary-only --output-path "$report" \
  "${packages[@]}"
"$python_bin" -B "$root/tools/assurance_gates.py" --root "$root" \
  coverage --report "$report"
