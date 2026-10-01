pub mod engine;
mod app;
mod base64;
pub mod addons;
mod update;
mod defaults;
#[cfg(windows)]
mod snap;
mod tts;
mod piper;
mod piper_voices;
pub mod compare;
pub mod convert;
mod docx;
mod flow;
mod fonts;
mod markdown;
mod net;
mod ocr;
pub mod office;
mod ocr_langs;
pub mod officetext;
mod sigstore;
mod tiff;
pub mod xml;
pub mod dsc;

use app::AppState;
use parking_lot::Mutex;
use tauri::Manager;

/// Worker threads: leave two cores for the UI and the OS, cap at 16.
fn worker_count() -> usize {
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    cpus.saturating_sub(2).clamp(2, 16)
}

/// MuPDF's shared cache for decoded images and fonts: 1/128 of RAM, 64–128 MB.
/// Rendered pages are cached by the UI, so this mostly serves re-renders at a new zoom.
fn store_bytes() -> usize {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let total = sys.total_memory() as usize;
    (total / 128).clamp(64 << 20, 128 << 20)
}

/// Tauri turns WebView2 pinch-zoom off together with its zoom shortcuts, and
/// with it off, touchpad pinches never reach the page at all. Turn pinches back
/// on: the page receives them as Ctrl+wheel events and zooms the document
/// itself (it cancels them, so the UI never zooms). Ctrl+wheel/Ctrl+= browser
/// zoom stays off.
#[cfg(windows)]
fn enable_touchpad_pinch(win: &tauri::WebviewWindow) {
    let _ = win.with_webview(|w| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings5;
        use windows_core::Interface;
        let settings = w.controller().CoreWebView2().and_then(|c| c.Settings());
        if let Ok(s) = settings.and_then(|s| s.cast::<ICoreWebView2Settings5>()) {
            let _ = s.SetIsPinchZoomEnabled(true);
        }
    });
}

/// Low asks WebView2 to give memory back: it collects garbage and frees the
/// GPU process's leftovers from pages drawn earlier (several hundred MB after a
/// long scroll). Used while minimized and briefly after a tab closes.
#[tauri::command]
fn memory_level(win: tauri::WebviewWindow, low: bool) {
    #[cfg(windows)]
    let _ = win.with_webview(move |w| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        };
        use windows_core::Interface;
        if let Ok(wv) = w.controller().CoreWebView2().and_then(|c| c.cast::<ICoreWebView2_19>()) {
            let level = if low { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
            let _ = wv.SetMemoryUsageTargetLevel(level);
        }
    });
    #[cfg(not(windows))]
    let _ = (win, low);
}

/// The app was called Folio (data under `app.folio.reader`) up to 0.4.0. On the first
/// start under the new name, copy over what the user kept: saved signatures (encrypted
/// for the Windows account, so they move as they are) and the page's local storage
/// (settings, recent files, reading positions, layout fixes; the page renames its keys).
/// Copied rather than moved, so an old install keeps working until it's uninstalled.
/// Runs before WebView2 starts, so its storage isn't in use yet.
#[cfg(windows)]
fn migrate_from_folio() {
    use std::{fs, io, path::Path, path::PathBuf};
    fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
        fs::create_dir_all(to)?;
        for e in fs::read_dir(from)? {
            let e = e?;
            let dest = to.join(e.file_name());
            if e.file_type()?.is_dir() { copy_dir(&e.path(), &dest)? } else { fs::copy(e.path(), dest).map(|_| ())? }
        }
        Ok(())
    }
    const OLD: &str = "app.folio.reader";
    const NEW: &str = "app.paginaut.reader";
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if let Some(roaming) = env("APPDATA") {
        let (from, to) = (roaming.join(OLD).join("signatures.dat"), roaming.join(NEW).join("signatures.dat"));
        if from.is_file() && !to.exists() && fs::create_dir_all(roaming.join(NEW)).is_ok() {
            let _ = fs::copy(&from, &to);
        }
    }
    if let Some(local) = env("LOCALAPPDATA") {
        let storage = Path::new("EBWebView").join("Default").join("Local Storage");
        let (from, to) = (local.join(OLD).join(&storage), local.join(NEW).join(&storage));
        if from.is_dir() && !to.exists() && copy_dir(&from, &to).is_err() {
            // A half copy would hide the old settings for good; start clean instead.
            let _ = fs::remove_dir_all(&to);
        }
    }
}

/// The installer's switches: `--register-types` after installing, `--unregister-types` before uninstalling
/// (Paginaut offered for its file types in Windows, or withdrawn). Handled before anything else starts, so they
/// never reach a running Paginaut. Returns the exit code, or None to start as usual.
pub fn installer_switch() -> Option<i32> {
    let arg = std::env::args().nth(1)?;
    #[cfg(windows)]
    {
        let exe = std::env::current_exe().ok()?;
        match arg.as_str() {
            "--register-types" => return Some(defaults::register_for_install(&defaults::PAGINAUT, &exe).map_or(1, |_| 0)),
            "--unregister-types" => {
                defaults::unregister_for_uninstall(&defaults::PAGINAUT, &exe);
                return Some(0);
            }
            _ => {}
        }
    }
    let _ = arg;
    None
}

