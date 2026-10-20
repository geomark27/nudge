use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use nudge_core::{
    Action, Alert, Backup, ImportMode, Occurrence, Reminder, ReminderSpec, ReminderStats, Service, Store, Theme, sound,
};
use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent, Wry};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::UpdaterExt;

/// Cada cuánto se revisan los recordatorios vencidos.
const TICK: Duration = Duration::from_secs(15);
/// Un respaldo normal pesa unos pocos KB; esto solo evita leer un archivo equivocado enorme.
const MAX_BACKUP_BYTES: u64 = 5 * 1024 * 1024;
/// Primera búsqueda de actualizaciones tras arrancar; después se repite cada `UPDATE_EVERY`.
const UPDATE_FIRST_CHECK: Duration = Duration::from_secs(60);
const UPDATE_EVERY: Duration = Duration::from_secs(6 * 3600);
/// Argumento con el que se lanza al iniciar sesión: arranca oculto en la bandeja.
const MINIMIZED_ARG: &str = "--minimized";
/// WebView2 exige los mismos argumentos en todas las ventanas. Son los de Tauri por
/// defecto más permitir audio sin interacción previa (el sonido de la alerta).
#[cfg(windows)]
const WEBVIEW2_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required";

struct AppState(Mutex<Service<Local>>);

type CmdResult<T> = Result<T, String>;

fn with_service<T>(
    state: &State<'_, AppState>,
    f: impl FnOnce(&Service<Local>) -> Result<T, nudge_core::Error>,
) -> CmdResult<T> {
    let svc = state.0.lock().map_err(|_| "estado interno bloqueado".to_string())?;
    f(&svc).map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct Status {
    paused_until: Option<DateTime<Utc>>,
    autostart: bool,
    version: String,
}

/// Versión nueva publicada en GitHub Releases.
#[derive(Clone, Serialize)]
struct UpdateInfo {
    version: String,
    /// Notas del release (la sección del CHANGELOG), en Markdown.
    notes: Option<String>,
}

#[tauri::command]
fn list_reminders(state: State<'_, AppState>) -> CmdResult<Vec<Reminder>> {
    with_service(&state, |s| s.list())
}

#[tauri::command]
fn create_reminder(app: AppHandle, state: State<'_, AppState>, spec: ReminderSpec) -> CmdResult<Reminder> {
    let r = with_service(&state, |s| s.create(spec, Utc::now()))?;
    notify_changed(&app);
    Ok(r)
}

#[tauri::command]
fn update_reminder(app: AppHandle, state: State<'_, AppState>, id: i64, spec: ReminderSpec) -> CmdResult<Reminder> {
    let r = with_service(&state, |s| s.update(id, spec, Utc::now()))?;
    notify_changed(&app);
    Ok(r)
}

#[tauri::command]
fn set_enabled(app: AppHandle, state: State<'_, AppState>, id: i64, enabled: bool) -> CmdResult<Reminder> {
    let r = with_service(&state, |s| s.set_enabled(id, enabled, Utc::now()))?;
    notify_changed(&app);
    Ok(r)
}

#[tauri::command]
fn delete_reminder(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    with_service(&state, |s| s.delete(id))?;
    close_alert_if_idle(&app, &state);
    notify_changed(&app);
    Ok(())
}

#[tauri::command]
fn pending_alerts(state: State<'_, AppState>) -> CmdResult<Vec<Alert>> {
    with_service(&state, |s| s.pending(Utc::now()))
}

#[tauri::command]
fn act(app: AppHandle, state: State<'_, AppState>, id: i64, action: Action) -> CmdResult<()> {
    with_service(&state, |s| s.act(id, action, Utc::now()))?;
    close_alert_if_idle(&app, &state);
    notify_changed(&app);
    Ok(())
}

#[tauri::command]
fn history(state: State<'_, AppState>, limit: Option<u32>) -> CmdResult<Vec<Occurrence>> {
    with_service(&state, |s| s.history(limit.unwrap_or(50)))
}

#[tauri::command]
fn status(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Status> {
    Ok(Status {
        paused_until: with_service(&state, |s| s.paused_until(Utc::now()))?,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        version: app.package_info().version.to_string(),
    })
}

#[tauri::command]
async fn check_update(app: AppHandle) -> CmdResult<Option<UpdateInfo>> {
    find_update(&app).await
}

/// Descarga, verifica la firma e instala la versión nueva; luego reinicia Nudge.
/// Emite `update-progress` con `[bytes descargados, total | null]`.
#[tauri::command]
async fn install_update(app: AppHandle) -> CmdResult<()> {
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(update_error)?;
    let Some(update) = update else {
        return Err("ya tienes la última versión".into());
    };
    let mut downloaded = 0u64;
    update
        .download_and_install(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = app.emit("update-progress", (downloaded, total));
            },
            || {},
        )
        .await
        .map_err(update_error)?;
    // En Windows el instalador ya cerró la app; en Linux y macOS hay que relanzarla.
    app.restart()
}

async fn find_update(app: &AppHandle) -> CmdResult<Option<UpdateInfo>> {
    let update = app.updater().map_err(|e| e.to_string())?.check().await.map_err(update_error)?;
    Ok(update.map(|u| UpdateInfo { version: u.version, notes: u.body }))
}

fn update_error(e: tauri_plugin_updater::Error) -> String {
    format!("no se pudo actualizar: {e}")
}

/// Busca versiones nuevas en segundo plano: avisa a la UI (`update-available`) y, una sola vez
/// por versión, con una notificación del sistema por si Nudge está solo en la bandeja.
fn spawn_update_checker(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(UPDATE_FIRST_CHECK);
        let mut notified: Option<String> = None;
        loop {
            match tauri::async_runtime::block_on(find_update(&app)) {
                Ok(Some(update)) => {
                    let _ = app.emit("update-available", update.clone());
                    if notified.as_deref() != Some(update.version.as_str()) {
                        let _ = app
                            .notification()
                            .builder()
                            .title(format!("Nudge {} disponible", update.version))
                            .body("Abre Nudge para actualizar con un clic.")
                            .show();
                        notified = Some(update.version);
                    }
                }
                Ok(None) => {}
                // Sin conexión o sin releases todavía: se reintenta en la siguiente vuelta.
                Err(e) => eprintln!("{e}"),
            }
            std::thread::sleep(UPDATE_EVERY);
        }
    });
}

