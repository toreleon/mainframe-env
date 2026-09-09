#!/usr/bin/env bash
# Build the exact mainframe-env release binaries with the canonical,
# reproducible release environment.

set -euo pipefail

release_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
release_target=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) release_target="${2:?--target needs a triple}"; shift 2 ;;
    -h|--help) sed -n '2,4p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

case "$release_target" in
  aarch64-apple-darwin|x86_64-apple-darwin|aarch64-unknown-linux-gnu|x86_64-unknown-linux-gnu) ;;
  *) echo "unsupported release target: $release_target" >&2; exit 2 ;;
esac

release_rustflags="--remap-path-prefix=${release_root}=/workspace/mainframe-env"
release_rustflags+=" --remap-path-prefix=${CARGO_HOME:-${HOME:?HOME is required}/.cargo}=/cargo"
release_rustflags+=" --remap-path-prefix=${RUSTUP_HOME:-${HOME:?HOME is required}/.rustup}=/rustup"
case "$release_target" in
  # Modern dyld requires LC_UUID. ld64 derives the UUID from the linked
  # content, so retaining it is both launchable and reproducible.
  *apple-darwin) export MACOSX_DEPLOYMENT_TARGET=15.0 ;;
  *linux-gnu) release_rustflags+=" -C link-arg=-Wl,--build-id=sha1" ;;
esac

unset CARGO_ENCODED_RUSTFLAGS
cd "$release_root"
CARGO_INCREMENTAL=0 SOURCE_DATE_EPOCH=0 ZERO_AR_DATE=1 LC_ALL=C TZ=UTC \
RUSTFLAGS="$release_rustflags" \
  cargo build --release --locked --all-features --target "$release_target" \
    -p mainframe-env-server -p mainframe-env-cli
