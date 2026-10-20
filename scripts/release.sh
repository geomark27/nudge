#!/usr/bin/env bash
# Publica una versión de Nudge:
#   1. verifica que todo esté en orden (rama main al día, sin cambios, tag libre, `make check`);
#   2. fija la versión en Cargo.toml / tauri.conf.json y pasa [Unreleased] de CHANGELOG.md a la versión;
#   3. crea el commit "release: vX.Y.Z" y el tag, los sube y abre el release en borrador con `gh`.
# Al recibir el tag, GitHub Actions (.github/workflows/release.yml) compila los instaladores de
# Windows, Linux y macOS, los sube al release y lo publica.
#
# Uso: scripts/release.sh 0.1.0 [--dry-run]     (o: make release VERSION=0.1.0 [DRY_RUN=1])
set -euo pipefail
cd "$(dirname "$0")/.."

die() { echo "✗ $*" >&2; exit 1; }
step() { echo "→ $*"; }

version=${1:-}
dry_run=false
[[ ${2:-} == "--dry-run" ]] && dry_run=true
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "indica la versión como X.Y.Z, p. ej.: make release VERSION=0.1.0"
tag="v$version"
branch=main

# ---------- Verificaciones ----------

command -v gh >/dev/null || die "falta la CLI de GitHub (gh)"
gh auth status >/dev/null 2>&1 || die "gh no tiene sesión: ejecuta 'gh auth login'"

# Sin la llave, las versiones instaladas no podrían verificar ni recibir actualizaciones.
! grep -q '"pubkey": "PENDIENTE' src-tauri/tauri.conf.json \
  || die "falta la llave del actualizador: ejecuta 'make updater-key' y haz commit de tauri.conf.json"
gh secret list --json name -q '.[].name' | grep -qx TAURI_SIGNING_PRIVATE_KEY \
  || die "el repo no tiene el secreto TAURI_SIGNING_PRIVATE_KEY: ejecuta 'make updater-key'"

[[ $(git branch --show-current) == "$branch" ]] || die "los releases salen de '$branch'"
[[ -z $(git status --porcelain) ]] || die "hay cambios sin commit; haz commit o guárdalos antes del release"

step "Comprobando el remoto"
git fetch --quiet --tags origin "$branch"
git merge-base --is-ancestor "origin/$branch" HEAD || die "tu '$branch' está atrasada respecto a origin: haz pull"
git rev-parse -q --verify "refs/tags/$tag" >/dev/null && die "el tag $tag ya existe"

current=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
if [[ $version != "$current" ]]; then
  newest=$(printf '%s\n%s\n' "$current" "$version" | sort -V | tail -1)
  [[ $newest == "$version" ]] || die "$version es anterior a la versión actual ($current)"
fi

notes=$(bash scripts/changelog-notes.sh Unreleased) || die "anota los cambios en [Unreleased] de CHANGELOG.md"
previous=$(git describe --tags --abbrev=0 2>/dev/null || true)
repo_url=$(gh repo view --json url -q .url)

echo
echo "Release $tag (versión actual: $current${previous:+, último tag: $previous})"
echo "Notas:"
echo "$notes" | sed 's/^/  │ /'
echo

if $dry_run; then
  echo "Simulación (DRY_RUN): no se modificó nada."
  exit 0
fi

step "Ejecutando make check"
make --no-print-directory check || die "make check falló"

read -r -p "¿Publicar $tag en $repo_url? [s/N] " answer
[[ $answer =~ ^[sS]$ ]] || die "cancelado"

# ---------- Cambios ----------

step "Fijando la versión $version"
sed -i "0,/^version = \".*\"$/s//version = \"$version\"/" Cargo.toml
sed -i "s/^  \"version\": \".*\",$/  \"version\": \"$version\",/" src-tauri/tauri.conf.json
cargo update --workspace --offline --quiet

step "Actualizando CHANGELOG.md"
today=$(date +%F)
if [[ -n $previous ]]; then
  version_link="[$version]: $repo_url/compare/$previous...$tag"
else
  version_link="[$version]: $repo_url/releases/tag/$tag"
fi
awk -v header="## [$version] - $today" \
    -v unreleased_link="[Unreleased]: $repo_url/compare/$tag...HEAD" \
    -v version_link="$version_link" '
  $0 == "## [Unreleased]" { print; print ""; print header; next }
  index($0, "[Unreleased]: ") == 1 { print unreleased_link; print version_link; next }
  { print }
' CHANGELOG.md > CHANGELOG.md.tmp && mv CHANGELOG.md.tmp CHANGELOG.md

step "Creando el commit y el tag $tag"
git add Cargo.toml Cargo.lock src-tauri/tauri.conf.json CHANGELOG.md
git commit --quiet -m "release: $tag"
git tag -a "$tag" -m "Nudge $tag"

step "Subiendo a origin"
git push --quiet origin "$branch"
git push --quiet origin "$tag"

step "Creando el release en borrador"
bash scripts/changelog-notes.sh "$version" | gh release create "$tag" --draft --verify-tag \
  --title "Nudge $tag" --notes-file -

echo
echo "✓ $tag publicado. GitHub Actions está compilando los instaladores (~15 min):"
echo "    gh run watch \$(gh run list --workflow release.yml --limit 1 --json databaseId -q '.[0].databaseId')"
echo "  Al terminar, el release se publica solo: $repo_url/releases/tag/$tag"
