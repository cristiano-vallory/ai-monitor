#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
CARGO_BIN="${CARGO_BIN:-$HOME/.cargo/bin/cargo}"
if [ ! -x "$CARGO_BIN" ]; then CARGO_BIN=cargo; fi

MODE="${1:-native}"
if [ "$#" -gt 1 ] || { [ "$MODE" != native ] && [ "$MODE" != --universal ]; }; then
  printf 'Uso: bash scripts/build-app.sh [--universal]\n' >&2
  exit 2
fi
NOTICE_HASH="$(sed -n 's/^Cargo.lock SHA-256: //p' packaging/THIRD_PARTY_NOTICES.txt)"
LOCK_HASH="$(shasum -a 256 Cargo.lock | awk '{print $1}')"
if [ "$NOTICE_HASH" != "$LOCK_HASH" ]; then
  printf 'Cargo.lock mudou: atualize avisos com python3 scripts/collect-licenses.py.\n' >&2
  exit 1
fi

# Do not embed developer home/workspace paths in distributed panic locations.
# Encoded flags preserve paths containing spaces without shell evaluation.
printf -v CARGO_ENCODED_RUSTFLAGS '%s\037%s\037%s' \
  "--remap-path-prefix=$ROOT=/src/ai-monitor" \
  "--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo" \
  "--remap-path-prefix=${RUSTUP_HOME:-$HOME/.rustup}=/rustup"
export CARGO_ENCODED_RUSTFLAGS
export CARGO_TARGET_DIR="$ROOT/target"
export MACOSX_DEPLOYMENT_TARGET=12.0
export CARGO_PROFILE_RELEASE_DEBUG=0
export CARGO_PROFILE_RELEASE_STRIP=symbols

if [ "$MODE" = --universal ]; then
  for TARGET in aarch64-apple-darwin x86_64-apple-darwin; do
    "$CARGO_BIN" build --release --locked --target "$TARGET"
  done
else
  # Ignore a default cross target: this mode explicitly builds for the host.
  HOST_TARGET="$("$CARGO_BIN" -vV | sed -n 's/^host: //p')"
  test -n "$HOST_TARGET"
  "$CARGO_BIN" build --release --locked --target "$HOST_TARGET"
fi

WORK="$(mktemp -d "$ROOT/target/app-build.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
APP="$WORK/AI Monitor.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
if [ "$MODE" = --universal ]; then
  lipo -create target/aarch64-apple-darwin/release/ai-monitor \
    target/x86_64-apple-darwin/release/ai-monitor -output "$APP/Contents/MacOS/ai-monitor"
else
  cp "target/$HOST_TARGET/release/ai-monitor" "$APP/Contents/MacOS/ai-monitor"
fi
cp packaging/Info.plist "$APP/Contents/Info.plist"
cp LICENSE "$APP/Contents/Resources/LICENSE.txt"
cp packaging/THIRD_PARTY_NOTICES.txt "$APP/Contents/Resources/THIRD_PARTY_NOTICES.txt"
if [ ! -f "$ROOT/target/AppIcon.icns" ]; then
  swift scripts/make-icon.swift "$ROOT/target"
  iconutil -c icns "$ROOT/target/AppIcon.iconset" -o "$ROOT/target/AppIcon.icns"
fi
cp target/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
codesign --force --sign - --timestamp=none "$APP"
codesign --verify --deep --strict --all-architectures "$APP"

# Publish only the fresh bundle; never merge old files into a release app.
# Replacing the bundle does not truncate the executable of an older running app.
mkdir -p "$ROOT/dist"
rm -rf "$ROOT/dist/AI Monitor.app"
mv "$APP" "$ROOT/dist/AI Monitor.app"
printf 'App criado: %s\n' "$ROOT/dist/AI Monitor.app"
