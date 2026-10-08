#!/usr/bin/env bash
# Run backend parity against a disposable PostgreSQL 18 cluster on capped storage.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
python_bin="${MAINFRAME_ENV_PYTHON:-$("$root/tools/jenkins/select-python.sh")}"
action="${1:-run}"

postgres_bin() {
  local candidate=""
  if command -v postgres >/dev/null 2>&1; then
    candidate="$(dirname "$("$python_bin" -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$(command -v postgres)")")"
  elif command -v brew >/dev/null 2>&1; then
    candidate="$(cd "$(brew --prefix postgresql@18 2>/dev/null)/bin" && pwd -P)"
  fi
  [[ -n "$candidate" && -x "$candidate/postgres" ]] || {
    echo "the exact PostgreSQL version in tools/ci-inputs.lock.json is required" >&2
    return 1
  }
  PATH="$candidate:$PATH" "$python_bin" -B "$root/tools/supply_chain.py" check \
    --runtime postgres >/dev/null
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

gates=(
  postgres-move postgres-effect postgres-stale-effect-recovery postgres-online-resume
  postgres-atomic-invariants postgres-work-leases postgres-storage-profile
  postgres-artifact-read-versions postgres-readiness postgres-retention postgres-durable
  postgres-carddemo-restart
)

[[ "$action" == run || "$action" == smoke || "$action" == check || "$action" == cleanup \
  || "$action" == list ]] || { echo "usage: $0 [run|smoke|check|cleanup|list]" >&2; exit 2; }

if [[ "$action" == list ]]; then
  printf '%s\n' "${gates[@]}"
  exit 0
fi

if [[ "$action" == check ]]; then
  bin="$(postgres_bin)"
  for tool in initdb pg_ctl pg_isready createdb dropdb psql pg_config; do
    [[ -x "$bin/$tool" ]] || { echo "missing PostgreSQL tool: $bin/$tool" >&2; exit 1; }
  done
  share="$(postgres_share "$bin")"
  printf 'PostgreSQL tools: %s (share: %s)\n' "$bin" "$share"
  exit 0
fi

workspace="${WORKSPACE:?WORKSPACE must be set}"
state="$workspace/.postgres"
# Development runs use `.postgres` as a bounded scratch
# volume. A mount point cannot itself be removed, so use one owned child while
# retaining the historical Jenkins path on ordinary workspaces.
if command -v mountpoint >/dev/null 2>&1 && mountpoint -q "$state"; then
  state="$state/parity"
fi
case "$state/" in
  "$workspace/.postgres/"*) ;;
  *) echo "unsafe PostgreSQL parity state path: $state" >&2; exit 1 ;;
esac
data="$state/data"
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
for tool in initdb pg_ctl pg_isready createdb dropdb psql pg_config; do
  [[ -x "$bin/$tool" ]] || { echo "missing PostgreSQL tool: $bin/$tool" >&2; exit 1; }
done
share="$(postgres_share "$bin")"
rm -rf "$state"
mkdir -p "$data"
printf '%s\n' "$bin" > "$state/bin-path"
trap cleanup EXIT INT TERM

"$bin/initdb" -D "$data" -L "$share" --username=hardening --auth-local=trust \
  --auth-host=trust --encoding=UTF8 --no-locale --no-instructions >/dev/null
port=$((54000 + ${BUILD_NUMBER:-0} % 1000))
"$bin/pg_ctl" -D "$data" -l "$log" \
  -o "-F -k '' -p $port -h 127.0.0.1" start -w -t 30
"$bin/pg_isready" -h 127.0.0.1 -p "$port" -U hardening -d postgres
if [[ "$action" == smoke ]]; then
  "$bin/createdb" -h 127.0.0.1 -p "$port" -U hardening mainframe_env
  "$bin/psql" -h 127.0.0.1 -p "$port" -U hardening -d mainframe_env \
    -v ON_ERROR_STOP=1 -Atqc "select current_setting('server_version_num')::integer / 10000"
  exit 0
