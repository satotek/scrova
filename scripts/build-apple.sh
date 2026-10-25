#!/usr/bin/env bash
# Builds ScrovaFFI.xcframework and the UniFFI Swift sources for apps/apple/ScrovaKit.
# Run inside the project dev shell: `direnv exec . scripts/build-apple.sh [--debug]`.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

profile=release
cargo_profile=(--release)
if [[ "${1:-}" == "--debug" ]]; then
  profile=debug
  cargo_profile=()
fi

# Keep in sync with the deployment targets in apps/apple/ScrovaKit/Package.swift.
export MACOSX_DEPLOYMENT_TARGET=14.0
export IPHONEOS_DEPLOYMENT_TARGET=17.0

targets=(aarch64-apple-darwin x86_64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim)
for target in "${targets[@]}"; do
  cargo build -p scrova-ffi --lib "${cargo_profile[@]}" --target "$target"
done

lib=libscrova_ffi.a
out="$root/target/apple"
kit="$root/apps/apple/ScrovaKit"
rm -rf "$out" "$kit/ScrovaFFI.xcframework"
mkdir -p "$out/macos" "$out/headers"

lipo -create \
  "target/aarch64-apple-darwin/$profile/$lib" \
  "target/x86_64-apple-darwin/$profile/$lib" \
  -output "$out/macos/$lib"

# Library mode reads the UniFFI metadata embedded in the built library, so any
# one slice works as input.
bindgen=(cargo run -q -p uniffi-bindgen-swift --)
input="target/aarch64-apple-darwin/$profile/$lib"
"${bindgen[@]}" "$input" "$kit/Sources/ScrovaKit/Generated" --swift-sources
"${bindgen[@]}" "$input" "$out/headers" --headers
"${bindgen[@]}" "$input" "$out/headers" --modulemap --module-name ScrovaFFI --modulemap-filename module.modulemap

xcodebuild -create-xcframework \
  -library "$out/macos/$lib" -headers "$out/headers" \
  -library "target/aarch64-apple-ios/$profile/$lib" -headers "$out/headers" \
  -library "target/aarch64-apple-ios-sim/$profile/$lib" -headers "$out/headers" \
  -output "$kit/ScrovaFFI.xcframework"