/// Pausa todas las alertas `minutes` minutos; `None` reanuda.
#[tauri::command]
fn pause(app: AppHandle, state: State<'_, AppState>, minutes: Option<u32>) -> CmdResult<()> {
    pause_for(&app, &state, minutes)
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> CmdResult<()> {
    let launcher = app.autolaunch();
    let result = if enabled { launcher.enable() } else { launcher.disable() };
    result.map_err(|e| e.to_string())
}

#[tauri::command]
fn get_theme(state: State<'_, AppState>) -> CmdResult<Theme> {
    with_service(&state, |s| s.theme())
}

/// Guarda el tema y lo aplica a todas las ventanas abiertas.
#[tauri::command]
fn set_theme(app: AppHandle, state: State<'_, AppState>, theme: Theme) -> CmdResult<()> {
    with_service(&state, |s| s.set_theme(theme))?;
    apply_theme(&app, theme);
    Ok(())
}

fn apply_theme(app: &AppHandle, theme: Theme) {
    for w in app.webview_windows().values() {
        let _ = w.set_theme(native_theme(theme));
    }
    let _ = app.emit("theme-changed", theme);
}

#[tauri::command]
fn stats(state: State<'_, AppState>, days: u32) -> CmdResult<Vec<ReminderStats>> {
    let since = Utc::now() - chrono::Duration::days(i64::from(days.clamp(1, 365)));
    with_service(&state, |s| s.stats(since))
}

// Los comandos con diálogos de archivo son `async`: así corren fuera del hilo principal,
// donde las variantes `blocking_*` del plugin congelarían la interfaz.

/// Guarda la configuración en el JSON que elija el usuario; `None` si cancela.
#[tauri::command]
async fn export_config(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Option<String>> {
    let backup = with_service(&state, |s| s.export(Utc::now()))?;
    let json = serde_json::to_string_pretty(&backup).map_err(|e| e.to_string())?;
    let Some(path) = file_dialog(&app, "Respaldo de Nudge", &["json"])
        .set_file_name(format!("nudge-{}.json", Local::now().format("%Y-%m-%d")))
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("no se pudo guardar el respaldo: {e}"))?;
    Ok(Some(path.display().to_string()))
}

/// Importa un respaldo elegido por el usuario; devuelve cuántos recordatorios entraron.
#[tauri::command]
async fn import_config(app: AppHandle, state: State<'_, AppState>, mode: ImportMode) -> CmdResult<Option<usize>> {
    let Some(path) = file_dialog(&app, "Respaldo de Nudge", &["json"]).blocking_pick_file() else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    let text = read_limited(&path, MAX_BACKUP_BYTES)?;
    let backup: Backup =
        serde_json::from_str(&text).map_err(|e| format!("el archivo no es un respaldo válido de Nudge: {e}"))?;
    let theme = backup.theme;
    let count = with_service(&state, |s| s.import(backup, mode, Utc::now()))?;
    apply_theme(&app, theme);
    close_alert_if_idle(&app, &state);
    notify_changed(&app);
    Ok(Some(count))
}

#[tauri::command]
fn list_sounds(app: AppHandle) -> CmdResult<Vec<String>> {
    let Ok(entries) = std::fs::read_dir(sounds_dir(&app)?) else {
        return Ok(Vec::new());
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| sound::is_safe_file_name(name))
        .collect();
    names.sort();
    Ok(names)
}

/// Copia a la carpeta de sonidos un audio elegido por el usuario; devuelve su nombre.
#[tauri::command]
async fn add_sound(app: AppHandle) -> CmdResult<Option<String>> {
    let extensions: Vec<&str> = sound::extensions().collect();
    let Some(path) = file_dialog(&app, "Audio", &extensions).blocking_pick_file() else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| e.to_string())?;
    let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    if size > sound::MAX_SOUND_BYTES {
        return Err(format!("el sonido pesa más de {} MB", sound::MAX_SOUND_BYTES / 1024 / 1024));
    }
    let original = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = sounds_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = sound::sound_file_name(&original, |n| dir.join(n).exists()).map_err(|e| e.to_string())?;
    std::fs::copy(&path, dir.join(&name)).map_err(|e| format!("no se pudo copiar el sonido: {e}"))?;
    Ok(Some(name))
}

