#!/usr/bin/env bash
# Builds the engine for iOS and generates its Swift bindings.
#
#   ffi/build-ios.sh [debug|release|xcode] [OUT_DIR]
#
# Output (default OUT_DIR = ios/Engine, not committed):
#   Swift/ferrowl_ffi.swift   the generated Swift API (compiled with the app)
#   FerrowlCore.xcframework   (debug/release) static library for devices
#                             (arm64) and the simulator (arm64 + x86_64),
#                             with the C header and module map
#   lib/<platform>/, include/ (xcode) what the Xcode project links
#
# `xcode` is what the project's "Engine" target runs before the app: it
# builds only the platform and architectures Xcode is building (from
# $PLATFORM_NAME, $ARCHS and $CONFIGURATION).
#
# Needs Xcode and the Rust iOS targets:
#   rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
set -euo pipefail
cd "$(dirname "$0")/.."

# Xcode's build phases export hundreds of settings meant for the app target
# (SDKROOT, *_DEPLOYMENT_TARGET, compiler flags…); with them the C parts of
# the engine (ring, SQLite) are built against the wrong SDK. Start over
# with a clean environment, keeping only what this script reads.
if [ "${1:-}" = xcode ] && [ -z "${FERROWL_CLEAN_ENV:-}" ]; then
  exec env -i FERROWL_CLEAN_ENV=1 HOME="$HOME" PATH="/usr/bin:/bin:/usr/sbin:/sbin" \
    ${DEVELOPER_DIR:+DEVELOPER_DIR="$DEVELOPER_DIR"} \
    CONFIGURATION="${CONFIGURATION:-Debug}" PLATFORM_NAME="${PLATFORM_NAME:-iphonesimulator}" \
    ARCHS="${ARCHS:-arm64}" "$0" "$@"
fi

MODE=${1:-release}
OUT=${2:-ios/Engine}

# Xcode starts build phases with a minimal PATH: find cargo/rustup.
export PATH="$HOME/.cargo/bin:/opt/homebrew/opt/rustup/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"
command -v cargo >/dev/null || { echo "error: cargo not found: install Rust (https://rustup.rs)." >&2; exit 1; }

device=()
sim=()
if [ "$MODE" = xcode ]; then
  PROFILE=debug
  [ "${CONFIGURATION:-Debug}" = Release ] && PROFILE=release
  case "${PLATFORM_NAME:-iphonesimulator}" in
    iphoneos) device=(aarch64-apple-ios) ;;
    *)
      for a in ${ARCHS:-arm64}; do
        case "$a" in
          arm64) sim+=(aarch64-apple-ios-sim) ;;
          x86_64) sim+=(x86_64-apple-ios) ;;
        esac
      done
      ;;
  esac
else
  PROFILE=$MODE
  device=(aarch64-apple-ios)
  sim=(aarch64-apple-ios-sim x86_64-apple-ios)
fi

XCODE_PLATFORM=${PLATFORM_NAME:-iphonesimulator}
unset ARCHS PLATFORM_NAME CONFIGURATION
# Same minimum iOS version as the app (ios/Ferrowl.xcodeproj).
export IPHONEOS_DEPLOYMENT_TARGET=17.0

flag=()
dir=debug
if [ "$PROFILE" = release ]; then
  flag=(--release)
  dir=release
fi

for t in ${device[@]+"${device[@]}"} ${sim[@]+"${sim[@]}"}; do
  cargo build -p ferrowl-ffi --locked --lib --target "$t" ${flag[@]+"${flag[@]}"}
done

# Swift is generated from the host build of the same crate: release
# libraries are stripped (strip = true), which removes the UniFFI metadata.
# The interface is identical on every platform.
cargo build -q -p ferrowl-ffi --locked --lib
gen=$(mktemp -d)
trap 'rm -rf "$gen"' EXIT
cargo run -q -p ferrowl-ffi --features bindgen --bin uniffi-bindgen -- \
  generate --library target/debug/libferrowl_ffi.dylib \
  --language swift --no-format \
  --out-dir "$gen"

headers="$gen/headers"
mkdir -p "$headers"
cp "$gen/ferrowl_ffiFFI.h" "$headers/"
cp "$gen/ferrowl_ffiFFI.modulemap" "$headers/module.modulemap"

