#!/usr/bin/env bash
# Publica a release <tag> com os ativos de <dist-dir>. DRY_RUN=1 só imprime.
set -euo pipefail
tag="$1"; distdir="$2"
v=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[ "$tag" = "v$v" ] || { echo "tag $tag != versão do Cargo.toml v$v" >&2; exit 1; }
[ -f "docs/releases/v$v.md" ] || { echo "notas ausentes: docs/releases/v$v.md" >&2; exit 1; }
# título = cabeçalho "# …" da 1ª linha das notas, que precisa citar a versão
title=$(sed -n '1s/^# //p' "docs/releases/v$v.md")
case "$title" in *"v$v"*) ;; *) echo "1ª linha de docs/releases/v$v.md deve ser '# … v$v …': '$title'" >&2; exit 1;; esac
shopt -s nullglob
files=(
  "$distdir"/ai-monitor-macos/*.dmg
  "$distdir"/ai-monitor-macos/*.dmg.sha256
  "$distdir"/ai-monitor-linux-*/ai-monitor-*-linux-*.tar.gz
  "$distdir"/ai-monitor-linux-*/ai-monitor-*-linux-*.tar.gz.sha256
  "$distdir"/ai-monitor-linux-*/ai-monitor_*_*.deb
  "$distdir"/ai-monitor-linux-*/ai-monitor_*_*.deb.sha256
)
# exatamente 10 + PKGBUILD, sem nomes duplicados
[ "${#files[@]}" -eq 10 ] || { echo "esperados 10 ativos, encontrados ${#files[@]}: ${files[*]}" >&2; exit 1; }
extra=$(find "$distdir" -type f | wc -l)
[ "$extra" -eq 10 ] || { echo "há $extra arquivos em $distdir; esperados exatamente 10" >&2; exit 1; }
dup=$(printf '%s\n' "${files[@]##*/}" | sort | uniq -d | head -1)
[ -z "$dup" ] || { echo "ativo duplicado por nome: $dup" >&2; exit 1; }
for f in "${files[@]}"; do
  case "$f" in *.sha256) ( cd "$(dirname "$f")" && sha256sum -c "$(basename "$f")" >/dev/null ) || exit 1;; esac
done
bash scripts/render-pkgbuild.sh "$v" \
  "$(awk '{print $1}' "$distdir"/ai-monitor-linux-x86_64/ai-monitor-*-linux-x86_64.tar.gz.sha256)" \
  "$(awk '{print $1}' "$distdir"/ai-monitor-linux-aarch64/ai-monitor-*-linux-aarch64.tar.gz.sha256)"
files+=(packaging/arch/PKGBUILD)
create=(gh release create "$tag" --draft --title "$title" --notes-file "docs/releases/v$v.md" "${files[@]}")
edit=(gh release edit "$tag" --draft=false)
if [ "${DRY_RUN:-0}" = 1 ]; then
  echo "DRY: ${create[*]}"; echo "DRY: ${edit[*]}"; exit 0
fi
"${create[@]}"
"${edit[@]}"