pub fn run() {
    #[cfg(windows)]
    migrate_from_folio();
    let engine = engine::Engine::new(worker_count(), store_bytes()).expect("MuPDF failed to start");
    let converter = engine.clone();
    let args: Vec<String> = std::env::args().collect();
    let launch = app::files_from_args(&args, None);

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            let files = app::files_from_args(&argv, Some(&cwd));
            app::forward_files(app, files);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState { engine, launch_files: Mutex::new(launch), started: std::time::Instant::now() })
        .register_asynchronous_uri_scheme_protocol("paginaut", app::protocol)
        .invoke_handler(tauri::generate_handler![
            app::open_document,
            app::unlock_document,
            app::close_document,
            app::trim_document,
            app::save_document,
            app::insert_pages,
            app::insert_blank,
            app::peek_file,
            app::merge_files,
            app::insert_from_open,
            app::save_parts,
            app::split_by_size,
            app::existing_files,
            app::page_size,
            app::content_box,
            app::page_sizes_of,
            app::stamp_pages,
            app::remove_stamps,
            app::first_page_size,
            app::set_outline,
            app::page_label_ranges,
            app::set_page_labels,
            app::properties,
            app::set_properties,
            app::save_with,
            app::security,
            app::check_owner,
            app::pictures,
            app::picture_bytes,
            app::picture_save,
            app::save_pictures,
            app::area_bytes,
            app::area_save,
            app::area_text,
            app::attachments,
            app::redact_mark,
            app::redaction_pages,
            app::redact_apply,
            app::redact_clear,
            app::sanitize,
            app::attachment_add,
            app::attachment_remove,
            app::tables,
            app::paragraphs,
            app::picture_edit,
            app::edit_text,
            app::table_in,
            app::table_save,
            app::attachment_save,
            app::attachment_open,
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
            app::forms_init,
            app::widgets,
            app::widget_set,
            app::widget_toggle,
            app::doc_undo,
            app::form_reset,
            app::save_flattened,
            app::annots,
            app::doc_annots,
            app::annot_edit,
            app::user_name,
            app::sig_list,
            app::read_picture,
            app::certs,
            app::sign_document,
            app::signatures,
            app::signed_version,
            app::sig_add,
            app::sig_delete,
            app::ocr_languages,
            app::ocr_download,
            app::ocr_download_stop,
            app::ocr_delete,
            app::ocr_check,
            app::ocr_run,
            app::ocr_remove,
            app::ocr_layers,
            app::office_engines,
            app::office_set_preference,
            app::office_install,
            app::office_install_stop,
            app::office_remove,
            app::office_convert,
            app::convert_options,
            app::convert_from_pdf,
            app::convert_stop,
            app::compare_run,
            app::compare_stop,
            app::fonts_installed,
            app::fonts_user,
            app::font_add,
            app::font_bytes,
            app::font_remove,
            app::skin_set,
            app::skin_bytes,
            app::skin_clear,
            app::default_apps_status,
            app::default_apps_set,
            app::default_apps_open,
            app::tts_voices,
            app::snap_button,
            app::tts_speak,
            app::piper_voices,
            app::piper_download,
            app::piper_download_stop,
            app::piper_delete,
            app::piper_sample,
            app::pfx_summary,
            app::signature_revocation,
            app::update_check,
            app::update_install,
            app::update_stop,
            app::open_path,
            app::markdown_read,
            app::markdown_preview,
            app::markdown_save,
            app::cancel,
            app::system_info,
            app::take_launch_files,
            memory_level,
        ])
        .setup(move |app| {
            // OCR models: English, Hindi and Marathi ship with the app; more are
            // downloaded into the local (not roaming) data folder.
            let builtin = app.path().resource_dir().map(|d| d.join("tessdata")).unwrap_or_default();
            let user = app.path().app_local_data_dir().map(|d| d.join("tessdata")).unwrap_or_default();
            app.manage(ocr::ModelDirs { builtin, user });
            // Piper voices (read aloud), downloaded on request.
            app.manage(piper::Voices { dir: app.path().app_local_data_dir().map(|d| d.join("voices")).unwrap_or_default() });
            // The signed add-on list kept from the last check (which OCR languages, which LibreOffice).
            if let Ok(dir) = app.path().app_local_data_dir() {
                addons::load_cached(&dir);
            }
            // Office files open through PDFs made by Microsoft Office or LibreOffice (see office/).
            let local = app.path().app_local_data_dir().unwrap_or_default();
            let office = std::sync::Arc::new(office::Office::new(&local));
            let o = office.clone();
            converter.set_converter(std::sync::Arc::new(move |path: &str, engine: Option<&str>| o.convert(path, engine)));
            app.manage(office);
            // Shown by the UI once the first frame is painted, so there's no white flash.
            #[cfg(windows)]
            if let Some(win) = app.get_webview_window("main") {
                enable_touchpad_pinch(&win);
                if let Ok(hwnd) = win.hwnd() {
                    if let Err(e) = snap::install(app.handle(), hwnd.0 as isize) {
                        eprintln!("snap layouts: {e}");
                    }
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Paginaut");
}
