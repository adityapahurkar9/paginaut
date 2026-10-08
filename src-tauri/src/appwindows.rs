//! Paginaut's windows. One process keeps every window, so documents stay open in the one engine: a tab moved to
//! another window takes its open document along (with unsaved changes and undo), nothing is saved or opened again.
//! A launch without files opens a new window; files opened from Explorer go to the window used last.
//!
//! The first window is "main" (tauri.conf.json); the others are "w-1", "w-2"… (capabilities/default.json names
//! both).

use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// What a new window starts with: files to open, or a tab moved from another window (the page's own record of
/// it, see windows.ts).
#[derive(Serialize, Default)]
pub struct WindowStart {
    pub files: Vec<String>,
    pub tab: Option<serde_json::Value>,
}

/// What a window told us about itself (see `window_report`).
#[derive(Default)]
struct Report {
    title: String,
    dirty: bool,
}

pub struct Windows {
    starts: Mutex<HashMap<String, WindowStart>>,
    reports: Mutex<HashMap<String, Report>>,
    last_focused: Mutex<String>,
    next: AtomicU64,
}

impl Windows {
    /// "main" starts with the files Paginaut was started with.
    pub fn new(launch: Vec<String>) -> Self {
        let starts = HashMap::from([("main".to_owned(), WindowStart { files: launch, tab: None })]);
        Windows { starts: Mutex::new(starts), reports: Mutex::default(), last_focused: Mutex::new("main".into()), next: AtomicU64::new(1) }
    }
}

