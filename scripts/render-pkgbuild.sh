#!/usr/bin/env bash
# Uso: scripts/render-pkgbuild.sh <version> <sha_x86_64> <sha_aarch64> [<source-base>]
set -euo pipefail
version="$1"; sx="$2"; sa="$3"
base="${4:-https://github.com/cristiano-vallory/ai-monitor/releases/download/v$version}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-a-zA-Z0-9.]*)?$ ]] || { echo "versão inválida: $version" >&2; exit 1; }
for s in "$sx" "$sa"; do
  [[ "$s" =~ ^[0-9a-f]{64}$ ]] || { echo "sha256 inválido (64 hex minúsculos): $s" >&2; exit 1; }
done
sed -e "s/@VERSION@/$version/g" -e "s|@SOURCE_BASE@|$base|g" \
    -e "s/@SHA_X86_64@/$sx/g" -e "s/@SHA_AARCH64@/$sa/g" \
    packaging/arch/PKGBUILD.in > packaging/arch/PKGBUILD
echo "rendered packaging/arch/PKGBUILD ($base)"
