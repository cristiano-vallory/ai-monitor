#!/usr/bin/env bash
# Uso: scripts/build-linux-packages.sh <x86_64|aarch64> <binary> <outdir>
# Produz ai-monitor-<v>-linux-<arch>.tar.gz e ai-monitor_<v>_<amd64|arm64>.deb + .sha256.
set -euo pipefail
arch="$1"; binary="$2"; outdir="$3"
version=$(awk -F'"' '/^version = /{print $2; exit}' Cargo.toml)
[ -f "$binary" ] || { echo "binário ausente: $binary" >&2; exit 1; }
mkdir -p "$outdir"
work=$(mktemp -d)
pkg="ai-monitor-$version-linux-$arch"
mkdir -p "$work/$pkg"
cp "$binary" "$work/$pkg/ai-monitor"
cp LICENSE "$work/$pkg/"
cp packaging/THIRD_PARTY_NOTICES-linux.txt "$work/$pkg/THIRD_PARTY_NOTICES.txt"
cp packaging/linux/ai-monitor.desktop "$work/$pkg/"
cp -r packaging/linux/icons "$work/$pkg/"
tar -C "$work" -czf "$outdir/$pkg.tar.gz" "$pkg"
debarch=$([ "$arch" = x86_64 ] && echo amd64 || echo arm64)
root="$work/deb"
mkdir -p "$root/DEBIAN" "$root/usr/bin" \
  "$root/usr/share/applications" "$root/usr/share/doc/ai-monitor"
install -m 0755 "$binary" "$root/usr/bin/ai-monitor"
install -m 0644 packaging/linux/ai-monitor.desktop \
  "$root/usr/share/applications/ai-monitor.desktop"
install -m 0644 packaging/THIRD_PARTY_NOTICES-linux.txt \
  "$root/usr/share/doc/ai-monitor/THIRD_PARTY_NOTICES.txt"
cat > "$root/usr/share/doc/ai-monitor/copyright" <<'CR'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: ai-monitor
Source: https://github.com/cristiano-vallory/ai-monitor

Files: *
Copyright: Cristiano Vallory
License: MIT
 Permission is hereby granted, free of charge, to any person obtaining a copy
 of this software and associated documentation files (the "Software"), to deal
 in the Software without restriction, including without limitation the rights
 to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 copies of the Software, and to permit persons to whom the Software is
 furnished to do so, subject to the following conditions:
 .
 The above copyright notice and this permission notice shall be included in
 all copies or substantial portions of the Software.
 .
 THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 SOFTWARE.
 .
 Licenças de terceiros: /usr/share/doc/ai-monitor/THIRD_PARTY_NOTICES.txt
CR
for size in 16 32 64 128 256 512; do
  dest="$root/usr/share/icons/hicolor/${size}x${size}/apps"
  mkdir -p "$dest"
  install -m 0644 "packaging/linux/icons/${size}x${size}/ai-monitor.png" "$dest/"
done
cat > "$root/DEBIAN/control" <<CT
Package: ai-monitor
Version: $version
Architecture: $debarch
Maintainer: Cristiano Vallory <cristianovallory@gmail.com>
Depends: libc6 (>= 2.35), libgcc-s1, libgl1, libegl1, libgl1-mesa-dri, libdrm2, libx11-6, libx11-xcb1, libxau6, libxdmcp6, libxcb1, libxcb-dri2-0, libxcb-dri3-0, libxcb-glx0, libxcb-present0, libxcb-randr0, libxcb-shm0, libxcb-sync1, libxcb-xfixes0, libxcb-xkb1, libxcursor1, libxext6, libxfixes3, libxi6, libxinerama1, libxrandr2, libxrender1, libxxf86vm1, libxkbcommon0, libxkbcommon-x11-0, libxshmfence1, libwayland-client0, libwayland-egl1, libwayland-cursor0, libwayland-egl1-mesa
Recommends: gnome-keyring
Suggests: docker.io
Section: utils
Priority: optional
Description: Monitor nativo de ai-memory e Docker para macOS e Linux
 Painel egui com bandeja de sistema (StatusNotifierItem), métricas de
 processos/contêineres e backend systemd de usuário para ai-memory.service.
CT
dpkg-deb --root-owner-group --build "$root" "$outdir/ai-monitor_${version}_${debarch}.deb"
( cd "$outdir" && sha256sum "$pkg.tar.gz" > "$pkg.tar.gz.sha256" \
  && sha256sum "ai-monitor_${version}_${debarch}.deb" > "ai-monitor_${version}_${debarch}.deb.sha256" )
rm -rf "$work"
echo "OK: $outdir/$pkg.tar.gz + ai-monitor_${version}_${debarch}.deb"
