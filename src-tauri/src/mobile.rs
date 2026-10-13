//! The Android viewer: the same engine and `paginaut://` pixel protocol as the desktop app,
//! with only the commands needed to read documents. Android's own services (the file picker,
//! "Open with", Share, printing) are in the Kotlin plugin, PaginautPlugin.kt; the page reaches
//! it through the commands here.

use crate::app::{self, blocking, AppState, Res};
use crate::engine::{DocId, DocInfo};
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use tauri::plugin::{Builder as PluginBuilder, PluginHandle, TauriPlugin};
use tauri::{Manager, Runtime, State, Wry};

pub fn run() {
    // A viewer fills in no forms: PDF scripts (form calculations, formatting) stay off.
    crate::engine::mupdf::allow_scripts(false);
    let engine = crate::engine::Engine::new(crate::worker_count(), crate::store_bytes()).expect("MuPDF failed to start");
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(native())
        .manage(AppState { engine, started: std::time::Instant::now() })
        .register_asynchronous_uri_scheme_protocol("paginaut", app::protocol)
        .invoke_handler(tauri::generate_handler![
            open_document,
            app::unlock_document,
            app::close_document,
            app::trim_document,
            app::properties,
            app::security,
            app::page_sizes,
            app::page_label,
            app::page_labels,
            app::outline,
            app::page_links,
            app::page_images,
            app::text_select,
            app::text_copy,
            app::text_range,
            app::text_range_copy,
            app::page_text,
            app::page_text_stats,
            app::search,
            app::reflow,
            app::annots,
            app::doc_annots,
            app::user_name,
            app::cancel,
            pick_file,
            take_intent,
            reopen,
            share,
            print,
            set_bars,
            set_secure,
            safe_area,
            speak,
            stop_speaking,
            storage,
            drop_copies,
            add_folder,
            remove_folder,
            all_files_access,
            scan_books,
            book_cover,
            save_repaired,
            pick_books,
            forget_picked,
            share_text,
            release,
        ])
        .build(tauri::generate_context!())
        .expect("error while running Paginaut")
        .run(|_, event| {
            // Closing the app: leave at once. A normal exit runs C++ static destructors (HarfBuzz's,
            // libc++'s) while engine threads may still be laying out or drawing a page, and
            // Android aborts on the first lock of a destroyed mutex: a crash report for a clean exit.
            if let tauri::RunEvent::Exit = event {
                extern "C" {
                    fn _exit(code: i32) -> !;
                }
                // SAFETY: nothing is left to save (reading positions are saved by the page as you
                // read and when the app goes to the background).
                unsafe { _exit(0) }
            }
        });
}

/// The Kotlin side, as an app-internal plugin: its calls are made from the commands below,
/// so the page needs no plugin permissions.
struct Native<R: Runtime>(PluginHandle<R>);

fn native<R: Runtime>() -> TauriPlugin<R> {
    PluginBuilder::new("paginaut")
        .setup(|app, api| {
            // Solid RAR comic books are unpacked here while open (std's temp dir isn't writable on Android).
            if let Ok(dir) = app.path().app_cache_dir() {
                crate::engine::mupdf::set_temp_dir(&dir.join("unpacked"));
                let _ = std::fs::create_dir_all(&dir);
                if let Ok(real) = std::fs::canonicalize(&dir) {
                    let _ = CACHE.set(real);
                }
            }
            #[cfg(target_os = "android")]
            {
                let handle = api.register_android_plugin("app.paginaut.reader", "PaginautPlugin")?;
                app.manage(Native(handle));
            }
            #[cfg(not(target_os = "android"))]
            let _ = (app, api);
            Ok(())
        })
        .build()
}

/// Runs a Kotlin command and waits for it (off the async runtime: some wait on the user).
async fn call(native: &Native<Wry>, cmd: &'static str, args: Value) -> Res<Value> {
    let handle = native.0.clone();
    blocking(move || handle.run_mobile_plugin::<Value>(cmd, args).map_err(|e| e.to_string())).await
}

/// A document ready for the engine, from where Android gave it: `{ path, name, uri, inPlace,
/// lasting, kept, fp }`, or `{}` when nothing was picked or sent.
#[tauri::command]
async fn pick_file(native: State<'_, Native<Wry>>) -> Res<Value> {
    call(&native, "pickFile", json!({})).await
}

#[tauri::command]
async fn take_intent(native: State<'_, Native<Wry>>) -> Res<Value> {
    call(&native, "takeIntent", json!({})).await
}

