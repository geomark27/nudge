# Nudge

App de escritorio **personal** de recordatorios recurrentes (nació para recordar gotas para los ojos;
sirve para agua, pausas, medicamentos según pauta médica, etc.). Local, sin conexión, sin backend,
sin cuentas. Multiplataforma: Windows, Linux y macOS. Responde y comenta en **español**.

Estado (2026-10-20): MVP armado; el núcleo tiene 32 pruebas en verde y la app ya se ejecutó con
`cargo tauri dev` en WSL/WSLg. Tema claro/oscuro listo. Añadidos sin probar aún a mano en la app:
respaldo JSON, sonidos propios, cumplimiento y franjas nocturnas.

## Decisiones tomadas (no re-discutir sin motivo)

- **Rust + Tauri v2** (sobre Go+Wails/Fyne): bandeja, autoarranque, instancia única e instaladores
  vienen como plugins oficiales en los 3 SO.
- **La alerta principal es una ventana propia siempre encima** (`ui/alert.html`) con
  Hecho / +5 / +10 / Descartar. La notificación nativa es solo respaldo: los botones de acción
  nativos se comportan distinto en cada SO (AUMID en Windows, firma en macOS, daemon en Linux).
- **UI sin build**: HTML/CSS/JS plano con `withGlobalTauri` (`window.__TAURI__`). Sin npm.
- **SQLite local** (`rusqlite` con `bundled`) en el directorio de datos de la app (`nudge.db`).
- Identificador `dev.ramosmg.nudge` (el nombre "Nudge" choca con `macadmins/nudge` de macOS).

## Arquitectura

```
crates/nudge-core/  Lógica pura, sin UI (testeable sin GTK/WebView)
  model.rs          ReminderSpec (lo editable), Reminder (spec + estado), Schedule, Anchor,
                    Days (bitmask lun=bit0), TimeOfDay ("HH:MM"), Action, Alert, Occurrence,
                    Theme, Backup + ImportMode (respaldo), ReminderStats
  schedule.rs       next_fire(spec, now, tz): próximo disparo; genérico sobre TimeZone
  store.rs          SQLite: migraciones por PRAGMA user_version; fechas en segundos Unix UTC;
                    spec guardado como JSON en reminders.spec
  service.rs        Service<Tz>: create/update/set_enabled/delete, tick, pending, act, pausa, historial,
                    tema, export/import (transaccional), stats
  sound.rs          Tonos integrados, `file:<nombre>` para sonidos propios y nombres de archivo seguros
src-tauri/src/lib.rs  Comandos Tauri, bandeja, ventana de alerta, hilo planificador (cada 15 s)
ui/                 index.html+app.js (configuración), alert.html+alert.js, common.js, style.css
scripts/gen_icons.py  Íconos provisionales sin dependencias
scripts/release.sh    `make release VERSION=x.y.z`: versión + CHANGELOG + commit + tag + release (gh)
scripts/changelog-notes.sh  Extrae una sección de CHANGELOG.md (notas del release; también lo usa el CI)
scripts/install.ps1, install.sh  Instaladores de una línea (último release o NUDGE_VERSION)
scripts/updater-key.sh  `make updater-key`: llave de firma de actualizaciones (una sola vez)
scripts/updater-manifest.sh  CI: genera latest.json desde las firmas .sig del release
.github/workflows/    ci.yml (`make check` en push/PR) y release.yml (instaladores de los 3 SO al subir un tag)
```

### Invariantes del planificador (cubiertos por pruebas; mantenlos)

- Nada de temporizadores largos: `tick(now)` compara `next_fire_at <= now` con la hora real.
  Tras suspensión/apagado sale **una** alerta atrasada ("pendiente desde…"), no una por slot perdido.
- `tick` al disparar: `pending_since = pending_since.or(next_fire_at)` y
  `next_fire_at = now + nag_min` (o `None` si `nag_min == 0`).
- `act`: Hecho/Descartar → `next_fire(spec, now)` y limpia `pending_since`; Posponer →
  `now + minutos` y limpia `pending_since`. Siempre registra una fila en `occurrences`.
- `Anchor::Fixed`: slots desde `window.start` cada N min hasta `window.end` inclusive (`end_offset()`).
  `Anchor::FromDone`: `now + N`, encajado en la siguiente franja/día activo.
  `Schedule::Times`: respeta días, ignora la franja. Siempre estrictamente después de `now`.
- Horas locales inexistentes (cambio de horario) avanzan a la primera válida; ambiguas → la primera.
- Pausa global: setting `paused_until`; mientras rige, `tick` no devuelve nada.
- `window.start != window.end`. Si `end < start` la franja cruza la medianoche y **pertenece al día en
  que abre** (los `days` se evalúan sobre ese día). Por eso `active_days` arranca en el día anterior.

### Contrato UI ↔ Rust