# Texts iOS shows without the engine, from core/locales, one
# Localizable.strings per language: push notifications ("loc-key" in the
# server's message) and the Shortcuts/Siri actions' names (read by the
# system from the app's files). Siri's phrases go to AppShortcuts.strings,
# keyed by the English phrase as written in AppIntents.swift.
for toml in core/locales/*.toml; do
  code=$(basename "$toml" .toml)
  [ "$code" = zh ] && code=zh-Hans
  file="ios/Ferrowl/$code.lproj/Localizable.strings"
  mkdir -p "$(dirname "$file")"
  {
    echo "/* Generated from core/locales by ffi/build-ios.sh: edit those files. */"
    grep -E '^(notif_new_mail|intent_[a-z_]+|focus_[a-z_]+) = ' "$toml" | sed -E 's/^([a-z_]+) = (".*")$/"\1" = \2;/'
  } > "$gen/strings"
  cmp -s "$gen/strings" "$file" || cp "$gen/strings" "$file"
  phrases="ios/Ferrowl/$code.lproj/AppShortcuts.strings"
  {
    echo "/* Generated from core/locales by ffi/build-ios.sh: edit those files. */"
    grep -E '^shortcut_[a-z_]+ = ' core/locales/en.toml | while IFS= read -r line; do
      key=${line%% = *}
      english=${line#* = }
      own=$(grep -E "^$key = " "$toml" | sed -E 's/^[a-z_]+ = //')
      echo "$english = ${own:-$english};"
    done | sed 's/{app}/${applicationName}/g'
  } > "$gen/phrases"
  cmp -s "$gen/phrases" "$phrases" || cp "$gen/phrases" "$phrases"
  # The widget's own texts (it runs without the engine).
  widget="ios/FerrowlWidget/$code.lproj/Localizable.strings"
  mkdir -p "$(dirname "$widget")"
  {
    echo "/* Generated from core/locales by ffi/build-ios.sh: edit those files. */"
    grep -E '^widget_[a-z_]+ = ' "$toml" | sed -E 's/^([a-z_]+) = (".*")$/"\1" = \2;/'
  } > "$gen/widget"
  cmp -s "$gen/widget" "$widget" || cp "$gen/widget" "$widget"
done

if [ "$MODE" = xcode ]; then
  # Xcode links the library of the platform being built directly
  # (Engine/lib/<platform>/), with the header and module map in
  # Engine/include: an .xcframework would be copied into the app before
  # this script runs, so the app could get the previous library.
  mkdir -p "$OUT/include" "$OUT/Swift"
  for f in ferrowl_ffiFFI.h module.modulemap; do
    cmp -s "$headers/$f" "$OUT/include/$f" || cp "$headers/$f" "$OUT/include/$f"
  done
  if [ ${#device[@]} -gt 0 ] && [ "${XCODE_PLATFORM}" = iphoneos ]; then
    lib="target/aarch64-apple-ios/$dir/libferrowl_ffi.a"
  else
    libs=()
    for t in "${sim[@]}"; do libs+=("target/$t/$dir/libferrowl_ffi.a"); done
    lipo -create "${libs[@]}" -output "$gen/libferrowl_ffi.a"
    lib="$gen/libferrowl_ffi.a"
  fi
  dest="$OUT/lib/$XCODE_PLATFORM/libferrowl_ffi.a"
  mkdir -p "$(dirname "$dest")"
  changed=0
  if ! cmp -s "$gen/ferrowl_ffi.swift" "$OUT/Swift/ferrowl_ffi.swift"; then
    cp "$gen/ferrowl_ffi.swift" "$OUT/Swift/ferrowl_ffi.swift"
    changed=1
  fi
  if ! cmp -s "$lib" "$dest"; then
    cp "$lib" "$dest"
    changed=1
  fi
  # Xcode does not watch the library: touching the Swift bindings makes it
  # recompile them and link the app again.
  [ $changed = 1 ] && touch "$OUT/Swift/ferrowl_ffi.swift"
  echo "iOS engine ready in $OUT ($XCODE_PLATFORM)"
  exit 0
fi

args=()
if [ ${#device[@]} -gt 0 ]; then
  args+=(-library "target/aarch64-apple-ios/$dir/libferrowl_ffi.a" -headers "$headers")
fi
if [ ${#sim[@]} -gt 0 ]; then
  # One library for both simulator architectures.
  libs=()
  for t in "${sim[@]}"; do libs+=("target/$t/$dir/libferrowl_ffi.a"); done
  mkdir -p "$gen/sim"
  lipo -create "${libs[@]}" -output "$gen/sim/libferrowl_ffi.a"
  args+=(-library "$gen/sim/libferrowl_ffi.a" -headers "$headers")
fi

mkdir -p "$OUT/Swift"
xcodebuild -create-xcframework "${args[@]}" -output "$gen/FerrowlCore.xcframework" >/dev/null
# Replace the framework only when it changed, so Xcode does not relink.
if ! diff -rq "$gen/FerrowlCore.xcframework" "$OUT/FerrowlCore.xcframework" >/dev/null 2>&1; then
  rm -rf "$OUT/FerrowlCore.xcframework"
  cp -R "$gen/FerrowlCore.xcframework" "$OUT/"
fi
if ! cmp -s "$gen/ferrowl_ffi.swift" "$OUT/Swift/ferrowl_ffi.swift"; then
  cp "$gen/ferrowl_ffi.swift" "$OUT/Swift/ferrowl_ffi.swift"
fi
echo "iOS engine ready in $OUT"
