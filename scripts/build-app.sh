#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
CARGO_BIN="${CARGO_BIN:-$HOME/.cargo/bin/cargo}"
if [ ! -x "$CARGO_BIN" ]; then CARGO_BIN=cargo; fi
"$CARGO_BIN" build --release --locked
APP="$ROOT/dist/AI Monitor.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/ai-monitor "$APP/Contents/MacOS/ai-monitor.next"
# Atomic binary replacement preserves the old executable's mappings until restart.
mv -f "$APP/Contents/MacOS/ai-monitor.next" "$APP/Contents/MacOS/ai-monitor"
cp packaging/Info.plist "$APP/Contents/Info.plist"
if [ ! -f "$ROOT/target/AppIcon.icns" ]; then
  swift scripts/make-icon.swift "$ROOT/target"
  iconutil -c icns "$ROOT/target/AppIcon.iconset" -o "$ROOT/target/AppIcon.icns"
fi
cp target/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"
printf 'App criado: %s\n' "$APP"