#[tauri::command]
async fn reopen(native: State<'_, Native<Wry>>, uri: String) -> Res<Value> {
    call(&native, "reopen", json!({ "uri": uri })).await
}

#[tauri::command]
async fn share(native: State<'_, Native<Wry>>, path: String, source: Option<String>) -> Res<()> {
    call(&native, "share", json!({ "path": path, "source": source })).await.map(|_| ())
}

/// A big file read in place (or copied for one visit) is let go once its document has closed.
#[tauri::command]
async fn release(native: State<'_, Native<Wry>>, path: String) -> Res<()> {
    call(&native, "release", json!({ "path": path })).await.map(|_| ())
}

#[tauri::command]
async fn share_text(native: State<'_, Native<Wry>>, text: String) -> Res<()> {
    call(&native, "shareText", json!({ "text": text })).await.map(|_| ())
}

/// Android's print service takes PDFs: other documents (e-books, XPS, comics) are written as a
/// PDF in the cache first (never beside the book: Paginaut doesn't write where your files are).
#[tauri::command]
async fn print(native: State<'_, Native<Wry>>, state: State<'_, AppState>, doc: DocId, path: String, title: String, pdf: bool) -> Res<()> {
    readable(&path)?;
    let file = if pdf {
        path
    } else {
        let dir = cache()?.join("print");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let out = dir.join("print.pdf").to_string_lossy().into_owned();
        let engine = state.engine.clone();
        let target = out.clone();
        blocking(move || engine.export_pdf(doc, &target)).await?;
        out
    };
    call(&native, "print", json!({ "path": file, "title": title })).await.map(|_| ())
}

/// The status and navigation bars: dark icons or light, hidden while reading.
#[tauri::command]
async fn set_bars(native: State<'_, Native<Wry>>, dark: bool, immersive: bool) -> Res<()> {
    call(&native, "setBars", json!({ "dark": dark, "immersive": immersive })).await.map(|_| ())
}

/// Hide from screenshots: Android's secure window (remembered by the Kotlin side for the next start).
#[tauri::command]
async fn set_secure(native: State<'_, Native<Wry>>, on: bool) -> Res<()> {
    call(&native, "setSecure", json!({ "on": on })).await.map(|_| ())
}

/// What the page must keep clear of (bars, camera cutout, keyboard): `{ top, right, bottom, left }` in CSS px.
#[tauri::command]
async fn safe_area(native: State<'_, Native<Wry>>) -> Res<Value> {
    call(&native, "safeArea", json!({})).await
}

/// Reads `text` aloud with Android's text-to-speech; resolves once spoken: `{ stopped }`.
#[tauri::command]
async fn speak(native: State<'_, Native<Wry>>, text: String, rate: f64, lang: Option<String>) -> Res<Value> {
    call(&native, "speak", json!({ "text": text, "rate": rate, "lang": lang })).await
}

#[tauri::command]
async fn stop_speaking(native: State<'_, Native<Wry>>) -> Res<()> {
    call(&native, "stopSpeaking", json!({})).await.map(|_| ())
}

/// What the app keeps and can reach: `{ copies, picked, folders, allFiles }`.
#[tauri::command]
async fn storage(native: State<'_, Native<Wry>>) -> Res<Value> {
    call(&native, "storage", json!({})).await
}

/// Deletes these copies from the cache (the page decides which).
#[tauri::command]
async fn drop_copies(native: State<'_, Native<Wry>>, paths: Vec<String>) -> Res<()> {
    call(&native, "dropCopies", json!({ "paths": paths })).await.map(|_| ())
}

/// Asks for a folder for the shelf: `{ uri, name }`, or `{}` if none was chosen.
#[tauri::command]
async fn add_folder(native: State<'_, Native<Wry>>) -> Res<Value> {
    call(&native, "addFolder", json!({})).await
}

#[tauri::command]
async fn remove_folder(native: State<'_, Native<Wry>>, uri: String) -> Res<()> {
    call(&native, "removeFolder", json!({ "uri": uri })).await.map(|_| ())
}

/// Shows where "All files access" is turned on or off.
#[tauri::command]
async fn all_files_access(native: State<'_, Native<Wry>>) -> Res<()> {
    call(&native, "allFilesAccess", json!({})).await.map(|_| ())
}

/// "Search and pick books": Android's picker, e-books and comics only; the books picked join the shelf
/// (`{ added, already, notBooks, noRoom }`).
#[tauri::command]
async fn pick_books(native: State<'_, Native<Wry>>) -> Res<Value> {
    call(&native, "pickBooks", json!({})).await
}

