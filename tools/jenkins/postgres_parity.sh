#!/usr/bin/env bash
# Run backend parity against a disposable PostgreSQL 18 cluster on capped storage.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
action="${1:-run}"

postgres_bin() {
  local candidate=""
  if command -v postgres >/dev/null 2>&1; then
    candidate="$(dirname "$(python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$(command -v postgres)")")"
  elif command -v brew >/dev/null 2>&1; then
    candidate="$(cd "$(brew --prefix postgresql@18 2>/dev/null)/bin" && pwd -P)"
  fi
  [[ -n "$candidate" && -x "$candidate/postgres" ]] || {
    echo "PostgreSQL 18 tools are required (macOS: brew install postgresql@18)" >&2
    return 1
  }
  "$candidate/postgres" --version | grep -Eq 'PostgreSQL\)?[[:space:]]+18\.' || {
    echo "backend parity requires PostgreSQL major version 18" >&2
    return 1
  }
  printf '%s\n' "$candidate"
}

postgres_share() {
  local bin="$1"
  local share
  share="$($bin/pg_config --sharedir)"
  [[ -f "$share/postgres.bki" && -d "$share/timezone" ]] || {
    echo "PostgreSQL support files are missing below $share; repair or relink the PostgreSQL 18 installation" >&2
    return 1
  }
  printf '%s\n' "$share"
}

[[ "$action" == run || "$action" == smoke || "$action" == check || "$action" == cleanup ]] \
  || { echo "usage: $0 [run|smoke|check|cleanup]" >&2; exit 2; }

if [[ "$action" == check ]]; then
  bin="$(postgres_bin)"
  for tool in initdb pg_ctl pg_isready createdb psql pg_config; do
    [[ -x "$bin/$tool" ]] || { echo "missing PostgreSQL tool: $bin/$tool" >&2; exit 1; }
  done
  share="$(postgres_share "$bin")"
  printf 'PostgreSQL tools: %s (share: %s)\n' "$bin" "$share"
  exit 0
fi

state="${WORKSPACE:?WORKSPACE must be set}/.postgres"
data="$state/data"
socket="$state/socket"
log="$state/postgres.log"

cleanup() {
  local bin=""
  if [[ -f "$state/bin-path" ]]; then
    bin="$(<"$state/bin-path")"
  elif bin="$(postgres_bin 2>/dev/null)"; then
    :
  fi
  if [[ -n "$bin" && -x "$bin/pg_ctl" && -s "$data/postmaster.pid" ]]; then
    "$bin/pg_ctl" -D "$data" stop -m fast -w -t 30 || true
  fi
}

if [[ "$action" == cleanup ]]; then
  cleanup
  exit 0
fi

bin="$(postgres_bin)"
for tool in initdb pg_ctl pg_isready createdb psql pg_config; do
  [[ -x "$bin/$tool" ]] || { echo "missing PostgreSQL tool: $bin/$tool" >&2; exit 1; }
done
share="$(postgres_share "$bin")"
rm -rf "$state"
mkdir -p "$data" "$socket"
printf '%s\n' "$bin" > "$state/bin-path"
trap cleanup EXIT INT TERM

"$bin/initdb" -D "$data" -L "$share" --username=hardening --auth-local=trust \
  --auth-host=trust --encoding=UTF8 --no-locale --no-instructions >/dev/null
port=$((54000 + ${BUILD_NUMBER:-0} % 1000))
"$bin/pg_ctl" -D "$data" -l "$log" \
  -o "-F -k $socket -p $port -h 127.0.0.1" start -w -t 30
"$bin/pg_isready" -h 127.0.0.1 -p "$port" -U hardening -d postgres
"$bin/createdb" -h 127.0.0.1 -p "$port" -U hardening mainframe_env
if [[ "$action" == smoke ]]; then
  "$bin/psql" -h 127.0.0.1 -p "$port" -U hardening -d mainframe_env \
    -v ON_ERROR_STOP=1 -Atqc "select current_setting('server_version_num')::integer / 10000"
  exit 0
fi

export MAINFRAME_ENV_TEST_POSTGRES_URL="postgres://hardening@127.0.0.1:$port/mainframe_env"
export MAINFRAME_ENV_POSTGRES_TEST_URL="$MAINFRAME_ENV_TEST_POSTGRES_URL"
out="${CARGO_TARGET_DIR:?CARGO_TARGET_DIR must be set}/ci-backend"
mkdir -p "$out"
cp "$CARGO_TARGET_DIR/ci-assurance/plan.json" "$out/plan.json"

python3 -B "$root/tools/ci_assurance.py" record --output "$out" \
  --gate postgres-move --expect-tests -- \
  cargo test --locked -p mainframe-env-store --test provider_move_contract \
  postgres_move_contract -- --ignored --exact
gates=(postgres-move)
if [[ -f "$root/crates/stores/mainframe-env-store/tests/effect_encoding_contract.rs" ]]; then
  python3 -B "$root/tools/ci_assurance.py" record --output "$out" \
    --gate postgres-effect --expect-tests -- \
    cargo test --locked -p mainframe-env-store --test effect_encoding_contract \
    postgres_effect_domains_cannot_be_mixed -- --ignored --exact
  gates+=(postgres-effect)
fi
python3 -B "$root/tools/ci_assurance.py" summary --plan "$out/plan.json" \
  --directory "$out" --output "$out/summary.json" --gates "${gates[@]}"
