#!/usr/bin/env bash
set -euo pipefail
export CARGO_HOME=/cache CARGO_TARGET_DIR=/target
case "${1:?check, build or maintain}" in
  check)
    /opt/mainframe-env/docker/ci.sh maintain
    python3 /opt/mainframe-env/docker/storage.py check /state /cache /target
    python3 /opt/mainframe-env/tools/supply_chain.py verify-jenkins --home "$JENKINS_HOME"
    rm -rf /target/ci-assurance /target/ci-backend
    cargo fmt --all -- --check
    python3 -B tools/supply_chain.py check
    cargo deny check
    python3 -B tools/run_tooling_tests.py
    cargo +1.95.0 check --workspace --all-targets --all-features --locked
    /opt/mainframe-env/docker/test-workspace.sh
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
    python3 -B tools/ci_assurance.py plan --event manual --ref refs/heads/main \
      --output /target/ci-assurance/plan.json
    tools/jenkins/postgres_parity.sh run
    ;;
  build)
    python3 /opt/mainframe-env/docker/storage.py check /target /releases
    cargo build --locked --release -p mainframe-env-server --bin mainframe-env-server
    ;;
  maintain)
    # Only remove disposable compiler output, and only between builds under flock.
    used="$(df -P /target | awk 'NR==2 {gsub(/%/, "", $5); print $5}')"
    if (( used >= 75 )); then
      python3 /opt/mainframe-env/docker/storage.py clean /cache /target
    fi
    ;;
  *) exit 2 ;;
esac
