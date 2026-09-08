#!/usr/bin/env bash
# Create or mount the 10 GiB APFS sparse bundle used by local Jenkins.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
image="${MAINFRAME_ENV_JENKINS_IMAGE:-$HOME/Library/Application Support/mainframe-env/jenkins-10gb.sparsebundle}"
volume="${MAINFRAME_ENV_JENKINS_VOLUME:-/Volumes/MainframeEnvJenkins}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --image) image="${2:?--image needs a path}"; shift 2 ;;
    --volume) volume="${2:?--volume needs a path}"; shift 2 ;;
    -h|--help)
      sed -n '2,20p' "${BASH_SOURCE[0]}"
      exit 0
      ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

[[ "$volume" == /* ]] || { echo "--volume must be absolute" >&2; exit 2; }
[[ "$image" == /* ]] || { echo "--image must be absolute" >&2; exit 2; }
command -v hdiutil >/dev/null || { echo "hdiutil is required on macOS" >&2; exit 1; }

if [[ ! -e "$image" ]]; then
  mkdir -p "$(dirname "$image")"
  hdiutil create -quiet -size 10g -type SPARSEBUNDLE -fs APFS \
    -volname "$(basename "$volume")" "$image"
fi

if ! mount | grep -Fq " on $volume ("; then
  hdiutil attach -quiet "$image" -mountpoint "$volume"
fi

mkdir -p "$volume/jenkins-home" "$volume/cargo-home" "$volume/tmp/controller"
python3 -B "$repo_root/tools/jenkins/disk_guard.py" verify \
  --root "$volume" \
  --require "JENKINS_HOME=$volume/jenkins-home" \
  --require "CARGO_HOME=$volume/cargo-home" \
  --require "TMPDIR=$volume/tmp/controller" \
  --minimum-free-bytes 0

printf '\nCapped Jenkins volume is ready at %s.\n' "$volume"
printf 'Start the controller with: tools/jenkins/run-local.sh --volume %q\n' "$volume"
