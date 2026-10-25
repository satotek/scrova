#!/usr/bin/env bash
# Builds the Rust cdylib and generated Kotlin bindings for apps/android.
# Run inside the project dev shell: `direnv exec . scripts/build-android.sh [--debug]`.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

profile=release
cargo_profile=(--release)
if [[ "${1:-}" == "--debug" ]]; then
  profile=debug
  cargo_profile=()
fi

jni="$root/apps/android/app/src/main/jniLibs"
kotlin="$root/apps/android/app/src/main/generated/kotlin"
lib=libscrova_ffi.so
abis=(arm64-v8a x86_64)

# The script owns these trees; clearing them prevents stale Kotlin or a stale
# ABI from surviving a failed/partial previous build.
rm -rf "$jni" "$kotlin"
mkdir -p "$jni" "$kotlin"

# API 26 matches the app's minSdk. Keeping it explicit avoids cargo-ndk's
# default (21), which would advertise a lower minimum than the app supports.
cargo ndk \
  -t arm64-v8a \
  -t x86_64 \
  -P 26 \
  -o "$jni" \
  build -p scrova-ffi --lib "${cargo_profile[@]}"

for abi in "${abis[@]}"; do
  if [[ ! -f "$jni/$abi/$lib" ]]; then
    echo "missing $jni/$abi/$lib" >&2
    exit 1
  fi
done

# Library mode reads the UniFFI metadata embedded in the cdylib. Generate
# before stripping so the metadata is untouched.
# --no-format avoids making ktlint an undeclared build dependency.
cargo run -q -p uniffi-bindgen -- generate \
  --language kotlin \
  --no-format \
  --out-dir "$kotlin" \
  "$jni/arm64-v8a/$lib"

if ! find "$kotlin" -type f -name '*.kt' | grep -q .; then
  echo "no Kotlin bindings were generated in $kotlin" >&2
  exit 1
fi

if [[ "$profile" == release ]]; then
  strip_tool="$(find "${ANDROID_NDK_HOME:?ANDROID_NDK_HOME is required}" \
    -path '*/toolchains/llvm/prebuilt/*/bin/llvm-strip' | head -n 1)"
  if [[ -z "$strip_tool" || ! -x "$strip_tool" ]]; then
    echo "llvm-strip not found under $ANDROID_NDK_HOME" >&2
    exit 1
  fi
  "$strip_tool" --strip-unneeded \
    "$jni/arm64-v8a/$lib" \
    "$jni/x86_64/$lib"
fi
