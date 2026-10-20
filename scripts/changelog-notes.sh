#!/usr/bin/env bash
# Imprime el cuerpo de una sección de CHANGELOG.md (sin su encabezado) para usarlo como notas
# del release. Uso: scripts/changelog-notes.sh 0.1.0   o   scripts/changelog-notes.sh Unreleased
set -euo pipefail

section=${1:?uso: scripts/changelog-notes.sh <versión|Unreleased>}
changelog="$(dirname "$0")/../CHANGELOG.md"

# Desde "## [<sección>]" hasta el siguiente "## [" o las referencias de enlaces del final,
# sin líneas en blanco al principio ni al final.
notes=$(awk -v s="## [$section]" '
  index($0, "## [") == 1 { if (found) exit; found = (index($0, s) == 1); next }
  /^\[[^]]+\]: / { if (found) exit }
  found { lines[++n] = $0; if ($0 ~ /[^[:space:]]/) { if (!first) first = n; last = n } }
  END { for (i = first; first && i <= last; i++) print lines[i] }
' "$changelog")

[[ -n $notes ]] || { echo "CHANGELOG.md no tiene notas en la sección [$section]" >&2; exit 1; }
printf '%s\n' "$notes"