/// Los recordatorios que lo usaban suenan con la campanita (la UI hace el respaldo).
#[tauri::command]
fn remove_sound(app: AppHandle, name: String) -> CmdResult<()> {
    std::fs::remove_file(sound_path(&app, &name)?).map_err(|e| e.to_string())
}

/// Bytes del sonido; la UI los reproduce desde un `Blob`.
#[tauri::command]
fn read_sound(app: AppHandle, name: String) -> CmdResult<tauri::ipc::Response> {
    let bytes = std::fs::read(sound_path(&app, &name)?).map_err(|e| e.to_string())?;
    Ok(tauri::ipc::Response::new(bytes))
}

fn sounds_dir(app: &AppHandle) -> CmdResult<PathBuf> {
    app.path().app_data_dir().map(|d| d.join("sounds")).map_err(|e| e.to_string())
}

/// Ruta de un sonido propio; rechaza nombres que saldrían de la carpeta.
fn sound_path(app: &AppHandle, name: &str) -> CmdResult<PathBuf> {
    if !sound::is_safe_file_name(name) {
        return Err(format!("nombre de sonido inválido: {name:?}"));
    }
    Ok(sounds_dir(app)?.join(name))
}

fn file_dialog(app: &AppHandle, filter: &str, extensions: &[&str]) -> FileDialogBuilder<Wry> {
    let dialog = app.dialog().file().add_filter(filter, extensions);
    match app.get_webview_window("main") {
        Some(w) => dialog.set_parent(&w),
        None => dialog,
    }
}

fn read_limited(path: &Path, max_bytes: u64) -> CmdResult<String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > max_bytes {
        return Err("el archivo es demasiado grande para ser un respaldo de Nudge".into());
    }
    std::fs::read_to_string(path).map_err(|e| format!("no se pudo leer el archivo: {e}"))
}

/// Cierra la ventana de alerta si ya no queda nada pendiente (p. ej. tras borrar).
fn close_alert_if_idle(app: &AppHandle, state: &State<'_, AppState>) {
    let idle = with_service(state, |s| s.pending(Utc::now())).is_ok_and(|p| p.is_empty());
    if idle && let Some(w) = app.get_webview_window("alert") {
        let _ = w.close();
    }
}

/// Tema de la barra de título y de `prefers-color-scheme`; `None` sigue al sistema.
fn native_theme(theme: Theme) -> Option<tauri::Theme> {
    match theme {
        Theme::System => None,
        Theme::Light => Some(tauri::Theme::Light),
        Theme::Dark => Some(tauri::Theme::Dark),
    }
}

fn saved_theme(app: &AppHandle) -> Option<tauri::Theme> {
    let state = app.state::<AppState>();
    with_service(&state, |s| s.theme()).ok().and_then(native_theme)
}

/// GNOME (y WSLg, que hereda su esquema) usa `appmenu:close`: la barra de título de GTK
/// solo muestra "cerrar". Si faltan minimizar/maximizar, se muestran los tres.
#[cfg(target_os = "linux")]
fn show_all_title_buttons() {
    use gtk::prelude::*;
    let Some(settings) = gtk::Settings::default() else { return };
    let layout = settings.gtk_decoration_layout().unwrap_or_default();
    if !(layout.contains("minimize") && layout.contains("maximize")) {
        settings.set_gtk_decoration_layout(Some(":minimize,maximize,close"));
    }
}

fn pause_for(app: &AppHandle, state: &State<'_, AppState>, minutes: Option<u32>) -> CmdResult<()> {
    let until = minutes.map(|m| Utc::now() + chrono::Duration::minutes(i64::from(m)));
    with_service(state, |s| s.pause_until(until))?;
    notify_changed(app);
    Ok(())
}

