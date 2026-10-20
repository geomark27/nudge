use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use nudge_core::{Action, Alert, Occurrence, Reminder, ReminderSpec, Service, Store, Theme};
use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_notification::NotificationExt;

/// Cada cuánto se revisan los recordatorios vencidos.
const TICK: Duration = Duration::from_secs(15);
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
    notify_changed(&app);
    Ok(())
}

#[tauri::command]
fn pending_alerts(state: State<'_, AppState>) -> CmdResult<Vec<Alert>> {
    with_service(&state, |s| s.pending(Utc::now()))
}

#[tauri::command]
fn act(app: AppHandle, state: State<'_, AppState>, id: i64, action: Action) -> CmdResult<()> {
    let pending = with_service(&state, |s| {
        s.act(id, action, Utc::now())?;
        s.pending(Utc::now())
    })?;
    if pending.is_empty()
        && let Some(w) = app.get_webview_window("alert")
    {
        let _ = w.close();
    }
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
    })
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
    for w in app.webview_windows().values() {
        let _ = w.set_theme(native_theme(theme));
    }
    let _ = app.emit("theme-changed", theme);
    Ok(())
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
        ])
        .run(tauri::generate_context!())
        .expect("error al iniciar Nudge");
}
