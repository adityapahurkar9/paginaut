//! The Android viewer: the same engine and `paginaut://` pixel protocol as the desktop app,
//! with only the commands needed to read documents. Android's own services (the file picker,
//! "Open with", Share, printing) are in the Kotlin plugin, PaginautPlugin.kt; the page reaches
//! it through the commands here.

use crate::app::{self, blocking, AppState, Res};
use crate::engine::DocId;
use parking_lot::Mutex;
use serde_json::{json, Value};
use tauri::plugin::{Builder as PluginBuilder, PluginHandle, TauriPlugin};
use tauri::{Manager, Runtime, State, Wry};

pub fn run() {
    let engine = crate::engine::Engine::new(crate::worker_count(), crate::store_bytes()).expect("MuPDF failed to start");
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(native())
        .manage(AppState { engine, launch_files: Mutex::new(Vec::new()), started: std::time::Instant::now() })
        .register_asynchronous_uri_scheme_protocol("paginaut", app::protocol)
        .invoke_handler(tauri::generate_handler![
            app::open_document,
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
            speak,
            stop_speaking,
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

/// A document copied into the cache from where Android gave it: `{ path, name, uri }`,
/// or `{}` when nothing was picked or sent.
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
async fn share(native: State<'_, Native<Wry>>, path: String) -> Res<()> {
    call(&native, "share", json!({ "path": path })).await.map(|_| ())
}

/// Android's print service takes PDFs: other documents (e-books, XPS, comics) are written
/// as a PDF next to their copy first.
#[tauri::command]
async fn print(native: State<'_, Native<Wry>>, state: State<'_, AppState>, doc: DocId, path: String, title: String, pdf: bool) -> Res<()> {
    let file = if pdf {
        path
    } else {
        let out = std::path::Path::new(&path).with_extension("print.pdf").to_string_lossy().into_owned();
        let engine = state.engine.clone();
        let target = out.clone();
        blocking(move || engine.export_pdf(doc, &target)).await?;
        out
    };
    call(&native, "print", json!({ "path": file, "title": title })).await.map(|_| ())
}

/// The status and navigation bars: colour behind them, dark icons or light, hidden while reading.
#[tauri::command]
async fn set_bars(native: State<'_, Native<Wry>>, color: String, dark: bool, immersive: bool) -> Res<()> {
    call(&native, "setBars", json!({ "color": color, "dark": dark, "immersive": immersive })).await.map(|_| ())
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
