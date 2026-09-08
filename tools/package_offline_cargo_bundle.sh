#!/usr/bin/env bash
# Package and verify the locked Cargo sources for an existing release tag.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
python_bin="${MAINFRAME_ENV_PYTHON:-$("$root/tools/jenkins/select-python.sh")}"
out="$root/dist"
tag=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag) tag="${2:?--tag needs a tag}"; shift 2 ;;
    --out) out="${2:?--out needs a directory}"; shift 2 ;;
    -h|--help) sed -n '2,14p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

[[ "$tag" =~ ^mainframe-env-v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] \
  || { echo "--tag must be a mainframe-env-vX.Y.Z release tag" >&2; exit 2; }
version="${tag#mainframe-env-v}"
actual_version="$(tr -d '[:space:]' < "$root/VERSION")"
[[ "$actual_version" == "$version" ]] \
  || { echo "VERSION mismatch: tag=$version repository=$actual_version" >&2; exit 1; }

head="$(git -C "$root" rev-parse HEAD)"
tag_commit="$(git -C "$root" rev-parse --verify "refs/tags/$tag^{commit}")"
[[ "$head" == "$tag_commit" ]] \
  || { echo "release bundle requires HEAD at $tag ($tag_commit), got $head" >&2; exit 1; }
[[ -z "$(git -C "$root" status --porcelain --untracked-files=normal)" ]] \
  || { echo "release bundle requires a clean tagged source tree" >&2; exit 1; }
"$python_bin" -B "$root/tools/supply_chain.py" check --runtime offline

target_root="${CARGO_TARGET_DIR:-$root/target}"
mkdir -p "$target_root" "$out"
stage="$(mktemp -d "$target_root/offline-release.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
sdk="$stage/offline-sdk"
mkdir -p "$sdk/.cargo"

echo "==> vendoring locked Cargo sources for $tag"
cargo vendor --locked "$sdk/vendor" > "$sdk/.cargo/config.toml"
sed -i.bak 's|^directory = .*|directory = "vendor"|' "$sdk/.cargo/config.toml"
rm -f "$sdk/.cargo/config.toml.bak"
grep -q '^directory = "vendor"$' "$sdk/.cargo/config.toml" \
  || { echo "could not make the vendor path relocatable" >&2; exit 1; }
cp "$root/Cargo.lock" "$sdk/Cargo.lock"
cp "$root/Cargo.toml" "$sdk/Cargo.toml"
cp "$root/LICENSE" "$sdk/LICENSE"
cp "$root/NOTICE" "$sdk/NOTICE"
mkdir -p "$sdk/LICENSES"
cp "$root/LICENSES/ICU.txt" "$sdk/LICENSES/ICU.txt"
mkdir -p "$sdk/SUPPLY-CHAIN"
cp "$root/tools/ci-inputs.lock.json" "$sdk/SUPPLY-CHAIN/ci-inputs.lock.json"
cp "$root/tools/jenkins/controller-plugins.lock.json" \
  "$sdk/SUPPLY-CHAIN/jenkins-controller-plugins.lock.json"

"$python_bin" -B "$root/tools/supply_chain.py" record-offline \
  --vendor "$sdk/vendor" --output "$sdk/SUPPLY-CHAIN/BUILD-INPUTS.json"
"$python_bin" -B "$root/tools/supply_chain.py" verify-offline \
  --vendor "$sdk/vendor" --record "$sdk/SUPPLY-CHAIN/BUILD-INPUTS.json"

cat > "$sdk/README.md" <<'EOF'
# mainframe-env offline Cargo dependency bundle

Extract this archive into the root of the matching tagged source checkout. It
contains the exact crates selected by `Cargo.lock`; build and test with
`cargo build --workspace --all-features --locked --offline` and
`cargo test --workspace --all-features --locked --offline`.

The Rust compiler and source checkout are not included.

The mainframe-env Apache-2.0 license, project NOTICE, and the complete retained
ICU text for the locked decNumber dependency are included at the archive root.
Each vendored crate retains its own complete license and notice files.

`SUPPLY-CHAIN/BUILD-INPUTS.json` binds the source revision, locked input files,
vendored tree, and exact tool executables used to assemble this archive. The
reviewed controller/plugin and CI input locks are retained beside it. The final
archive is reproduced from two clean staging copies in the digest-pinned GNU
tar environment before its immutable local path is accepted.
EOF

echo "==> verifying a clean checkout with networking disabled in Cargo"
verify="$stage/verify"
git clone --quiet --no-local "$root" "$verify"
git -C "$verify" checkout --quiet --detach "$head"
rm -rf "$verify/.cargo" "$verify/vendor"
cp -R "$sdk/.cargo" "$verify/.cargo"
cp -R "$sdk/vendor" "$verify/vendor"
CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$stage/verify-target" \
  cargo build --manifest-path "$verify/Cargo.toml" \
    --workspace --all-features --locked --offline

archive="$out/mainframe-env-${version}-cargo-vendor.tar.gz"
echo "==> packaging $archive"
"$python_bin" -B "$root/tools/reproducible_archive.py" \
  --source "$sdk" --output "$archive"
printf 'bundle: %s\n' "$archive"