fn notify_changed(app: &AppHandle) {
    let _ = app.emit("reminders-changed", ());
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Muestra (o trae al frente) la ventana de alertas, siempre encima del resto.
fn show_alert_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("alert") {
        let _ = app.emit("alerts-changed", ());
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_always_on_top(true);
        let _ = w.set_focus();
        return;
    }
    let builder = WebviewWindowBuilder::new(app, "alert", WebviewUrl::App("alert.html".into()))
        .title("Nudge — Recordatorio")
        .inner_size(420.0, 320.0)
        .min_inner_size(360.0, 220.0)
        .always_on_top(true)
        .theme(saved_theme(app))
        .center()
        .focused(true);
    #[cfg(windows)]
    let builder = builder.additional_browser_args(WEBVIEW2_ARGS);
    match builder.build() {
        Ok(w) => {
            let _ = w.request_user_attention(Some(tauri::UserAttentionType::Critical));
        }
        Err(e) => eprintln!("no se pudo abrir la ventana de alerta: {e}"),
    }
}

/// Aviso nativo del sistema como respaldo de la ventana propia.
fn native_notification(app: &AppHandle, alerts: &[Alert]) {
    let (title, body) = match alerts {
        [one] => (one.title.clone(), one.message.clone()),
        many => ("Nudge".to_string(), format!("Tienes {} recordatorios pendientes", many.len())),
    };
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Revisa los recordatorios cada `TICK` en un hilo propio.
fn spawn_scheduler(app: AppHandle) {
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        // Al arrancar, mostrar lo que quedó sin responder antes de cerrar la app.
        let mut first = true;
        loop {
            let now = Utc::now();
            let result = state.0.lock().map_err(|_| "lock".to_string()).and_then(|svc| {
                let fired = svc.tick(now).map_err(|e| e.to_string())?;
                let leftover = if first { svc.pending(now).map_err(|e| e.to_string())? } else { Vec::new() };
                Ok((fired, leftover))
            });
            match result {
                Ok((fired, leftover)) => {
                    if !fired.is_empty() {
                        native_notification(&app, &fired);
                    }
                    if !fired.is_empty() || !leftover.is_empty() {
                        show_alert_window(&app);
                        notify_changed(&app);
                    }
                }
                Err(e) => eprintln!("error del planificador: {e}"),
            }
            first = false;
            std::thread::sleep(TICK);
        }
    });
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Abrir Nudge", true, None::<&str>)?;
    let pause30 = MenuItem::with_id(app, "pause-30", "Pausar 30 minutos", true, None::<&str>)?;
    let pause60 = MenuItem::with_id(app, "pause-60", "Pausar 1 hora", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume", "Reanudar", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Salir", true, None::<&str>)?;
    let sep = || PredefinedMenuItem::separator(app);
    let menu = Menu::with_items(app, &[&open, &sep()?, &pause30, &pause60, &resume, &sep()?, &quit])?;

    let mut tray = TrayIconBuilder::with_id("nudge").tooltip("Nudge").menu(&menu);
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.on_menu_event(|app, event| {
        let state = app.state::<AppState>();
        let result = match event.id().as_ref() {
            "open" => {
                show_main(app);
                Ok(())
            }
            "pause-30" => pause_for(app, &state, Some(30)),
            "pause-60" => pause_for(app, &state, Some(60)),
            "resume" => pause_for(app, &state, None),
            "quit" => {
                app.exit(0);
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(e) = result {
            eprintln!("acción de bandeja fallida: {e}");
        }
    })
    .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Debe registrarse primero: una segunda instancia solo trae al frente la existente.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![MINIMIZED_ARG])))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = Store::open(dir.join("nudge.db"))?;
            app.manage(AppState(Mutex::new(Service::new(store, Local))));

            #[cfg(target_os = "linux")]
            show_all_title_buttons();
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_theme(saved_theme(app.handle()));
            }

            build_tray(app.handle())?;
            if !std::env::args().any(|a| a == MINIMIZED_ARG) {
                show_main(app.handle());
            }
            spawn_scheduler(app.handle().clone());
            spawn_update_checker(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Cerrar la ventana principal la oculta: Nudge sigue en la bandeja.
            if let WindowEvent::CloseRequested { api, .. } = event
                && window.label() == "main"
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_reminders,
            create_reminder,
            update_reminder,
            set_enabled,
            delete_reminder,
            pending_alerts,
            act,
            history,
            status,
            pause,
            set_autostart,
            get_theme,
            set_theme,
            stats,
            export_config,
            import_config,
            list_sounds,
            add_sound,
            remove_sound,
            read_sound,
            check_update,
            install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error al iniciar Nudge");
}
