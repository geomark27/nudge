# Nudge

Recordatorios recurrentes de escritorio: locales, sin conexión y multiplataforma (Windows, Linux, macOS).
Vive en la bandeja del sistema y, cuando toca, abre una ventana **siempre encima** con
**Hecho / +5 min / +10 min / Descartar**, además de una notificación nativa.

## Estructura

```
crates/nudge-core/   Núcleo sin UI: modelo, planificador, SQLite y reglas (con pruebas)
src-tauri/           App Tauri v2: bandeja, ventanas, comandos, autoarranque, instancia única
ui/                  Interfaz en HTML/CSS/JS sin build (index = configuración, alert = alerta)
scripts/             Utilidades (íconos provisionales)
```

## Cómo funciona el planificador

- Cada recordatorio guarda `next_fire_at` (UTC). Un hilo revisa cada 15 s contra la hora real;
  no hay temporizadores largos que se desfasen con la suspensión.
- Si el equipo estuvo suspendido o apagado, cada recordatorio produce **una sola** alerta
  "pendiente desde…", no una por cada horario perdido.
- Tipos de horario:
  - **Fijo**: cada N minutos alineado al inicio de la franja (08:00, 10:00, 12:00…).
  - **Desde que lo marco hecho**: el intervalo se reinicia al pulsar "Hecho".
  - **Horas exactas**: lista de horas; respetan los días pero no la franja.
- Si no respondes, insiste cada N minutos (configurable, 0 = no insistir).
- "Pausar" silencia todo hasta la hora indicada; al terminar, lo vencido suena una vez.
- Datos en `nudge.db` dentro del directorio de datos de la app
  (Windows `%APPDATA%\dev.ramosmg.nudge`, Linux `~/.local/share/dev.ramosmg.nudge`,
  macOS `~/Library/Application Support/dev.ramosmg.nudge`).

## Requisitos

Todas las plataformas: Rust estable y la CLI de Tauri:

```sh
cargo install tauri-cli --version "^2" --locked
```

**Linux (Ubuntu/Debian, incluido WSL con WSLg):**

```sh
sudo apt update
sudo apt install -y build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev libdbus-1-dev
```

En GNOME el ícono de bandeja requiere la extensión *AppIndicator*.

**Windows:** Rust con toolchain MSVC + "Desarrollo para el escritorio con C++" de Visual Studio
Build Tools. WebView2 ya viene en Windows 10/11. Compila desde una ruta de Windows
(p. ej. `C:\dev\nudge`), no desde `\\wsl.localhost\…`.

**macOS:** `xcode-select --install`.

## Desarrollo

```sh
cargo test -p nudge-core      # pruebas del núcleo (no requiere GTK ni WebView)
cargo tauri dev               # ejecuta la app con recarga
cargo tauri build             # instaladores: MSI/NSIS, DEB/RPM/AppImage, DMG
```

## Pendientes conocidos

- Íconos provisionales (`python3 scripts/gen_icons.py`). Para el definitivo:
  `cargo tauri icon ruta/al/icono-1024.png` (genera también el `.icns` de macOS).
- En Wayland el sistema puede impedir que la ventana de alerta tome el foco; aparece, pero quizá
  no por delante de todo.
- En Windows, durante `cargo tauri dev` las notificaciones nativas aparecen a nombre de
  PowerShell; con la app instalada usan el nombre de Nudge.
- macOS requiere firmar la app para que las notificaciones nativas sean fiables.
- Siguientes pasos sugeridos: exportar/importar JSON, sonidos personalizados, estadísticas.
