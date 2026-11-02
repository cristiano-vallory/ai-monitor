#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
VERSION="$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' packaging/Info.plist)"
NAME="AI-Monitor-$VERSION-macos-universal.dmg"
DMG="${1:-$ROOT/dist/$NAME}"
mkdir -p "$ROOT/target"
WORK="$(mktemp -d "$ROOT/target/dmg-verify.XXXXXX")"
MOUNT="$WORK/volume"
MOUNTED=0
cleanup() {
  if [ "$MOUNTED" = 1 ] && ! hdiutil detach "$MOUNT" -quiet; then
    printf 'Volume ainda montado; preservando %s para ejeção manual.\n' "$WORK" >&2
    return
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT

hdiutil verify "$DMG"
hdiutil attach "$DMG" -readonly -nobrowse -noautoopen -mountpoint "$MOUNT" -quiet
MOUNTED=1
test "$(readlink "$MOUNT/Applications")" = /Applications
test -f "$MOUNT/LEIA-ME.txt"
test "$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$MOUNT/AI Monitor.app/Contents/Info.plist")" = "$VERSION"
codesign --verify --deep --strict --all-architectures "$MOUNT/AI Monitor.app"
ARCHS="$(lipo -archs "$MOUNT/AI Monitor.app/Contents/MacOS/ai-monitor")"
test "$ARCHS" = 'x86_64 arm64' || test "$ARCHS" = 'arm64 x86_64'

# Simulate the Finder copy into a clean destination. No user app is replaced.
mkdir "$WORK/Applications"
ditto "$MOUNT/AI Monitor.app" "$WORK/Applications/AI Monitor.app"
codesign --verify --deep --strict --all-architectures "$WORK/Applications/AI Monitor.app"
"$WORK/Applications/AI Monitor.app/Contents/MacOS/ai-monitor" --version
hdiutil detach "$MOUNT" -quiet
MOUNTED=0
# The installed app must also work after ejecting the image.
"$WORK/Applications/AI Monitor.app/Contents/MacOS/ai-monitor" --version
printf 'DMG validado: assinatura, arquiteturas, cópia e execução após ejetar.\n'
