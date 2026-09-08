#!/usr/bin/env bash
# Run bounded libFuzzer targets without modifying their committed seed corpora.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
python_bin="${MAINFRAME_ENV_PYTHON:-python3}"
mode="${1:-smoke}"

case "$mode" in
  smoke|periodic) ;;
  *) echo "usage: $0 [smoke|periodic]" >&2; exit 2 ;;
esac

export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
"$python_bin" -B "$root/tools/assurance_gates.py" --root "$root" inventory

target_root="${CARGO_TARGET_DIR:?CARGO_TARGET_DIR must be set}"
mkdir -p "$target_root"
plan="$target_root/fuzz-plan-$mode-$$.tsv"
"$python_bin" -B "$root/tools/assurance_gates.py" --root "$root" \
  fuzz-plan --mode "$mode" > "$plan"
target_count=0
runs_per_target=''
while IFS=$'\t' read -r target seed_path toolchain tool_version runs max_input; do
  [[ -n "$target" && -n "$seed_path" && -n "$toolchain" && -n "$tool_version" \
    && -n "$runs" && -n "$max_input" ]] || {
    echo 'fuzz plan row is incomplete' >&2
    exit 1
  }
  cargo fuzz --version | grep -Fx "cargo-fuzz $tool_version" >/dev/null || {
    echo "cargo-fuzz $tool_version is required" >&2
    exit 1
  }
  RUSTUP_AUTO_INSTALL=0 rustup run "$toolchain" rustc --version >/dev/null
  RUSTUP_AUTO_INSTALL=0 rustup component list --toolchain "$toolchain" --installed \
    | grep -Eq '^rust-src(-|$)' || {
    echo "rust-src is missing from fuzz toolchain $toolchain" >&2
    exit 1
  }
  if [[ -z "$runs_per_target" ]]; then
    runs_per_target="$runs"
  elif [[ "$runs_per_target" != "$runs" ]]; then
    echo 'fuzz plan uses inconsistent per-target run budgets' >&2
    exit 1
  fi
  corpus="$target_root/fuzz-corpus-$mode-$target-$$"
  artifacts="$target_root/fuzz-artifacts-$mode-$target-$$"
  mkdir -p "$corpus" "$artifacts"
  cp "$root/$seed_path/"* "$corpus/"
  cargo +"$toolchain" fuzz run \
    --fuzz-dir "$root/fuzz" \
    --target-dir "$target_root/fuzz-build" \
    "$target" "$corpus" -- \
    -runs="$runs" -max_len="$max_input" -timeout=5 -artifact_prefix="$artifacts/"
  target_count=$((target_count + 1))
done < "$plan"
[[ "$target_count" -gt 0 ]] || { echo 'fuzz plan selected zero targets' >&2; exit 1; }

printf 'fuzz result: ok. %s targets; %s runs per target; mode=%s\n' \
  "$target_count" "$runs_per_target" "$mode"
