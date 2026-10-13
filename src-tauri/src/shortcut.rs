use std::sync::Mutex;

use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::settings::{self, DEFAULT_SHORTCUT, KEY_SHORTCUT};

/// Kayıtlı kısayol ve (varsa) açılışta kaydedilemediğinde oluşan hata.
pub struct ShortcutState {
    pub current: Mutex<String>,
    pub error: Mutex<Option<String>>,
}

fn register<R: Runtime>(app: &AppHandle<R>, shortcut: &str) -> Result<(), String> {
    app.global_shortcut()
        .register(shortcut)
        .map_err(|e| format!("“{shortcut}” atanamadı: {e}"))
}

/// Açılışta kayıtlı (yoksa varsayılan) kısayolu etkinleştirir.
pub fn init<R: Runtime>(app: &AppHandle<R>) -> ShortcutState {
    let shortcut =
        settings::get_string(app, KEY_SHORTCUT).unwrap_or_else(|| DEFAULT_SHORTCUT.to_owned());
    let error = register(app, &shortcut).err().map(|e| {
        format!("{e}. Başka bir uygulama kullanıyor olabilir, yeni bir kısayol seç.")
    });
    ShortcutState {
        current: Mutex::new(shortcut),
        error: Mutex::new(error),
    }
}

/// Mevcut kısayol kayıtlı değilse yeniden kaydeder.
pub fn ensure_registered<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<ShortcutState>();
    let current = state.current.lock().unwrap().clone();
    let gs = app.global_shortcut();
    if !gs.is_registered(current.as_str()) && register(app, &current).is_ok() {
        *state.error.lock().unwrap() = None;
    }
}

#[tauri::command]
pub fn set_shortcut<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, ShortcutState>,
    shortcut: String,
) -> Result<String, String> {
    let old = state.current.lock().unwrap().clone();
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();

    if let Err(e) = register(&app, &shortcut) {
        let _ = register(&app, &old);
        return Err(e);
    }

    settings::set_string(&app, KEY_SHORTCUT, &shortcut)?;
    *state.current.lock().unwrap() = shortcut.clone();
    *state.error.lock().unwrap() = None;
    Ok(shortcut)
}

/// Kısayol kaydedilirken mevcut kısayolun pencereyi kapatmaması için geçici olarak kaldırır.
#[tauri::command]
pub fn suspend_shortcut<R: Runtime>(app: AppHandle<R>) {
    let _ = app.global_shortcut().unregister_all();
}

#[tauri::command]
pub fn resume_shortcut<R: Runtime>(app: AppHandle<R>) {
    ensure_registered(&app);
}