/// A window's number: 0 for "main", n for "w-n". The page uses it to keep its request ids apart from other
/// windows' (they share the engine's cancel ids).
pub fn window_index(label: &str) -> u64 {
    label.strip_prefix("w-").and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// Each window's Stop for a kind of job, so a Stop in one window leaves another window's job running.
pub struct StopFlags(Mutex<Option<HashMap<String, Arc<AtomicBool>>>>);

impl StopFlags {
    pub const fn new() -> Self {
        StopFlags(Mutex::new(None))
    }
    /// A fresh flag for a job starting in `label`.
    pub fn start(&self, label: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.0.lock().get_or_insert_with(HashMap::new).insert(label.to_owned(), flag.clone());
        flag
    }
    pub fn stop(&self, label: &str) {
        if let Some(f) = self.0.lock().as_ref().and_then(|m| m.get(label)) {
            f.store(true, Ordering::Release);
        }
    }
}

/// Window setup that needs the window itself (each window, "main" included): touchpad pinches and Snap Layouts on
/// the maximize button. Runs on the UI thread, where the window's own child windows must be made.
pub fn window_ready(win: &WebviewWindow) {
    #[cfg(windows)]
    {
        let w = win.clone();
        let _ = win.run_on_main_thread(move || {
            crate::enable_touchpad_pinch(&w);
            if let Ok(hwnd) = w.hwnd() {
                if let Err(e) = crate::snap::install(w.app_handle(), hwnd.0 as isize, w.label()) {
                    eprintln!("snap layouts: {e}");
                }
            }
        });
    }
    #[cfg(not(windows))]
    let _ = win;
}

/// Keeps track of the window used last and forgets closed ones.
pub fn on_window_event(win: &tauri::Window, event: &tauri::WindowEvent) {
    let state = win.state::<Windows>();
    match event {
        tauri::WindowEvent::Focused(true) => *state.last_focused.lock() = win.label().to_owned(),
        tauri::WindowEvent::Destroyed => {
            state.reports.lock().remove(win.label());
            state.starts.lock().remove(win.label());
            #[cfg(windows)]
            crate::snap::forget(win.label());
        }
        _ => {}
    }
}

/// The window used last (another one if it's gone).
fn last_window(app: &AppHandle) -> Option<WebviewWindow> {
    let label = app.state::<Windows>().last_focused.lock().clone();
    app.get_webview_window(&label).or_else(|| app.webview_windows().into_values().next())
}

/// Opens a new window, with its top-left at `at` (physical pixels on the screen) or a little below and right of
/// the window used last.
pub fn open_window(app: &AppHandle, at: Option<PhysicalPosition<f64>>, start: WindowStart) -> Result<WebviewWindow, String> {
    let state = app.state::<Windows>();
    let label = format!("w-{}", state.next.fetch_add(1, Ordering::Relaxed));
    state.starts.lock().insert(label.clone(), start);
    // The size of the window used last (unless maximized: then the usual size).
    let last = last_window(app);
    let (mut w, mut h) = (1280.0, 860.0);
    let mut pos = None;
    if let Some(l) = &last {
        let scale = l.scale_factor().unwrap_or(1.0);
        if !l.is_maximized().unwrap_or(false) {
            if let Ok(s) = l.inner_size() {
                (w, h) = (s.width as f64 / scale, s.height as f64 / scale);
            }
        }
        if at.is_none() {
            if let Ok(p) = l.outer_position() {
                pos = Some(PhysicalPosition::new(p.x as f64 + 32.0 * scale, p.y as f64 + 32.0 * scale));
            }
        }
    }
    let mut b = WebviewWindowBuilder::new(app, &label, WebviewUrl::default())
        .title("Paginaut")
        .inner_size(w, h)
        .min_inner_size(560.0, 400.0)
        .decorations(false)
        .visible(false);
    if at.is_none() && pos.is_none() {
        b = b.center();
    }
    let win = b.build().map_err(|e| {
        state.starts.lock().remove(&label);
        e.to_string()
    })?;
    if let Some(p) = at.or(pos) {
        let _ = win.set_position(p);
    }
    window_ready(&win);
    Ok(win)
}

/// Files from a second launch (or Explorer): into the window used last; no files opens a new window.
pub fn forward_files(app: &AppHandle, files: Vec<String>) {
    if files.is_empty() {
        if let Err(e) = open_window(app, None, WindowStart::default()) {
            eprintln!("new window: {e}");
        }
        return;
    }
    if let Some(w) = last_window(app) {
        let _ = w.unminimize();
        let _ = w.set_focus();
        let _ = app.emit_to(w.label(), "open-files", files);
    }
}

/// Some other window has unsaved changes (an update must wait for them).
pub fn others_dirty(app: &AppHandle, label: &str) -> bool {
    app.state::<Windows>().reports.lock().iter().any(|(l, r)| l != label && r.dirty)
}

// ---------------------------------------------------------------- commands

/// What this window starts with (taken once).
#[tauri::command]
pub fn window_start(window: WebviewWindow, state: State<'_, Windows>) -> WindowStart {
    state.starts.lock().remove(window.label()).unwrap_or_default()
}

/// A new window; with `tab`, that tab moves into it, and with `at_cursor` the window opens where the mouse is
/// (a tab dragged out of the strip and dropped there). Async: on Windows a window made from a command that runs
/// on the UI thread would wait for itself.
#[tauri::command]
pub async fn window_new(app: AppHandle, tab: Option<serde_json::Value>, at_cursor: bool) -> Result<String, String> {
    let at = if at_cursor {
        // The tab lands under the mouse: the window's top-left a little up and left of it.
        app.cursor_position().ok().map(|c| {
            let scale = app.monitor_from_point(c.x, c.y).ok().flatten().map(|m| m.scale_factor()).unwrap_or(1.0);
            PhysicalPosition::new(c.x - 120.0 * scale, c.y - 18.0 * scale)
        })
    } else {
        None
    };
    open_window(&app, at, WindowStart { files: Vec::new(), tab }).map(|w| w.label().to_owned())
}

#[derive(Serialize)]
pub struct WindowEntry {
    label: String,
    title: String,
}

/// The other windows, by their titles (for "Move tab to window").
#[tauri::command]
pub fn window_list(window: WebviewWindow, state: State<'_, Windows>) -> Vec<WindowEntry> {
    let reports = state.reports.lock();
    let mut list: Vec<WindowEntry> = window
        .app_handle()
        .webview_windows()
        .into_keys()
        .filter(|l| l != window.label())
        .map(|label| {
            let title = reports.get(&label).map(|r| r.title.clone()).unwrap_or_else(|| "Paginaut".into());
            WindowEntry { label, title }
        })
        .collect();
    list.sort_by_key(|e| window_index(&e.label));
    list
}

/// Moves a tab into window `label` (`at_x`: where along its tab strip it was dropped, in its page's pixels).
#[tauri::command]
pub fn tab_send(app: AppHandle, label: String, tab: serde_json::Value, at_x: Option<f64>) -> Result<(), String> {
    let win = app.get_webview_window(&label).ok_or("That window has closed.")?;
    win.emit_to(label.as_str(), "tab-arrive", serde_json::json!({ "tab": tab, "atX": at_x })).map_err(|e| e.to_string())?;
    let _ = win.unminimize();
    let _ = win.set_focus();
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DropTarget {
    label: String,
    /// The mouse across that window, in its page's pixels.
    x: f64,
}

/// Height of the title bar with the tab strip, in page pixels (App.css .titlebar).
const STRIP: f64 = 44.0;

/// The other Paginaut window whose tab strip is under the mouse, if any (a tab dropped there joins it).
#[tauri::command]
pub fn window_at(window: WebviewWindow) -> Option<DropTarget> {
    let app = window.app_handle();
    let c = app.cursor_position().ok()?;
    let wins = app.webview_windows();
    // The window on top at that point, as Windows sees it (another app's window may cover ours).
    #[cfg(windows)]
    let target = unsafe {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, WindowFromPoint, GA_ROOT};
        let hit = GetAncestor(WindowFromPoint(POINT { x: c.x as i32, y: c.y as i32 }), GA_ROOT);
        wins.values().find(|w| w.hwnd().is_ok_and(|h| h.0 == hit.0)).cloned()
    };
    #[cfg(not(windows))]
    let target = wins
        .values()
        .find(|w| {
            let (Ok(p), Ok(s)) = (w.inner_position(), w.inner_size()) else { return false };
            w.is_visible().unwrap_or(false)
                && !w.is_minimized().unwrap_or(true)
                && c.x >= p.x as f64
                && c.x < p.x as f64 + s.width as f64
                && c.y >= p.y as f64
                && c.y < p.y as f64 + s.height as f64
        })
        .cloned();
    let target = target.filter(|w| w.label() != window.label())?;
    let p = target.inner_position().ok()?;
    let scale = target.scale_factor().unwrap_or(1.0);
    let (x, y) = ((c.x - p.x as f64) / scale, (c.y - p.y as f64) / scale);
    (y >= 0.0 && y < STRIP).then(|| DropTarget { label: target.label().to_owned(), x })
}

/// The window's title (its current document, for the taskbar and Windows' own Alt+Tab), what the other windows'
/// menus call it (its documents), and whether it has unsaved changes.
#[tauri::command]
pub fn window_report(window: WebviewWindow, state: State<'_, Windows>, title: String, summary: String, dirty: bool) {
    let full = if title.is_empty() { "Paginaut".to_owned() } else { format!("{title} - Paginaut") };
    let _ = window.set_title(&full);
    state.reports.lock().insert(window.label().to_owned(), Report { title: summary, dirty });
}

/// Exit Paginaut: every window is asked to close (each asks about its own unsaved changes).
#[tauri::command]
pub fn quit_all(app: AppHandle) {
    for w in app.webview_windows().into_values() {
        let _ = w.close();
    }
}
