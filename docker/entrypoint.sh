#!/usr/bin/env bash
set -euo pipefail
mkdir -p /cache /target
dev_uid="${DEV_UID:-1000}"
dev_gid="${DEV_GID:-1000}"
mkdir -p /cache/home /ibm-docs/topic-cache
dev_home=/cache/home
if [[ "${MAINFRAME_ENV_DEV_CONTAINER:-0}" == 1 ]]; then
  dev_home=/home/developer
  usermod --uid "$dev_uid" --gid "$dev_gid" developer
fi
mkdir -p "$dev_home"
chown "$dev_uid:$dev_gid" /cache /cache/home /target /workspace/dist /workspace/.postgres /ibm-docs /ibm-docs/topic-cache "$dev_home"
export HOME="$dev_home"
export CARGO_HOME=/cache CARGO_TARGET_DIR=/target
# Separate development and CI cache volumes; serialize writers to each cache.
exec 9>/cache/build.lock
check_storage() {
  local used
  used="$(df -P /target | awk 'NR==2 {gsub(/%/, "", $5); print $5}')"
  if (( used >= 75 )); then
    python3 /opt/mainframe-env/docker/storage.py clean /cache /target
  fi
  if [[ "${3:-}" != clean ]]; then
    python3 /opt/mainframe-env/docker/storage.py check /cache /target
  fi
}
if [[ "${MAINFRAME_ENV_DEV_CONTAINER:-0}" == 1 ]]; then
  # The IDE is long-lived. Lock only startup here; its Cargo wrapper locks work.
  flock 9
  check_storage "$@"
  flock -u 9
else
  flock 9
  check_storage "$@"
fi
exec setpriv --reuid="$dev_uid" --regid="$dev_gid" --clear-groups "$@"
