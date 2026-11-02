#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
if [ "$#" -ne 0 ]; then
  printf 'Uso: bash scripts/build-dmg.sh\n' >&2
  exit 2
fi
bash scripts/build-app.sh --universal
VERSION="$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' packaging/Info.plist)"
CARGO_VERSION="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' Cargo.toml | head -1)"
if [ "$VERSION" != "$CARGO_VERSION" ]; then
  printf 'Versões de Cargo.toml e Info.plist divergem.\n' >&2
  exit 1
fi
NAME="AI-Monitor-$VERSION-macos-universal.dmg"
STAGE="$(mktemp -d "$ROOT/target/dmg-build.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT

# Allowlist the installer payload. Never copy the repository or local settings.
ditto --norsrc --noextattr --noqtn "$ROOT/dist/AI Monitor.app" "$STAGE/AI Monitor.app"
ln -s /Applications "$STAGE/Applications"
cp packaging/LEIA-ME.txt "$STAGE/LEIA-ME.txt"
codesign --verify --deep --strict --all-architectures "$STAGE/AI Monitor.app"
ARCHS="$(lipo -archs "$STAGE/AI Monitor.app/Contents/MacOS/ai-monitor")"
test "$ARCHS" = 'x86_64 arm64' || test "$ARCHS" = 'arm64 x86_64'

hdiutil create -volname "AI Monitor $VERSION" -srcfolder "$STAGE" \
  -fs HFS+ -format UDZO -ov "$ROOT/dist/$NAME"
hdiutil verify "$ROOT/dist/$NAME"
(cd "$ROOT/dist" && shasum -a 256 "$NAME" > "$NAME.sha256")
printf 'Instalador criado: %s\nChecksum: %s\n' "$ROOT/dist/$NAME" "$ROOT/dist/$NAME.sha256"
