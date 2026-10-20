# Atajos de desarrollo de Nudge. `make` sin argumentos lista los comandos.
# En Windows ejecútalo dentro de WSL (cargo y la CLI de Tauri viven allí).

SHELL := bash
.DEFAULT_GOAL := help

CARGO ?= cargo
PYTHON ?= python3

.PHONY: help dev build test test-all lint fmt fmt-check check icons icon clean version notes release updater-key

help: ## Muestra esta ayuda
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F ':.*## ' '{printf "  \033[36mmake %-10s\033[0m %s\n", $$1, $$2}'

dev: ## Ejecuta la app con recarga (cargo tauri dev)
	$(CARGO) tauri dev

build: ## Genera los instaladores (cargo tauri build)
	$(CARGO) tauri build

test: ## Pruebas del núcleo (no requiere GTK ni WebView)
	$(CARGO) test -p nudge-core

test-all: ## Pruebas de todo el workspace
	$(CARGO) test --workspace

lint: ## Clippy en todo el workspace; los avisos cuentan como error
	$(CARGO) clippy --workspace --all-targets -- -D warnings

fmt: ## Formatea el código (rustfmt.toml: max_width 120)
	$(CARGO) fmt --all

fmt-check: ## Verifica el formato sin modificar archivos
	$(CARGO) fmt --all -- --check

check: fmt-check lint test ## Formato + clippy + pruebas: úsalo antes de cada commit

icons: ## Regenera los íconos provisionales
	$(PYTHON) scripts/gen_icons.py

icon: ## Ícono definitivo desde un PNG 1024x1024: make icon PNG=ruta.png
	@test -n "$(PNG)" || { echo "Falta PNG: make icon PNG=ruta/al/icono-1024.png"; exit 1; }
	$(CARGO) tauri icon "$(PNG)"

clean: ## Borra los artefactos de compilación (target/)
	$(CARGO) clean

version: ## Muestra la versión actual y el último tag
	@echo "versión: $$(sed -n 's/^version = "\(.*\)"$$/\1/p' Cargo.toml | head -1)"
	@echo "último tag: $$(git describe --tags --abbrev=0 2>/dev/null || echo 'ninguno')"

notes: ## Muestra lo pendiente de publicar ([Unreleased] de CHANGELOG.md)
	@bash scripts/changelog-notes.sh Unreleased

release: ## Publica una versión: make release VERSION=0.1.0 (DRY_RUN=1 solo simula)
	@bash scripts/release.sh "$(VERSION)" $(if $(DRY_RUN),--dry-run)

updater-key: ## Una sola vez: crea la llave de firma de actualizaciones y la sube como secreto a GitHub
	@bash scripts/updater-key.sh
