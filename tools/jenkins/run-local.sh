#!/usr/bin/env bash
# Run Jenkins LTS in the foreground with JENKINS_HOME on the capped volume.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
python_bin="$("$repo_root/tools/jenkins/select-python.sh")"
volume="${MAINFRAME_ENV_JENKINS_VOLUME:-/Volumes/MainframeEnvJenkins}"
port="${JENKINS_PORT:-8080}"
java_bin="${MAINFRAME_ENV_JAVA:-}"

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
if [[ -z "$java_bin" ]]; then
  java_candidates=()
  [[ -n "${HOME:-}" ]] \
    && java_candidates+=("$HOME/.homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home/bin/java")
  java_candidates+=(
    /opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home/bin/java
    /usr/local/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home/bin/java
    /usr/bin/java
  )
  for candidate in "${java_candidates[@]}"; do
    if [[ -x "$candidate" ]] && "$candidate" -version >/dev/null 2>&1; then
      java_bin="$candidate"
      break
    fi
  done
fi
[[ -n "$java_bin" && -x "$java_bin" ]] || {
  echo "the locked Java runtime is missing; set MAINFRAME_ENV_JAVA" >&2
  exit 1
}

export MAINFRAME_ENV_JENKINS_VOLUME="$volume"
export JENKINS_HOME="$volume/jenkins-home"
export CARGO_HOME="$volume/cargo-home"
export TMPDIR="$volume/tmp/controller"
export RUSTUP_AUTO_INSTALL=0
export MAINFRAME_ENV_PYTHON="$python_bin"
export MAINFRAME_ENV_JAVA="$java_bin"
export PATH="$(dirname "$python_bin"):$PATH"
export JAVA_TOOL_OPTIONS="${JAVA_TOOL_OPTIONS:+$JAVA_TOOL_OPTIONS }-Djava.io.tmpdir=$TMPDIR -Dhudson.plugins.git.GitSCM.ALLOW_LOCAL_CHECKOUT=true"
init_dir="$JENKINS_HOME/init.groovy.d"
mkdir -p "$JENKINS_HOME" "$CARGO_HOME" "$TMPDIR" "$init_dir"
install -m 0644 "$repo_root/tools/jenkins/init-capped-controller.groovy" \
  "$init_dir/10-mainframe-env-capped-controller.groovy"
"$MAINFRAME_ENV_PYTHON" -B "$repo_root/tools/supply_chain.py" check \
  --runtime controller --jenkins-home "$JENKINS_HOME"
"$MAINFRAME_ENV_PYTHON" -B "$repo_root/tools/jenkins/disk_guard.py" verify \
  --root "$volume" \
  --require "JENKINS_HOME=$JENKINS_HOME" \
  --require "CARGO_HOME=$CARGO_HOME" \
  --require "TMPDIR=$TMPDIR"

exec "$MAINFRAME_ENV_JAVA" -jar "$JENKINS_HOME/controller/jenkins.war" \
  --httpListenAddress=127.0.0.1 --httpPort="$port"
