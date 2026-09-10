#!/usr/bin/env bash
set -euo pipefail
export CARGO_HOME=/cache CARGO_TARGET_DIR=/target
case "${1:?check, build or maintain}" in
  check)
    rm -rf /target/ci-assurance /target/ci-backend
    /opt/mainframe-env/docker/ci.sh maintain
    python3 /opt/mainframe-env/docker/storage.py check /state /cache /target
    python3 /opt/mainframe-env/tools/supply_chain.py verify-jenkins --home "$JENKINS_HOME"
    python3 -B docker/ci_plan.py plan
    python3 -B docker/ci_plan.py check
    ;;
  build)
    python3 /opt/mainframe-env/docker/storage.py check /target /releases
    [[ "$(python3 -B docker/ci_plan.py build-required)" == true ]] || {
      echo 'The verified plan does not require a runtime build.' >&2
      exit 1
    }
    python3 -B tools/ci_assurance.py record --output /target/ci-assurance \
      --gate build-server -- cargo build --locked --release -p mainframe-env-server --bin mainframe-env-server
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
