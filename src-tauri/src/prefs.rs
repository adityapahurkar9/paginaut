//! What the page keeps in the WebView's storage (settings, recent files and reading positions, the shelf, layout
//! fixes…), mirrored in a file of our own: storage.json in the app's data folder. The WebView can start its storage
//! over (it did on a reinstall), and everything went with it; this file is ours, so it stays.
//!
//! The file is `{"saved": <ms>, "items": {"paginaut.…": "<the stored text>", …}}`, as the page wrote it
//! (store.ts). Each window gets it in its page before the page runs (`script`); index.html puts it back in the
//! storage when it's newer than what the storage has.
//!
//! On Linux the file is the user's alone (0600): it lists the files they've opened, and a new file would otherwise
//! be readable by other accounts. On Windows the app's data folder is already the user's alone.

use parking_lot::Mutex;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// One write at a time (windows write the same file).
static WRITING: Mutex<()> = Mutex::new(());

fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

/// Makes a file the user's alone (Linux); an older file keeps its mode otherwise.
#[cfg(unix)]
fn make_private(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

/// Writes a file only the user can read (Linux), created that way so it's never readable by others for a moment.
fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        // A temp file left from before has its own mode (and may be open elsewhere): start a new one.
        let _ = std::fs::remove_file(path);
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
        f.write_all(data)
    }
    #[cfg(not(unix))]
    std::fs::write(path, data)
}

fn is_mirror(v: &serde_json::Value) -> bool {
    v.get("saved").is_some_and(|s| s.is_number()) && v.get("items").and_then(|i| i.as_object()).is_some_and(|i| i.values().all(|x| x.is_string()))
}

/// 0.9.6 kept only the settings, in settings.json (`{"saved", "settings"}`): read as a mirror of that one item.
fn from_settings_file(dir: &std::path::Path) -> Option<serde_json::Value> {
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).ok()?).ok()?;
    let settings = v.get("settings")?.as_object()?;
    Some(serde_json::json!({ "saved": v.get("saved")?.as_f64()?, "items": { "paginaut.settings": serde_json::to_string(settings).ok()? } }))
}

/// The page's start-up line with the mirror, or None without one.
pub fn script(app: &AppHandle) -> Option<String> {
    let dir = dir(app).ok()?;
    // One written before 0.9.8 was readable by other accounts.
    #[cfg(unix)]
    let _ = make_private(&dir.join("storage.json"));
    let value = std::fs::read_to_string(dir.join("storage.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .filter(is_mirror)
        .or_else(|| from_settings_file(&dir))?;
    // Written again from the parsed value, so only JSON reaches the page.
    Some(format!("window.__paginautStorage = {};", serde_json::to_string(&value).ok()?))
}

#[tauri::command]
pub async fn storage_write(app: AppHandle, json: String) -> crate::app::Res<()> {
    let value: serde_json::Value = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    if !is_mirror(&value) {
        return Err("not the page's storage".into());
    }
    let dir = dir(&app)?;
    crate::app::blocking(move || {
        let _one = WRITING.lock();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // Whole or not at all: a half-written file would lose everything in it.
        let (tmp, file) = (dir.join("storage.tmp"), dir.join("storage.json"));
        write_private(&tmp, json.as_bytes()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
        // The settings are in storage.json now.
        let _ = std::fs::remove_file(dir.join("settings.json"));
        Ok(())
    })
    .await
}
