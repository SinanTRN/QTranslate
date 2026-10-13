#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod settings;
mod shortcut;
mod translate;

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, State, WebviewWindow, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::ShortcutState as KeyState;

use settings::KEY_DEEPL;
use shortcut::ShortcutState;

const WIDTH: f64 = 640.0;
/// Pencere ekranın üstünden bu oranda aşağıda açılır.
const TOP_RATIO: f64 = 0.22;

struct Http(reqwest::Client);

/// Gösterildikten hemen sonra gelen geçici odak kayıplarını yok saymak için.
struct ShownAt(Mutex<Instant>);

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

/// Pencereyi imlecin bulunduğu ekranda, yatayda ortalı konumlandırır.
fn position(app: &AppHandle, w: &WebviewWindow) {
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| w.primary_monitor().ok().flatten());
    let Some(m) = monitor else { return };

    let width = (WIDTH * m.scale_factor()) as i32;
    let (pos, size) = (m.position(), m.size());
    let x = pos.x + (size.width as i32 - width) / 2;
    let y = pos.y + (size.height as f64 * TOP_RATIO) as i32;
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

fn show(app: &AppHandle) {
    let Some(w) = main_window(app) else { return };
    *app.state::<ShownAt>().0.lock().unwrap() = Instant::now();
    position(app, &w);
    let _ = w.show();
    let _ = w.set_focus();
    let _ = app.emit("shown", ());
}

fn hide(app: &AppHandle) {
    let Some(w) = main_window(app) else { return };
    let _ = w.hide();
    shortcut::ensure_registered(app);
    let _ = app.emit("hidden", ());
}

fn toggle(app: &AppHandle) {
    let Some(w) = main_window(app) else { return };
    if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) {
        hide(app);
    } else {
        show(app);
    }
}

// ---------- Komutlar ----------

#[tauri::command]
async fn translate(
    app: AppHandle,
    http: State<'_, Http>,
    text: String,
    mode: Option<String>,
) -> Result<translate::Translation, String> {
    let key = settings::get_string(&app, KEY_DEEPL)
        .ok_or_else(|| "Önce ayarlardan bir DeepL API anahtarı gir.".to_string())?;
    let mode = translate::Mode::parse(mode.as_deref());
    translate::translate(&http.0, &key, &text, mode).await
}

#[tauri::command]
fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    app.clipboard()
        .write_text(text)
        .map_err(|e| format!("Panoya kopyalanamadı: {e}"))
}

#[tauri::command]
fn hide_window(app: AppHandle) {
    hide(&app);
}

#[tauri::command]
fn resize(window: WebviewWindow, height: f64) {
    let _ = window.set_size(LogicalSize::new(WIDTH, height.clamp(64.0, 720.0)));
}

#[derive(Serialize)]
struct SettingsView {
    has_key: bool,
    shortcut: String,
    autostart: bool,
    shortcut_error: Option<String>,
}

fn settings_view(app: &AppHandle) -> SettingsView {
    let sc = app.state::<ShortcutState>();
    let shortcut = sc.current.lock().unwrap().clone();
    let shortcut_error = sc.error.lock().unwrap().clone();
    SettingsView {
        has_key: settings::get_string(app, KEY_DEEPL).is_some(),
        shortcut,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        shortcut_error,
    }
}

#[tauri::command]
fn get_settings(app: AppHandle) -> SettingsView {
    settings_view(&app)
}

/// Boş anahtar gönderilirse kayıtlı anahtar korunur.
#[tauri::command]
fn save_settings(app: AppHandle, deepl_key: String, autostart: bool) -> Result<SettingsView, String> {
    let key = deepl_key.trim();
    if !key.is_empty() {
        settings::set_string(&app, KEY_DEEPL, key)?;
    }

    let launcher = app.autolaunch();
    if launcher.is_enabled().unwrap_or(false) != autostart {
        let res = if autostart { launcher.enable() } else { launcher.disable() };
        res.map_err(|e| format!("Otomatik başlatma ayarlanamadı: {e}"))?;
    }

    Ok(settings_view(&app))
}

// ---------- Kurulum ----------

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Aç", true, None::<&str>)?;
    let prefs = MenuItem::with_id(app, "settings", "Ayarlar", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Çıkış", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &prefs, &quit])?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("QTranslate")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id.as_ref() {
            "open" => show(app),
            "settings" => {
                show(app);
                let _ = app.emit("open-settings", ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = e
            {
                show(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        // Tekrar çalıştırılırsa yeni süreç açmak yerine mevcut pencereyi göster.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, event| {
                    if event.state() == KeyState::Pressed {
                        toggle(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();

            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()?;
            app.manage(Http(client));
            app.manage(ShownAt(Mutex::new(Instant::now())));
            app.manage(shortcut::init(&handle));

            setup_tray(&handle)?;

            // İlk çalıştırmada (anahtar yokken) pencereyi aç ki ayarlar görünsün.
            if settings::get_string(&handle, KEY_DEEPL).is_none() {
                show(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(false) => {
                let app = window.app_handle();
                let shown_at = *app.state::<ShownAt>().0.lock().unwrap();
                if shown_at.elapsed() > Duration::from_millis(250) {
                    hide(app);
                }
            }
            // Alt+F4 vb. uygulamayı kapatmasın, sadece gizlesin.
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                hide(window.app_handle());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            translate,
            copy_text,
            hide_window,
            resize,
            get_settings,
            save_settings,
            shortcut::set_shortcut,
            shortcut::suspend_shortcut,
            shortcut::resume_shortcut,
        ])
        .run(tauri::generate_context!())
        .expect("QTranslate başlatılamadı");
}
