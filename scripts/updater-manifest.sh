#!/usr/bin/env bash
# Genera latest.json, el manifiesto que consulta el actualizador de Nudge, a partir de las firmas
# .sig que deja cada compilación (<instalador>.sig, con el mismo nombre fijo que en el release), y
# lo sube al release. Lo ejecuta el CI después de compilar.
# Necesita gh con GH_REPO y GH_TOKEN, y jq.  Uso: scripts/updater-manifest.sh v0.1.0 <carpeta-con-.sig>
#
# El actualizador busca primero "<so>-<arq>-<instalador>" (p. ej. linux-x86_64-deb, según cómo se
# instaló Nudge) y si no existe, "<so>-<arq>".
set -euo pipefail

tag=${1:?uso: scripts/updater-manifest.sh vX.Y.Z <carpeta-con-.sig>}
sigs=${2:?uso: scripts/updater-manifest.sh vX.Y.Z <carpeta-con-.sig>}
version=${tag#v}
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT

base="https://github.com/$GH_REPO/releases/download/$tag"
platforms='{}'

# add <clave> <instalador>: el instalador debe estar en el release y su firma en $sigs.
add() {
  platforms=$(jq --arg key "$1" --arg url "$base/$2" --arg sig "$(tr -d '\r\n' < "$sigs/$2.sig")" \
    '. + {($key): {url: $url, signature: $sig}}' <<<"$platforms")
}

for sig in "$sigs"/*.sig; do
  [[ -f $sig ]] || continue
  file=$(basename "$sig" .sig)
  case $file in
    *-setup.exe) add windows-x86_64-nsis "$file"; add windows-x86_64 "$file" ;;
    # Sin clave genérica linux-x86_64: una instalación AppImage de v0.1.0 recibiría un .deb.
    *.deb) add linux-x86_64-deb "$file" ;;
    # El .app de macOS es universal: sirve para Apple Silicon e Intel.
    *.app.tar.gz) for arch in aarch64 x86_64; do add "darwin-$arch-app" "$file"; add "darwin-$arch" "$file"; done ;;
  esac
done

[[ $(jq length <<<"$platforms") -gt 0 ]] || { echo "no hay firmas .sig en $sigs" >&2; exit 1; }

notes=$(bash "$(dirname "$0")/changelog-notes.sh" "$version")
jq -n --arg version "$version" --arg notes "$notes" --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --argjson platforms "$platforms" \
  '{version: $version, notes: $notes, pub_date: $date, platforms: $platforms}' > "$dir/latest.json"

echo "latest.json para $tag:"
jq -r '.platforms | keys[] | "  " + .' "$dir/latest.json"
gh release upload "$tag" "$dir/latest.json" --clobber
