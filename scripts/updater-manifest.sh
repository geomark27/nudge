#!/usr/bin/env bash
# Genera latest.json, el manifiesto que consulta el actualizador de Nudge, a partir de las firmas
# .sig del release, y lo sube al mismo release. Lo ejecuta el CI después de compilar.
# Necesita gh con GH_REPO y GH_TOKEN, y jq.  Uso: scripts/updater-manifest.sh v0.1.0
#
# El actualizador busca primero "<so>-<arq>-<instalador>" (p. ej. linux-x86_64-deb, según cómo se
# instaló Nudge) y si no existe, "<so>-<arq>".
set -euo pipefail

tag=${1:?uso: scripts/updater-manifest.sh vX.Y.Z}
version=${tag#v}
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT

gh release download "$tag" -p '*.sig' -D "$dir"
base="https://github.com/$GH_REPO/releases/download/$tag"
platforms='{}'

# add <clave> <instalador>: el instalador y su firma deben estar en el release.
add() {
  platforms=$(jq --arg key "$1" --arg url "$base/$2" --arg sig "$(tr -d '\r\n' < "$dir/$2.sig")" \
    '. + {($key): {url: $url, signature: $sig}}' <<<"$platforms")
}

for sig in "$dir"/*.sig; do
  file=$(basename "$sig" .sig)
  case $file in
    *-setup.exe) add windows-x86_64-nsis "$file"; add windows-x86_64 "$file" ;;
    *.msi) add windows-x86_64-msi "$file" ;;
    *.AppImage) add linux-x86_64-appimage "$file"; add linux-x86_64 "$file" ;;
    *.deb) add linux-x86_64-deb "$file" ;;
    *.rpm) add linux-x86_64-rpm "$file" ;;
    # El .app de macOS es universal: sirve para Apple Silicon e Intel.
    *.app.tar.gz) for arch in aarch64 x86_64; do add "darwin-$arch-app" "$file"; add "darwin-$arch" "$file"; done ;;
  esac
done

[[ $(jq length <<<"$platforms") -gt 0 ]] || { echo "el release $tag no tiene firmas .sig" >&2; exit 1; }

notes=$(bash "$(dirname "$0")/changelog-notes.sh" "$version")
jq -n --arg version "$version" --arg notes "$notes" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --argjson platforms "$platforms" \
  '{version: $version, notes: $notes, pub_date: $date, platforms: $platforms}' > "$dir/latest.json"

echo "latest.json para $tag:"
jq -r '.platforms | keys[] | "  " + .' "$dir/latest.json"
gh release upload "$tag" "$dir/latest.json" --clobber
