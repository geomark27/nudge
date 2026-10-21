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
- La franja puede cruzar la medianoche (22:00–02:00): cuenta para el día en que empieza.
- Si no respondes, insiste cada N minutos (configurable, 0 = no insistir).
- "Pausar" silencia todo hasta la hora indicada; al terminar, lo vencido suena una vez.
- **Sonidos**: cinco tonos integrados (Campanita, Suave, Doble toque, Gota, Alarma) o tus propios
  MP3/WAV/OGG/M4A (hasta 10 MB), que se copian a la carpeta `sounds/` de la app.
- **Cumplimiento**: por recordatorio, hechas / pospuestas / descartadas en los últimos 7 o 30 días;
  el porcentaje es hechas ÷ (hechas + descartadas).
- **Respaldo**: exporta recordatorios y tema a JSON; al importar puedes añadir o reemplazar.
  No incluye historial ni archivos de sonido.
- **Tema** claro, oscuro o el del sistema.
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

El `Makefile` agrupa los comandos (`make` muestra la lista; `make check` = formato + clippy + pruebas):

```sh
cargo test -p nudge-core      # pruebas del núcleo (no requiere GTK ni WebView)
cargo tauri dev               # ejecuta la app con recarga
cargo tauri build             # instalador del sistema actual: NSIS (.exe), .deb o .dmg
```

## Instalación

Hay un instalador por sistema. Estos enlaces descargan siempre la última versión:

| Sistema | Instalador | Por terminal |
|---|---|---|
| Windows 10/11 (x64) | [`Nudge-windows-x64-setup.exe`](https://github.com/geomark27/nudge/releases/latest/download/Nudge-windows-x64-setup.exe) | Sí (PowerShell) |
| Linux x64 con apt: Ubuntu, Debian, Mint, Pop!_OS… | [`Nudge-linux-amd64.deb`](https://github.com/geomark27/nudge/releases/latest/download/Nudge-linux-amd64.deb) | Sí (pide `sudo`) |
| macOS (Intel y Apple Silicon) | [`Nudge-macos-universal.dmg`](https://github.com/geomark27/nudge/releases/latest/download/Nudge-macos-universal.dmg) | Sí |

Otras distribuciones de Linux (Fedora, Arch, openSUSE…) no tienen instalador por ahora.

### Por terminal

**Windows** (PowerShell):

```powershell
iwr -useb https://raw.githubusercontent.com/geomark27/nudge/main/scripts/install.ps1 | iex
```

**Linux y macOS**:

```sh
curl -fsSL https://raw.githubusercontent.com/geomark27/nudge/main/scripts/install.sh | bash
```

El instalador descarga la última versión de
[Releases](https://github.com/geomark27/nudge/releases/latest), la instala y abre Nudge. Para una
versión concreta: `$env:NUDGE_VERSION = "v0.1.0"` antes del comando en Windows, o
`… | NUDGE_VERSION=v0.1.0 bash` en Linux/macOS.

- **Windows**: instalación por usuario en `%LOCALAPPDATA%\Nudge`, sin permisos de administrador.
- **Linux**: instala el `.deb` con apt (pide `sudo`). En otras distribuciones avisa que no hay
  instalador y no hace nada.
- **macOS**: copia `Nudge.app` a `/Applications` (Intel y Apple Silicon).

### Con el instalador descargado

- **Windows**: doble clic en el `.exe`. Instala por usuario, sin permisos de administrador.
- **Linux**: doble clic en el `.deb` (se abre en el Centro de aplicaciones) o
  `sudo apt install ./Nudge-linux-amd64.deb`.
- **macOS**: abre el `.dmg` y arrastra Nudge a Aplicaciones.

Como no está firmado con un certificado de pago, Windows (SmartScreen: "Más información → Ejecutar de
todas formas") y macOS (clic derecho → "Abrir") avisan la primera vez. Con la instalación por terminal
no aparece ese aviso.

**Actualizaciones**: Nudge busca versiones nuevas al abrir y cada 6 horas. Cuando hay una, avisa y
se actualiza con **Actualizar ahora**: descarga, verifica la firma e instala sola. También puedes
volver a ejecutar el instalador. Tus recordatorios, historial y sonidos se conservan.

## Versiones y releases

**Una sola vez**, antes del primer release: `make updater-key`. Crea la llave que firma las actualizaciones
(`~/.tauri/nudge.key`; respáldala), la guarda como secreto del repo y escribe la llave pública en
`tauri.conf.json`, que va en el commit.

Las versiones siguen [SemVer](https://semver.org/lang/es/) y los cambios se anotan en
[`CHANGELOG.md`](CHANGELOG.md), en la sección **[Unreleased]**, a medida que se hacen.

```sh
make notes                                  # qué saldrá en el próximo release
make release VERSION=0.2.0 DRY_RUN=1        # simula: verifica todo sin tocar nada
make release VERSION=0.2.0                  # publica
```

`make release` verifica la rama `main`, que no haya cambios sin commit, que el tag esté libre y que pase
`make check`. Luego fija la versión en `Cargo.toml` y `tauri.conf.json`, convierte [Unreleased] en la
sección de la versión, crea el commit `release: vX.Y.Z` y el tag, sube ambos y abre el release en
borrador con `gh`. El workflow [`release.yml`](.github/workflows/release.yml) compila los instaladores
en Windows, Linux y macOS, los adjunta al release y lo publica cuando los tres terminan bien.

## Pendientes conocidos

- Íconos provisionales (`python3 scripts/gen_icons.py`). Para el definitivo:
  `cargo tauri icon ruta/al/icono-1024.png` (genera también el `.icns` de macOS).
- En Wayland el sistema puede impedir que la ventana de alerta tome el foco; aparece, pero quizá
  no por delante de todo.
- En Windows, durante `cargo tauri dev` las notificaciones nativas aparecen a nombre de
  PowerShell; con la app instalada usan el nombre de Nudge.
- macOS requiere firmar la app para que las notificaciones nativas sean fiables.
- En Linux, reproducir MP3/OGG propios depende de los plugins de GStreamer instalados
  (`gstreamer1.0-plugins-good`/`-bad`/`-libav`); los tonos integrados no los necesitan.
- Siguientes pasos sugeridos: volumen por recordatorio, gráfico diario de cumplimiento.
