#!/usr/bin/env bash
# Crea (una sola vez) la llave con la que se firman las actualizaciones automáticas y la configura:
#   - privada: ~/.tauri/nudge.key  → secreto TAURI_SIGNING_PRIVATE_KEY del repo en GitHub (para el CI);
#   - pública: plugins.updater.pubkey de src-tauri/tauri.conf.json (va en el commit).
# La llave privada nunca se imprime. Respáldala: si se pierde, las versiones ya instaladas no
# podrán actualizarse solas y habría que reinstalar con el instalador de una línea.
#
# Uso: make updater-key
set -euo pipefail
cd "$(dirname "$0")/.."

die() { echo "✗ $*" >&2; exit 1; }
step() { echo "→ $*"; }

key="${NUDGE_UPDATER_KEY:-$HOME/.tauri/nudge.key}"
conf=src-tauri/tauri.conf.json

command -v gh >/dev/null || die "falta la CLI de GitHub (gh)"
gh auth status >/dev/null 2>&1 || die "gh no tiene sesión: ejecuta 'gh auth login'"

if [[ -e $key ]]; then
  step "Reutilizo la llave existente $key (crear otra rompería las actualizaciones)"
else
  step "Generando la llave en $key"
  mkdir -p "$(dirname "$key")"
  # --ci: sin contraseña, para que el CI firme solo con el secreto.
  cargo tauri signer generate --ci -w "$key" >/dev/null
  chmod 600 "$key"
fi
[[ -s "$key.pub" ]] || die "no encontré la llave pública $key.pub"
pub=$(tr -d '\n' < "$key.pub")

step "Guardando la llave privada como secreto TAURI_SIGNING_PRIVATE_KEY de $(gh repo view --json nameWithOwner -q .nameWithOwner)"
gh secret set TAURI_SIGNING_PRIVATE_KEY < "$key"

step "Escribiendo la llave pública en $conf"
sed -i "s|^      \"pubkey\": \".*\",$|      \"pubkey\": \"$pub\",|" "$conf"
grep -q "\"pubkey\": \"$pub\"" "$conf" || die "no pude escribir la llave pública en $conf"

echo
echo "✓ Listo. Haz commit de $conf y guarda una copia de $key en un lugar seguro"
echo "  (gestor de contraseñas o similar). No la subas al repositorio."
