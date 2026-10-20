# Nudge

App de escritorio **personal** de recordatorios recurrentes (nació para recordar gotas para los ojos;
sirve para agua, pausas, medicamentos según pauta médica, etc.). Local, sin conexión, sin backend,
sin cuentas. Multiplataforma: Windows, Linux y macOS. Responde y comenta en **español**.

Estado (2026-10-20): MVP armado; el núcleo tiene 18 pruebas en verde y la app ya se ejecutó con
`cargo tauri dev` en WSL/WSLg. Aún no hay commits en git.

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
                    Days (bitmask lun=bit0), TimeOfDay ("HH:MM"), Action, Alert, Occurrence
  schedule.rs       next_fire(spec, now, tz): próximo disparo; genérico sobre TimeZone
  store.rs          SQLite: migraciones por PRAGMA user_version; fechas en segundos Unix UTC;
                    spec guardado como JSON en reminders.spec
  service.rs        Service<Tz>: create/update/set_enabled/delete, tick, pending, act, pausa, historial
src-tauri/src/lib.rs  Comandos Tauri, bandeja, ventana de alerta, hilo planificador (cada 15 s)
ui/                 index.html+app.js (configuración), alert.html+alert.js, common.js, style.css
scripts/gen_icons.py  Íconos provisionales sin dependencias
```

### Invariantes del planificador (cubiertos por pruebas; mantenlos)

- Nada de temporizadores largos: `tick(now)` compara `next_fire_at <= now` con la hora real.
  Tras suspensión/apagado sale **una** alerta atrasada ("pendiente desde…"), no una por slot perdido.
- `tick` al disparar: `pending_since = pending_since.or(next_fire_at)` y
  `next_fire_at = now + nag_min` (o `None` si `nag_min == 0`).
- `act`: Hecho/Descartar → `next_fire(spec, now)` y limpia `pending_since`; Posponer →
  `now + minutos` y limpia `pending_since`. Siempre registra una fila en `occurrences`.
- `Anchor::Fixed`: slots desde `window.start` cada N min hasta `window.end` inclusive.
  `Anchor::FromDone`: `now + N`, encajado en la siguiente franja/día activo.
  `Schedule::Times`: respeta días, ignora la franja. Siempre estrictamente después de `now`.
- Horas locales inexistentes (cambio de horario) avanzan a la primera válida; ambiguas → la primera.
- Pausa global: setting `paused_until`; mientras rige, `tick` no devuelve nada.
- `window.start < window.end` (no hay franjas que crucen medianoche todavía).

### Contrato UI ↔ Rust

Comandos: `list_reminders`, `create_reminder{spec}`, `update_reminder{id,spec}`, `set_enabled{id,enabled}`,
`delete_reminder{id}`, `pending_alerts`, `act{id,action}`, `history{limit}`, `status`, `pause{minutes|null}`,
`set_autostart{enabled}`, `get_theme`, `set_theme{theme}`. Eventos: `reminders-changed`, `alerts-changed`,
`theme-changed` (payload: el tema).
JSON en snake_case: `schedule = {kind:"interval", every_min, anchor:"fixed"|"from_done"}` o
`{kind:"times", times:["08:00"]}`; `action = {type:"done"|"dismiss"} | {type:"snooze", minutes}`.
`theme = "system"|"light"|"dark"` (setting `theme`; `system` = sin fila).
Si cambias un tipo serializado en `model.rs`, actualiza `ui/app.js`/`ui/alert.js`.

## Comandos

```sh
cargo test -p nudge-core          # núcleo; no requiere libs gráficas
cargo clippy --workspace --all-targets
cargo fmt --all                   # rustfmt.toml: max_width 120
cargo tauri dev                   # app (Linux: requiere libwebkit2gtk-4.1-dev y compañía, ver README)
cargo tauri build
```

## Convenciones

- Comentarios y textos de UI en español; nombres de código en inglés.
- La lógica nueva va en `nudge-core` con pruebas; `src-tauri` solo orquesta.
- En JS, el texto del usuario siempre por `textContent` (helper `el()` en `common.js`), nunca `innerHTML`.
- CSP en `tauri.conf.json`: scripts solo desde archivos `.js` propios (no inline).
- WebView2 exige los **mismos** `additionalBrowserArgs` en todas las ventanas: si cambias los de
  `tauri.conf.json`, cambia también `WEBVIEW2_ARGS` en `src-tauri/src/lib.rs`.
- Tema: CSS oscuro por `prefers-color-scheme` salvo `data-theme="light"`, o por `data-theme="dark"`;
  Rust aplica además el tema nativo (`set_theme`) a cada ventana, incluida la de alerta al crearla.
- Linux: `show_all_title_buttons()` fuerza minimizar/maximizar en la barra de GTK (GNOME/WSLg traen
  `appmenu:close`, que solo muestra cerrar).

## Pendientes / ideas siguientes

- Exportar/importar configuración en JSON.
- Ícono definitivo (`cargo tauri icon icono-1024.png`, genera también `.icns`).
- Franjas que crucen medianoche; sonidos personalizados; estadísticas de cumplimiento.
- Build nativo en Windows (desde ruta `C:\…`, no `\\wsl.localhost\…`) y probar toasts/autoarranque.
- Revisar en Wayland que la ventana de alerta se vea por delante (el compositor puede bloquear el foco).