Comandos: `list_reminders`, `create_reminder{spec}`, `update_reminder{id,spec}`, `set_enabled{id,enabled}`,
`delete_reminder{id}`, `pending_alerts`, `act{id,action}`, `history{limit}`, `status`, `pause{minutes|null}`,
`set_autostart{enabled}`, `get_theme`, `set_theme{theme}`, `stats{days}`, `export_config`, `import_config{mode}`,
`list_sounds`, `add_sound`, `remove_sound{name}`, `read_sound{name}` (bytes crudos → `ArrayBuffer`),
`check_update` → `{version, notes} | null`, `install_update` (descarga, instala y reinicia).
Los de diálogo de archivo son `async` (las variantes `blocking_*` no pueden ir en el hilo principal).
Eventos: `reminders-changed`, `alerts-changed`, `theme-changed` (payload: el tema), `update-available`
(`{version, notes}`), `update-progress` (`[descargados, total|null]`). `status` incluye `version`.
JSON en snake_case: `schedule = {kind:"interval", every_min, anchor:"fixed"|"from_done"}` o
`{kind:"times", times:["08:00"]}`; `action = {type:"done"|"dismiss"} | {type:"snooze", minutes}`.
`theme = "system"|"light"|"dark"` (setting `theme`; `system` = sin fila).
`spec.tone = "chime"|"soft"|"double"|"drop"|"alarm"|"file:<nombre>"` (por defecto `chime`; `sound` sigue siendo
el on/off). `mode = "add"|"replace"`. Respaldo: `{app:"nudge", version:1, exported_at, theme, reminders:[spec]}`.
Si cambias un tipo serializado en `model.rs`, actualiza `ui/app.js`/`ui/alert.js`.

## Comandos (atajos en el `Makefile`: `make` lista todos; `make check` antes de cada commit)

```sh
cargo test -p nudge-core          # núcleo; no requiere libs gráficas
cargo clippy --workspace --all-targets
cargo fmt --all                   # rustfmt.toml: max_width 120
cargo tauri dev                   # app (Linux: requiere libwebkit2gtk-4.1-dev y compañía, ver README)
cargo tauri build
```

Releases: anota cada cambio en `[Unreleased]` de `CHANGELOG.md` **en el mismo commit**. Publicar es
`make release VERSION=x.y.z` (`DRY_RUN=1` simula); el tag dispara el CI que sube los instaladores.
La versión vive en `Cargo.toml` (`[workspace.package]`) y `src-tauri/tauri.conf.json`: deben coincidir.

Actualizador: `tauri-plugin-updater` lee `releases/latest/download/latest.json`. Las firmas `.sig` solo
se generan en el CI (`--config src-tauri/tauri.release.conf.json` + secreto `TAURI_SIGNING_PRIVATE_KEY`), así
`make build` local no necesita la llave. **No regeneres la llave**: las apps instaladas solo aceptan
la pública que traen; perderla obliga a reinstalar. El bundler falla si `pubkey` sigue en "PENDIENTE".

## Convenciones

- Comentarios y textos de UI en español; nombres de código en inglés.
- La lógica nueva va en `nudge-core` con pruebas; `src-tauri` solo orquesta.
- En JS, el texto del usuario siempre por `textContent` (helper `el()` en `common.js`), nunca `innerHTML`.
- CSP en `tauri.conf.json`: scripts solo desde archivos `.js` propios (no inline); `media-src blob:` es
  para reproducir los sonidos propios.
- Tonos integrados: si agregas uno, va en `BUILTIN_TONES` (`sound.rs`) **y** en `TONES` (`common.js`).
- Sonidos propios en `<app_data>/sounds/`; todo nombre que llega de la UI pasa por
  `sound::is_safe_file_name` (no puede salir de la carpeta).
- WebView2 exige los **mismos** `additionalBrowserArgs` en todas las ventanas: si cambias los de
  `tauri.conf.json`, cambia también `WEBVIEW2_ARGS` en `src-tauri/src/lib.rs`.
- Tema: CSS oscuro por `prefers-color-scheme` salvo `data-theme="light"`, o por `data-theme="dark"`;
  Rust aplica además el tema nativo (`set_theme`) a cada ventana, incluida la de alerta al crearla.
- Linux: `show_all_title_buttons()` fuerza minimizar/maximizar en la barra de GTK (GNOME/WSLg traen
  `appmenu:close`, que solo muestra cerrar).

## Pendientes / ideas siguientes

- Ícono definitivo (`cargo tauri icon icono-1024.png`, genera también `.icns`).
- Probar a mano: diálogos de exportar/importar y de sonidos, reproducción de MP3/OGG en WebKitGTK
  (depende de los plugins de GStreamer instalados) y WebView2.
- Ideas: volumen por recordatorio, gráfico diario de cumplimiento, incluir los sonidos en el respaldo.
- Build nativo en Windows (desde ruta `C:\…`, no `\\wsl.localhost\…`) y probar toasts/autoarranque.
- Revisar en Wayland que la ventana de alerta se vea por delante (el compositor puede bloquear el foco).
