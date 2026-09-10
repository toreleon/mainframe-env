#!/usr/bin/env bash
# A separate VM and its caches live inside one 44 GiB APFS image.
set -euo pipefail
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
volume=/Volumes/MainframeEnvDocker
bundle="${MAINFRAME_ENV_DOCKER_BUNDLE:-$HOME/.local/share/mainframe-env/docker.sparsebundle}"
[[ "$(uname -s)" == Darwin ]] || { echo 'This bootstrap requires macOS.' >&2; exit 1; }
command -v colima >/dev/null || { echo 'Install Colima and Lima first (see docker/README.md).' >&2; exit 1; }
if [[ ! -d "$bundle" ]]; then
  mkdir -p "$(dirname "$bundle")"
  hdiutil create -size 44g -type SPARSEBUNDLE -fs APFS -volname MainframeEnvDocker "$bundle"
fi
if ! mount | grep -Fq " on $volume ("; then
  hdiutil attach "$bundle" -mountpoint "$volume" -nobrowse
fi
python3 "$repo/docker/storage.py" host "$volume"
export COLIMA_HOME="$volume/colima"
export XDG_CACHE_HOME="$volume/cache"
export TMPDIR="$volume/tmp"
mkdir -p "$COLIMA_HOME" "$XDG_CACHE_HOME" "$TMPDIR" "$volume/secrets"
chmod 700 "$volume/secrets"
python3 "$repo/docker/storage.py" secrets "$volume/secrets"
python3 "$repo/docker/git-credentials.py" "$volume/secrets/github_credentials"
# Cache the verified controller/plugin downloads outside disposable build layers.
if ! python3 "$repo/tools/supply_chain.py" verify-jenkins --home "$volume/jenkins-seed" >/dev/null 2>&1; then
  python3 "$repo/tools/supply_chain.py" install-jenkins --home "$volume/jenkins-seed"
fi
# Keep the root disk, Docker data disk, downloads and logs within the outer cap.
# Explicit mounts replace Colima's default whole-home writable mount.
colima start mainframe-env --activate=false --runtime docker --vm-type vz \
  --cpus 4 --memory 8 --disk 32 --root-disk 6 --mount-type virtiofs \
  --mount "$repo:w" --mount "$volume/secrets" --ssh-agent=false --ssh-config=false
python3 "$repo/docker/storage.py" host "$volume"
printf 'Docker VM ready. Run: %s/docker/dev up\n' "$repo"
