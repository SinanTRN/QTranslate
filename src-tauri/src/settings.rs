use serde_json::Value;
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

const STORE: &str = "settings.json";

pub const KEY_DEEPL: &str = "deepl_key";
pub const KEY_SHORTCUT: &str = "shortcut";

pub const DEFAULT_SHORTCUT: &str = "Alt+Space";

pub fn get_string<R: Runtime>(app: &AppHandle<R>, key: &str) -> Option<String> {
    let store = app.store(STORE).ok()?;
    store
        .get(key)?
        .as_str()
        .map(str::to_owned)
        .filter(|s| !s.is_empty())
}

pub fn set_string<R: Runtime>(app: &AppHandle<R>, key: &str, value: &str) -> Result<(), String> {
    let store = app.store(STORE).map_err(|e| e.to_string())?;
    store.set(key, Value::String(value.to_owned()));
    store
        .save()
        .map_err(|e| format!("Ayarlar kaydedilemedi: {e}"))
}