fi

export MAINFRAME_ENV_TEST_POSTGRES_URL="postgres://hardening@127.0.0.1:$port/mainframe_env"
export MAINFRAME_ENV_POSTGRES_TEST_URL="$MAINFRAME_ENV_TEST_POSTGRES_URL"
out="${CARGO_TARGET_DIR:?CARGO_TARGET_DIR must be set}/ci-backend"
mkdir -p "$out"
cp "$CARGO_TARGET_DIR/ci-assurance/plan.json" "$out/plan.json"

reset_database() {
  "$bin/dropdb" -h 127.0.0.1 -p "$port" -U hardening --if-exists --force mainframe_env \
    >/dev/null 2>&1
  "$bin/createdb" -h 127.0.0.1 -p "$port" -U hardening mainframe_env
}

for gate in "${gates[@]}"; do
  reset_database
  case "$gate" in
    postgres-move)
      command=(cargo test --locked -p mainframe-env-store --test provider_move_contract \
        postgres_move_contract -- --ignored --exact)
      ;;
    postgres-effect)
      command=(cargo test --locked -p mainframe-env-store --test effect_encoding_contract \
        postgres_effect_domains_cannot_be_mixed -- --ignored --exact)
      ;;
    postgres-stale-effect-recovery)
      command=(cargo test --locked -p mainframe-env-server --lib \
        recovery_tests::postgres_stale_effect_recovery_known_success_is_not_redispatched \
        -- --ignored --exact)
      ;;
    postgres-online-resume)
      command=(cargo test --locked -p mainframe-env-server \
        --test durable_execution_recovery postgres_durable_resume_blocks_until_reconciled \
        -- --ignored --exact)
      ;;
    postgres-atomic-invariants)
      command=(cargo test --locked -p mainframe-env-store --test atomic_invariants_contract \
        postgres_atomic_invariants_contract -- --ignored --exact)
      ;;
    postgres-work-leases)
      command=(cargo test --locked -p mainframe-env-store --test work_lease_contract \
        postgres_work_deadlines_and_fencing_contract -- --ignored --exact)
      ;;
    postgres-storage-profile)
      command=(cargo test --locked -p mainframe-env-store --test postgres_storage_contract \
        postgres_quota_and_shared_artifact_contract -- --ignored --exact)
      ;;
    postgres-artifact-read-versions)
      command=(cargo test --locked -p mainframe-env-store --test postgres_storage_contract \
        postgres_artifact_read_versions_are_compatible_and_fail_closed -- --ignored --exact)
      ;;
    postgres-readiness)
      command=(cargo test --locked -p mainframe-env-store --lib \
        postgres::tests::writable_probe_requires_provider_state_dml_and_rolls_everything_back \
        -- --ignored --exact)
      ;;
    postgres-retention)
      command=(cargo test --locked -p mainframe-env-store --test retention_contract \
        postgres_retention_contract -- --ignored --exact)
      ;;
    postgres-durable)
      command=(cargo test --locked -p mainframe-env-store --lib \
        durable::tests::postgres18_migration_and_durable_contracts -- --ignored --exact)
      ;;
    postgres-carddemo-restart)
      command=(cargo test --locked -p mainframe-env-conformance --lib \
        carddemo::tests::carddemo_full_memory_sqlite_and_postgres_controls -- --ignored --exact)
      ;;
    *) echo "unknown PostgreSQL gate: $gate" >&2; exit 2 ;;
  esac
  "$python_bin" -B "$root/tools/ci_assurance.py" record --output "$out" \
    --gate "$gate" --expect-tests --min-tests 1 -- "${command[@]}"
done
"$python_bin" -B "$root/tools/ci_assurance.py" summary --plan "$out/plan.json" \
  --directory "$out" --output "$out/summary.json" --gates "${gates[@]}"
