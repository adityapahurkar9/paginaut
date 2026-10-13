//! The Android viewer: the same engine and `paginaut://` pixel protocol as the desktop app,
//! with only the commands needed to read documents. The engine runs in its own isolated process
//! (engine_link.rs); the commands here pass the page's requests to it. Android's own services (the
//! file picker, "Open with", Share, printing) are in the Kotlin plugin, PaginautPlugin.kt.

use crate::app::{self, blocking, Res};
use crate::engine::DocId;
use crate::engine_link::Remote;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::http::{Request, Response, StatusCode};
use tauri::ipc::Channel;
use tauri::{UriSchemeContext, UriSchemeResponder};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use tauri::plugin::{Builder as PluginBuilder, PluginHandle, TauriPlugin};
use tauri::{Manager, Runtime, State, Wry};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(native())
        .register_asynchronous_uri_scheme_protocol("paginaut", protocol)
        .invoke_handler(tauri::generate_handler![
            open_document,
            unlock_document,
            close_document,
            trim_document,
            properties,
            security,
            page_sizes,
            page_label,
            page_labels,
            outline,
            page_links,
            page_images,
            text_select,
            text_copy,
            text_range,
            text_range_copy,
            page_text,
            page_text_stats,
            search,
            reflow,
            annots,
            doc_annots,
            app::user_name,
            cancel,
            engine_crash_test,
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
            let dir = app.path().app_cache_dir()?;
            let _ = std::fs::create_dir_all(&dir);
            if let Ok(real) = std::fs::canonicalize(&dir) {
                let _ = CACHE.set(real);
            }
            #[cfg(target_os = "android")]
            {
                let handle = api.register_android_plugin("app.paginaut.reader", "PaginautPlugin")?;
                // The engine's process, started by the plugin (EngineService); started early, as it takes a moment.
                let h = handle.clone();
                let remote = Remote::new(
                    move || {
                        let v: Value = h.run_mobile_plugin("engineConnect", json!({})).map_err(|e| e.to_string())?;
                        v["fd"].as_i64().map(|fd| fd as i32).ok_or_else(|| "The engine didn't start.".to_string())
                    },
                    &dir,
                    {
                        let app = app.app_handle().clone();
                        move |doc| {
                            use tauri::Emitter;
                            let _ = app.emit("engine-refused", doc);
                        }
                    },
                );
                remote.warm_up();
                app.manage(remote);
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
async fn print(native: State<'_, Native<Wry>>, remote: State<'_, Arc<Remote>>, doc: DocId, path: String, title: String, pdf: bool) -> Res<()> {
    readable(&path)?;
    let file = if pdf {
        path
    } else {
        let dir = cache()?.join("print");
        let _ = std::fs::remove_dir_all(&dir);
        let out = dir.join("print.pdf").to_string_lossy().into_owned();
        let (r, target) = (remote.inner().clone(), out.clone());
        blocking(move || r.write_to("export_pdf", doc, &target, json!({}))).await?;
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

// ---------------------------------------------------------------- the engine's commands

/// Runs `f` with the engine's connection, off the async runtime (calls wait for the engine).
async fn with<T: Send + 'static>(remote: &State<'_, Arc<Remote>>, f: impl FnOnce(&Remote) -> Res<T> + Send + 'static) -> Res<T> {
    let r = remote.inner().clone();
    blocking(move || f(&r)).await
}

#[tauri::command]
async fn open_document(remote: State<'_, Arc<Remote>>, path: String) -> Res<Value> {
    readable(&path)?;
    with(&remote, move |r| r.open(&path)).await
}

#[tauri::command]
async fn unlock_document(remote: State<'_, Arc<Remote>>, doc: DocId, password: String) -> Res<Value> {
    with(&remote, move |r| r.unlock(doc, &password)).await
}

#[tauri::command]
async fn close_document(remote: State<'_, Arc<Remote>>, doc: DocId) -> Res<()> {
    with(&remote, move |r| Ok(r.close(doc))).await
}

/// A command about one document, passed on as it is.
macro_rules! forward {
    ($name:ident, $cmd:literal $(, $arg:ident: $ty:ty)*) => {
        #[tauri::command]
        async fn $name(remote: State<'_, Arc<Remote>>, doc: DocId $(, $arg: $ty)*) -> Res<Value> {
            #[allow(unused_mut)]
            let mut args = serde_json::Map::new();
            $(args.insert(stringify!($arg).into(), json!($arg));)*
            with(&remote, move |r| r.doc_value($cmd, doc, Value::Object(args))).await
        }
    };
}

forward!(trim_document, "trim");
forward!(properties, "properties");
forward!(security, "security");
forward!(page_sizes, "page_sizes");
forward!(page_label, "page_label", page: usize);
forward!(page_labels, "page_labels");
forward!(outline, "outline");
forward!(page_links, "page_links", page: usize);
forward!(page_images, "page_images", page: usize);
forward!(text_select, "text_select", page: usize, a: [f32; 2], b: [f32; 2], mode: i32);
forward!(text_copy, "text_copy", page: usize, a: [f32; 2], b: [f32; 2]);
forward!(text_range, "text_range", page: usize, p: [f32; 2], mode: i32);
forward!(text_range_copy, "text_range_copy", page: usize, from: i32, to: i32);
forward!(page_text, "page_text", page: usize);
forward!(page_text_stats, "page_text_stats", page: usize);
forward!(annots, "annots", page: usize);
forward!(doc_annots, "doc_annots");

#[tauri::command]
async fn search(remote: State<'_, Arc<Remote>>, doc: DocId, needle: String, opts: Option<Value>, sid: u64, on_event: Channel<Value>) -> Res<()> {
    with(&remote, move |r| r.stream("search", doc, sid, json!({ "needle": needle, "opts": opts }), on_event)).await
}

#[tauri::command]
async fn reflow(remote: State<'_, Arc<Remote>>, doc: DocId, start: usize, sid: u64, on_event: Channel<Value>) -> Res<()> {
    with(&remote, move |r| r.stream("reflow", doc, sid, json!({ "start": start }), on_event)).await
}

#[tauri::command]
async fn cancel(remote: State<'_, Arc<Remote>>, ids: Vec<u64>) -> Res<()> {
    with(&remote, move |r| Ok(r.cancel(ids))).await
}

/// Stops the engine as a crash would, or makes it stop answering (`how`: "hang"), to test that the app
/// carries on (debug builds only).
#[tauri::command]
async fn engine_crash_test(remote: State<'_, Arc<Remote>>, how: Option<String>) -> Res<()> {
    if !cfg!(debug_assertions) {
        return Err("Not in release builds.".into());
    }
    with(&remote, move |r| r.trouble_for_test(how.as_deref().unwrap_or("crash"))).await
}

#[tauri::command]
async fn book_cover(remote: State<'_, Arc<Remote>>, path: String, fp: String) -> Res<Value> {
    if fp.is_empty() || !fp.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("no fingerprint".into());
    }
    readable(&path)?;
    let covers = cache()?.join("covers");
    with(&remote, move |r| r.cover(&path, &covers, &fp)).await
}

#[tauri::command]
async fn save_repaired(remote: State<'_, Arc<Remote>>, doc: DocId, target: String, pdf: bool) -> Res<()> {
    writable(&target)?;
    with(&remote, move |r| r.write_to("save_repaired", doc, &target, json!({ "pdf": pdf }))).await
}

// ---------------------------------------------------------------- paginaut:// protocol

fn respond(responder: UriSchemeResponder, status: StatusCode, kind: &str, body: Vec<u8>) {
    let resp = Response::builder()
        .status(status)
        .header("Content-Type", kind)
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "no-store")
        .body(body)
        .unwrap();
    responder.respond(resp);
}

/// As the desktop's (app::protocol): `/render?d=&p=&s=&r=&e=&c=&id=&q=` gives 8 bytes of size and RGBA pixels
/// (204 if the drawing was cancelled), drawn by the engine's process; `/cover?f=` a shelf cover from the cache.
fn protocol<R: tauri::Runtime>(ctx: UriSchemeContext<'_, R>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let uri = request.uri().to_string();
    let q: std::collections::HashMap<String, String> =
        uri.split_once('?').map(|(_, q)| q).unwrap_or("").split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect();
    if request.uri().path() == "/cover" {
        let f = q.get("f").filter(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_hexdigit()));
        return match f.and_then(|f| std::fs::read(cache().ok()?.join("covers").join(format!("{f}.jpg"))).ok()) {
            Some(b) => respond(responder, StatusCode::OK, "image/jpeg", b),
            None => respond(responder, StatusCode::NOT_FOUND, "application/octet-stream", Vec::new()),
        };
    }
    let num = |k: &str| q.get(k).and_then(|v| v.parse::<f64>().ok());
    let (Some(doc), Some(page), Some(scale)) = (num("d"), num("p"), num("s")) else {
        return respond(responder, StatusCode::BAD_REQUEST, "text/plain", b"bad render request".to_vec());
    };
    let clip = q.get("c").and_then(|c| {
        let v: Vec<i32> = c.split(',').filter_map(|x| x.parse().ok()).collect();
        (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
    });
    let args = json!({
        "page": page as usize,
        "scale": scale as f32,
        "rotate": num("r").unwrap_or(0.0) as i32,
        "clip": clip,
        "extras": num("e").unwrap_or(1.0).clamp(0.0, 2.0) as u8,
        "rid": num("id").unwrap_or(0.0) as u64,
        "prio": num("q").unwrap_or(0.0) as i32,
    });
    let remote = ctx.app_handle().state::<Arc<Remote>>().inner().clone();
    // Waits for the engine: not on WebView's thread.
    std::thread::spawn(move || match remote.render(doc as DocId, args) {
        Ok(Some(px)) => respond(responder, StatusCode::OK, "application/octet-stream", px),
        Ok(None) => respond(responder, StatusCode::NO_CONTENT, "application/octet-stream", Vec::new()),
        Err(e) => respond(responder, StatusCode::INTERNAL_SERVER_ERROR, "text/plain", e.into_bytes()),
    });
}
