//! "Report a problem": the error log (errlog.rs) and the details of this computer or phone that help make sense of
//! it, gathered only when someone opens the report. Nothing here goes anywhere by itself: the page shows it, and the
//! user copies it, saves it, or opens a GitHub issue (desktop) or shares it (Android).

use tauri::{AppHandle, Runtime};

/// A line for the log from the page: a script error, a promise nobody caught, a command that failed.
#[tauri::command]
pub fn log_error(kind: String, message: String) {
    // The kind is one short word (it shows in brackets); anything else is the page's own mistake.
    let kind: String = kind.chars().filter(|c| c.is_ascii_alphanumeric()).take(12).collect();
    crate::errlog::write(if kind.is_empty() { "page" } else { &kind }, &message);
}

/// The whole error log, oldest first (already cleaned of paths and names).
#[tauri::command]
pub fn error_log() -> String {
    crate::errlog::read()
}

/// True once, when the last session crashed (or, on the desktop, ended without closing): the page offers a report.
#[tauri::command]
pub fn crash_notice() -> bool {
    crate::errlog::take_crash_notice()
}

/// Empties the error log.
#[tauri::command]
pub fn error_log_clear() {
    crate::errlog::clear();
}

/// What the app knows about where it runs, as label and value: version, system, processor, memory, web view.
#[tauri::command]
pub fn system_details<R: Runtime>(app: AppHandle<R>) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut add = |k: &str, v: String| {
        if !v.trim().is_empty() {
            rows.push((k.to_string(), v));
        }
    };
    add("Paginaut", app.package_info().version.to_string());
    let os = sysinfo::System::long_os_version().unwrap_or_else(|| std::env::consts::OS.to_string());
    let kernel = sysinfo::System::kernel_version().map(|k| format!(" (kernel {k})")).unwrap_or_default();
    add("System", format!("{os}{kernel}"));
    add("Architecture", std::env::consts::ARCH.to_string());
    let mut sys = sysinfo::System::new();
    sys.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
    sys.refresh_memory();
    let brand = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    add("Processor", if brand.is_empty() { format!("{threads} threads") } else { format!("{brand}, {threads} threads") });
    add("Memory", format!("{} total, {} free", gib(sys.total_memory()), gib(sys.available_memory())));
    #[cfg(desktop)]
    add(if cfg!(windows) { "WebView2" } else { "WebKitGTK" }, tauri::webview_version().unwrap_or_default());
    #[cfg(target_os = "linux")]
    {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        add("Session", [env("XDG_SESSION_TYPE"), env("XDG_CURRENT_DESKTOP")].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(", "));
        add("Installed as", if std::env::var_os("APPIMAGE").is_some() { "AppImage".into() } else { "package (.deb)".into() });
    }
    rows
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
}

/// Saves the report where the user chose (desktop; Android shares it instead).
#[cfg(desktop)]
#[tauri::command]
pub async fn report_save(path: String, text: String) -> crate::app::Res<()> {
    tauri::async_runtime::spawn_blocking(move || std::fs::write(&path, text).map_err(|e| format!("Couldn't save the report: {e}")))
        .await
        .map_err(|e| e.to_string())?
}
