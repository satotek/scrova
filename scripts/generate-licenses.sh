#!/usr/bin/env bash
# Regenerates the Rust third-party license list bundled into both apps.
# Run inside the project dev shell: `direnv exec . scripts/generate-licenses.sh`.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

out="$root/apps/common/licenses-rust.json"
mkdir -p "$(dirname "$out")"

# The app ships two Android ABIs, universal macOS, iOS device, and the arm64
# simulator. Target-specific dependencies differ, so union the normal graph for
# each target instead of listing crates for the host machine.
targets=(
  aarch64-linux-android
  x86_64-linux-android
  aarch64-apple-darwin
  x86_64-apple-darwin
  aarch64-apple-ios
  aarch64-apple-ios-sim
)

tmp="$(mktemp -d "${TMPDIR:-/tmp}/scrova-licenses.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT

cargo metadata --format-version 1 --locked >"$tmp/metadata.json"

# Resolve each target separately: Cargo's workspace-wide metadata resolve also
# enables features from the bindgen CLI crate, which would incorrectly add
# build-time-only crates such as clap. `cargo tree -p scrova-ffi` reflects what
# the shipped library actually links.
for target in "${targets[@]}"; do
  cargo tree -p scrova-ffi -e normal --target "$target" \
    --prefix none --format '{p}' \
    | sed -E 's/ \([^)]*\)$//' \
    >"$tmp/$target.txt"
done
sort -u "$tmp"/*.txt >"$tmp/labels.txt"

# Cargo metadata is the source of license and repository fields; the labels
# file is only the reachability filter.
jq -n --slurpfile metadata "$tmp/metadata.json" --rawfile labels "$tmp/labels.txt" '
  ($labels | split("\n") | map(select(length > 0))) as $wanted
  | $metadata[0].packages
  | map(
      select(.source != null)
      | . as $package
      | select($wanted | index("\($package.name) v\($package.version)") != null)
      | {
          name: .name,
          version: .version,
          license: (.license // ""),
          repository: (.repository // "")
        }
    )
  | unique_by([.name, .version])
  | sort_by([.name, .version])
' >"$out"

count="$(jq 'length' "$out")"
printf 'wrote %s entries to %s\n' "$count" "$out"
