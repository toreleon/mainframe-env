#!/usr/bin/env bash
# Run Jenkins LTS in the foreground with JENKINS_HOME on the capped volume.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
python_bin="$("$repo_root/tools/jenkins/select-python.sh")"
volume="${MAINFRAME_ENV_JENKINS_VOLUME:-/Volumes/MainframeEnvJenkins}"
port="${JENKINS_PORT:-8080}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --volume) volume="${2:?--volume needs a path}"; shift 2 ;;
    --port) port="${2:?--port needs a number}"; shift 2 ;;
    -h|--help)
      sed -n '2,16p' "${BASH_SOURCE[0]}"
      exit 0
      ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

[[ "$port" =~ ^[0-9]+$ ]] && (( port >= 1 && port <= 65535 )) \
  || { echo "invalid Jenkins port: $port" >&2; exit 2; }
command -v jenkins-lts >/dev/null || {
  echo "jenkins-lts is missing; install it with: brew install jenkins-lts" >&2
  exit 1
}

export MAINFRAME_ENV_JENKINS_VOLUME="$volume"
export JENKINS_HOME="$volume/jenkins-home"
export CARGO_HOME="$volume/cargo-home"
export TMPDIR="$volume/tmp/controller"
export RUSTUP_AUTO_INSTALL=0
export MAINFRAME_ENV_PYTHON="$python_bin"
export PATH="$(dirname "$python_bin"):$PATH"
export JAVA_TOOL_OPTIONS="${JAVA_TOOL_OPTIONS:+$JAVA_TOOL_OPTIONS }-Djava.io.tmpdir=$TMPDIR -Dhudson.plugins.git.GitSCM.ALLOW_LOCAL_CHECKOUT=true"
init_dir="$JENKINS_HOME/init.groovy.d"
mkdir -p "$JENKINS_HOME" "$CARGO_HOME" "$TMPDIR" "$init_dir"
install -m 0644 "$repo_root/tools/jenkins/init-capped-controller.groovy" \
  "$init_dir/10-mainframe-env-capped-controller.groovy"
"$MAINFRAME_ENV_PYTHON" -B "$repo_root/tools/jenkins/disk_guard.py" verify \
  --root "$volume" \
  --require "JENKINS_HOME=$JENKINS_HOME" \
  --require "CARGO_HOME=$CARGO_HOME" \
  --require "TMPDIR=$TMPDIR"

exec jenkins-lts --httpListenAddress=127.0.0.1 --httpPort="$port"
