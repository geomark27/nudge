#!/usr/bin/env bash
# ================================================================
#  Nudge - Instalador para Linux / Mac
#  Uso:      curl -fsSL https://raw.githubusercontent.com/geomark27/nudge/main/scripts/install.sh | bash
#  Version:  curl -fsSL ... | NUDGE_VERSION=v0.1.0 bash
#  Sirve tambien para actualizar: reemplaza la version instalada y conserva tus datos.
#
#  Linux: .deb con apt (Ubuntu, Debian, Mint, Pop!_OS...). Otras distribuciones no tienen instalador.
#  macOS: copia Nudge.app a /Applications (o ~/Applications si no hay permiso).
# ================================================================

set -euo pipefail

REPO="geomark27/nudge"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

die() { echo "  error: $*" >&2; exit 1; }

OS=$(uname -s)
ARCH=$(uname -m)

echo ""
echo "  Nudge - Instalador"
echo "  ------------------------------------"
echo ""

# 1. Buscar la version (la ultima, o la indicada en NUDGE_VERSION)
echo "[1/4] Buscando version..."
if [ -n "${NUDGE_VERSION:-}" ]; then
  API="https://api.github.com/repos/$REPO/releases/tags/$NUDGE_VERSION"
else
  API="https://api.github.com/repos/$REPO/releases/latest"
fi
# --retry: la red puede cortarse; un 404 (sin --retry-all-errors) significa que no hay release.
RELEASE=$(curl -fsSL --retry 3 --retry-delay 2 --connect-timeout 15 --max-time 60 "$API") \
  || die "no hay una version publicada (${NUDGE_VERSION:-latest}) o no hubo conexion; revisa https://github.com/$REPO/releases"
VERSION=$(printf '%s' "$RELEASE" | grep -m1 '"tag_name"' | cut -d'"' -f4)
echo "      version: $VERSION"

# URL del primer asset cuyo nombre cumple el patron (regex extendida).
asset_url() {
  printf '%s' "$RELEASE" | grep -o '"browser_download_url": *"[^"]*"' | cut -d'"' -f4 | grep -E "$1" | head -1
}

# Elegir el formato segun el sistema
case "$OS" in
  Linux)
    [ "$ARCH" = "x86_64" ] || die "por ahora solo hay version para Linux x86_64 (tienes $ARCH)"
    command -v apt-get >/dev/null && command -v dpkg >/dev/null \
      || die "solo hay instalador .deb (Ubuntu, Debian, Mint...); tu distribucion no usa apt"
    # Nudge-linux-amd64.deb (hasta v0.1.0: Nudge_<version>_amd64.deb)
    KIND=deb; PATTERN='amd64\.deb$'
    ;;
  Darwin)
    KIND=dmg; PATTERN='\.dmg$'
    ;;
  *)
    die "sistema operativo no soportado: $OS (en Windows usa install.ps1)"
    ;;
esac

URL=$(asset_url "$PATTERN")
[ -n "$URL" ] || die "el release $VERSION no tiene instalador $KIND"
FILE="$TMP/$(basename "$URL")"

# 2. Descargar
echo "[2/4] Descargando $(basename "$URL")..."
curl -fsSL --retry 3 --retry-delay 2 --connect-timeout 15 "$URL" -o "$FILE"
echo "      descargado"

# 3. Instalar (cerrando antes la version que este abierta)
echo "[3/4] Instalando..."
pkill -x nudge 2>/dev/null || true
case "$KIND" in
  deb)
    sudo apt-get install -y "$FILE" >/dev/null
    LAUNCH="nudge"
    WHERE="/usr/bin/nudge"
    ;;
  dmg)
    MNT="$TMP/mnt"
    mkdir -p "$MNT"
    hdiutil attach "$FILE" -nobrowse -quiet -mountpoint "$MNT"
    DEST="/Applications"
    [ -w "$DEST" ] || DEST="$HOME/Applications"
    mkdir -p "$DEST"
    rm -rf "$DEST/Nudge.app"
    cp -R "$MNT/Nudge.app" "$DEST/"
    hdiutil detach "$MNT" -quiet
    # La app no esta firmada: sin esto, Gatekeeper la bloquea al abrirla.
    xattr -dr com.apple.quarantine "$DEST/Nudge.app" 2>/dev/null || true
    LAUNCH="open $DEST/Nudge.app"
    WHERE="$DEST/Nudge.app"
    ;;
esac
echo "      instalado en $WHERE"

# 4. Abrir Nudge (queda en la bandeja / barra de menu)
echo "[4/4] Iniciando Nudge..."
if [ "$OS" = "Darwin" ] || [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]; then
  nohup $LAUNCH >/dev/null 2>&1 &
  echo "      listo"
else
  echo "      no hay sesion grafica; abrelo desde el menu de aplicaciones"
fi

echo ""
echo "  Nudge $VERSION instalado correctamente"
echo ""
echo "  Proximos pasos:"
echo "    1. Busca el icono de Nudge en la bandeja del sistema"
echo "    2. Crea tu primer recordatorio con '+ Nuevo'"
echo "    3. Las siguientes versiones se instalan solas desde la app"
echo ""
