//! Settings kept in a file of our own, settings.json in the app's data folder, beside the WebView's storage.
//! The WebView can start its storage over (it did on a reinstall), and the theme and every other choice went
//! with it; this file is ours, so it stays. The page writes both and starts from the newer (store.ts).
//!
//! The file is `{"saved": <ms>, "settings": {...}}`, as the page wrote it. Each window gets it in its page before
//! the page runs (`script`), so settings are there at once, as they were from the WebView's storage.

use parking_lot::Mutex;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// One write at a time (windows write the same file).
static WRITING: Mutex<()> = Mutex::new(());

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("settings.json"))
}

/// The page's start-up line with the saved settings, or None with no file (or one that isn't ours).
pub fn script(app: &AppHandle) -> Option<String> {
    let text = std::fs::read_to_string(path(app).ok()?).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("settings")?.as_object()?;
    // Written again from the parsed value, so only JSON reaches the page.
    Some(format!("window.__paginautSettings = {};", serde_json::to_string(&value).ok()?))
}

#[tauri::command]
pub async fn settings_write(app: AppHandle, json: String) -> crate::app::Res<()> {
    let value: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    if value.get("settings").and_then(|s| s.as_object()).is_none() {
        return Err("not settings".into());
    }
    let file = path(&app)?;
    crate::app::blocking(move || {
        let _one = WRITING.lock();
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        // Whole or not at all: a half-written file would lose every setting.
        let tmp = file.with_extension("tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &file).map_err(|e| e.to_string())
    })
    .await
}
