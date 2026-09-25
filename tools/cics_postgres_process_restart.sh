#!/usr/bin/env bash
# Run the bounded CIC-906 BTS subprocess selector against one disposable PostgreSQL 18.6 cluster.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$root"
if command -v brew >/dev/null 2>&1; then
  pg_bin="$(brew --prefix postgresql@18)/bin"
else
  pg_bin="$(dirname "$(command -v pg_config)")"
fi
[[ "$($pg_bin/pg_config --version)" =~ ^PostgreSQL[[:space:]]18\.6([[:space:]]|$) ]] || {
  echo 'PostgreSQL 18.6 is required' >&2
  exit 1
}
for tool in initdb pg_ctl createdb pg_config; do
  [[ -x "$pg_bin/$tool" ]] || { echo "missing $pg_bin/$tool" >&2; exit 1; }
done

scratch="$(mktemp -d "${TMPDIR:-/tmp}/cic906-bts-pg.XXXXXX")"
data="$scratch/data"
socket="$scratch/socket"
mkdir -p "$socket"
cleanup() {
  local result=$?
  trap - EXIT
  if [[ -s "$data/postmaster.pid" ]]; then
    "$pg_bin/pg_ctl" -D "$data" stop -m immediate -w -t 30 || true
  fi
  cargo clean || true
  if [[ -d "$scratch" && ! -L "$scratch" ]]; then
    rm -rf -- "$scratch"
  fi
  exit "$result"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

port="$(python3 -B -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
echo "CIC-906 task-owned PostgreSQL 18.6 cluster: data=$data socket=$socket port=$port"
"$pg_bin/initdb" -D "$data" -U cic906 --auth-local=trust --auth-host=trust \
  --encoding=UTF8 --no-locale --no-instructions >/dev/null
"$pg_bin/pg_ctl" -D "$data" -l "$scratch/postgres.log" \
  -o "-F -k $socket -p $port -h 127.0.0.1" start -w -t 30 >/dev/null
"$pg_bin/createdb" -h 127.0.0.1 -p "$port" -U cic906 cic906_bts_restart
export MAINFRAME_ENV_POSTGRES_TEST_URL="postgres://cic906@127.0.0.1:$port/cic906_bts_restart"
cargo test --locked -p mainframe-env-cics --lib \
  service::tests::bts_selected_link_reconciles_after_postgres_process_exit \
  -- --ignored --exact --nocapture
cargo test --locked -p mainframe-env-cics --lib \
  service::tests::bts_selected_link_reconciles_outer_receipt_after_postgres_reopen \
  -- --ignored --exact