/// Takes the picked books off the shelf (files in Recent, `keep`, still open from there).
#[tauri::command]
async fn forget_picked(native: State<'_, Native<Wry>>, keep: Vec<String>) -> Res<()> {
    call(&native, "forgetPicked", json!({ "keep": keep })).await.map(|_| ())
}

/// The e-books and comics the shelf can reach: `{ items: [{ id, name, size, mtime, fp, kind }] }`.
#[tauri::command]
async fn scan_books(native: State<'_, Native<Wry>>) -> Res<Value> {
    call(&native, "scanBooks", json!({})).await
}

// ---------------------------------------------------------------- what the engine may touch
//
// The page names files by path. Kotlin only ever gives it Paginaut's own copies and links (in the
// cache) or, with all-files access, books on shared storage; these checks hold the engine to that
// whatever the page asks: never the app's own data, other apps' folders, or a file type Paginaut
// doesn't read, and it writes only into its cache.

/// The app's cache folder, links resolved (set at start).
static CACHE: OnceLock<PathBuf> = OnceLock::new();

fn cache() -> Res<&'static Path> {
    CACHE.get().map(PathBuf::as_path).ok_or_else(|| "Paginaut isn't ready yet.".to_string())
}

/// What Paginaut reads from shared storage (the shelf's books, and documents it opens).
const DOCUMENTS: &[&str] = &["pdf", "xps", "oxps", "epub", "mobi", "fb2", "fbz", "cbz", "cbr", "cbt", "md", "markdown", "txt"];

const REFUSED: &str = "Paginaut doesn't open files from there.";

/// `path`'s folder with links and `..` resolved. A folder not made yet (somewhere to write) is
/// its nearest existing one, plus plain names.
fn real_dir(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let mut base = dir;
    let mut rest = Vec::new();
    loop {
        if let Ok(real) = std::fs::canonicalize(base) {
            let mut out = real;
            for name in rest.iter().rev() {
                out.push(name);
            }
            return Some(out);
        }
        match base.components().next_back()? {
            Component::Normal(name) => rest.push(name.to_owned()),
            _ => return None,
        }
        base = base.parent()?;
    }
}

/// Inside the cache (the file itself may be a link: big files are read in place through one).
fn in_cache(path: &Path) -> bool {
    matches!((cache(), real_dir(path)), (Ok(c), Some(d)) if d.starts_with(c))
}

/// A document on shared storage (the phone's own, or a card): a plain file, not a link, of a type
/// Paginaut reads, and not in another app's data or obb folder (Android/media, where WhatsApp
/// keeps documents, is fine).
fn shared_document(path: &Path) -> bool {
    let Some(dir) = real_dir(path) else { return false };
    let Ok(meta) = std::fs::symlink_metadata(path) else { return false };
    if !meta.is_file() {
        return false;
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !DOCUMENTS.contains(&ext.as_str()) {
        return false;
    }
    let parts: Vec<String> = dir.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    // ["/", "storage", "emulated", "0", …] or ["/", "storage", "1A2B-3C4D", …]
    if parts.get(1).map(String::as_str) != Some("storage") {
        return false;
    }
    let skip = if parts.get(2).map(String::as_str) == Some("emulated") { 4 } else { 3 };
    if parts.len() < skip {
        return false;
    }
    let rest = &parts[skip..];
    !(rest.first().is_some_and(|p| p.eq_ignore_ascii_case("android")) && rest.get(1).is_some_and(|p| p.eq_ignore_ascii_case("data") || p.eq_ignore_ascii_case("obb")))
}

/// The engine may read `path`.
fn readable(path: &str) -> Res<()> {
    let p = Path::new(path);
    if in_cache(p) || shared_document(p) {
        Ok(())
    } else {
        Err(REFUSED.into())
    }
}

/// The engine may write `path`: only into the cache.
fn writable(path: &str) -> Res<()> {
    if in_cache(Path::new(path)) {
        Ok(())
    } else {
        Err("Paginaut only writes into its own cache.".into())
    }
}

#[tauri::command]
async fn open_document(state: State<'_, AppState>, path: String, engine: Option<String>) -> Res<DocInfo> {
    readable(&path)?;
    app::open_document(state, path, engine).await
}

#[tauri::command]
async fn book_cover(app: tauri::AppHandle, state: State<'_, AppState>, path: String, fp: String) -> Res<Value> {
    readable(&path)?;
    app::book_cover(app, state, path, fp).await
}

#[tauri::command]
async fn save_repaired(state: State<'_, AppState>, doc: DocId, target: String, pdf: bool) -> Res<()> {
    writable(&target)?;
    app::save_repaired(state, doc, target, pdf).await
}
