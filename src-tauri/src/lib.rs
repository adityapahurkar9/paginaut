pub mod engine;
mod app;
#[cfg(mobile)]
mod mobile;
#[cfg(target_os = "android")]
mod engine_link;

// The desktop app: editing, OCR, Office files, conversion, signing and Windows' own services.
// The Android viewer is built from the engine and app.rs alone (see mobile.rs).
#[cfg(desktop)]
mod desk;
#[cfg(desktop)]
mod base64;
#[cfg(desktop)]
pub mod addons;
#[cfg(desktop)]
mod update;
#[cfg(desktop)]
mod appwindows;
#[cfg(desktop)]
mod defaults;
#[cfg(windows)]
mod snap;
#[cfg(windows)]
mod dragghost;
#[cfg(windows)]
mod print;
#[cfg(target_os = "linux")]
mod print_linux;
#[cfg(target_os = "linux")]
mod pkcs11;
#[cfg(desktop)]
mod tts;
#[cfg(desktop)]
mod piper;
#[cfg(desktop)]
mod piper_voices;
#[cfg(desktop)]
pub mod compare;
#[cfg(desktop)]
pub mod convert;
#[cfg(desktop)]
mod docx;
#[cfg(desktop)]
mod flow;
#[cfg(desktop)]
mod fonts;
#[cfg(desktop)]
mod markdown;
#[cfg(desktop)]
mod net;
#[cfg(desktop)]
mod ocr;
#[cfg(desktop)]
pub mod office;
#[cfg(desktop)]
mod ocr_langs;
#[cfg(desktop)]
pub mod officetext;
#[cfg(desktop)]
mod sigstore;
#[cfg(desktop)]
mod tiff;
#[cfg(desktop)]
pub mod xml;
#[cfg(desktop)]
pub mod dsc;
#[cfg(desktop)]
mod shelf;
#[cfg(desktop)]
mod prefs;

#[cfg(desktop)]
use app::AppState;
#[cfg(desktop)]
use tauri::Manager;

/// Worker threads: leave two cores for the UI and the OS, cap at 16 (4 on phones, whose
/// many cores are mostly small ones, and each worker holds its own page in memory).
pub(crate) fn worker_count() -> usize {
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let cap = if cfg!(mobile) { 4 } else { 16 };
    cpus.saturating_sub(2).clamp(2, cap)
}

/// MuPDF's shared cache for decoded images and fonts: 1/128 of RAM, 64–128 MB (on phones
/// 1/96, 24–64 MB). Rendered pages are cached by the UI, so this mostly serves re-renders
/// at a new zoom.
pub(crate) fn store_bytes() -> usize {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let total = sys.total_memory() as usize;
    if cfg!(mobile) {
        (total / 96).clamp(24 << 20, 64 << 20)
    } else {
        (total / 128).clamp(64 << 20, 128 << 20)
    }
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
#[cfg(desktop)]
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
#[cfg(desktop)]
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Linux: WebKitGTK's DMA-BUF renderer shows a blank window (or none) in virtual machines, WSL and with
    // some NVIDIA drivers; the older path works everywhere. Someone who sets it themselves decides.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    #[cfg(target_os = "linux")]
    private_temp();
    #[cfg(desktop)]
    run_desktop();
    #[cfg(mobile)]
    mobile::run();
}

/// Linux: temporary files go in a folder only this user can open (~/.cache/paginaut/tmp), not the /tmp every
/// account shares, where the names Paginaut uses (print copies, rebuilt books, the update being installed)
/// could be made in advance by someone else. Windows' temp folder is the user's own already. Set here, before
/// any thread starts, so every `std::env::temp_dir()` (and the programs Paginaut starts) uses it; a TMPDIR
/// someone chose themselves is kept.
#[cfg(target_os = "linux")]
fn private_temp() {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::path::PathBuf;
    let chosen = std::env::var_os("TMPDIR").filter(|t| !t.is_empty() && !["/tmp", "/tmp/", "/var/tmp", "/var/tmp/"].contains(&t.to_string_lossy().as_ref()));
    if chosen.is_some() {
        return;
    }
    let cache = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")));
    let Some(dir) = cache.map(|c| c.join("paginaut").join("tmp")) else { return };
    if std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir).is_err() {
        return;
    }
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    std::env::set_var("TMPDIR", &dir);
}

/// The temporary files the app makes (copies for Office conversion and printing, attachments and
/// signed revisions opened in other apps), removed when it quits and again when it starts. One still
/// open in another app can't be removed then (Windows keeps it while it's in use): the next start
/// takes it. (Unpacked RAR comics go by themselves: native/cbr.c opens them delete-on-close.)
#[cfg(desktop)]
fn clear_temp() {
    let tmp = std::env::temp_dir();
    let _ = std::fs::remove_dir_all(tmp.join("Paginaut"));
    let _ = std::fs::remove_dir_all(tmp.join("paginaut-libreoffice"));
    if let Ok(list) = std::fs::read_dir(&tmp) {
        for e in list.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            // Copies of documents (engine::temp_path), for Office (office/com.rs), RAR leftovers of older versions.
            if name.starts_with("paginaut-") && (name.ends_with(".pdf") || name.starts_with("paginaut-office-") || name.ends_with(".tmp")) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

#[cfg(desktop)]
fn run_desktop() {
    #[cfg(windows)]
    migrate_from_folio();
    let engine = engine::Engine::new(worker_count(), store_bytes()).expect("MuPDF failed to start");
    let converter = engine.clone();
    let args: Vec<String> = std::env::args().collect();
    let launch = app::files_from_args(&args, None);

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            let files = app::files_from_args(&argv, Some(&cwd));
            appwindows::forward_files(app, files);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState { engine, started: std::time::Instant::now() })
        .manage(appwindows::Windows::new())
        .on_window_event(appwindows::on_window_event)
        .register_asynchronous_uri_scheme_protocol("paginaut", app::protocol)
        .invoke_handler(tauri::generate_handler![
            app::open_document,
            app::unlock_document,
            app::close_document,
            app::trim_document,
            desk::save_document,
            desk::insert_pages,
            desk::insert_blank,
            desk::peek_file,
            desk::merge_files,
            desk::insert_from_open,
            desk::save_parts,
            desk::split_by_size,
            desk::existing_files,
            desk::page_size,
            desk::content_box,
            desk::page_sizes_of,
            desk::stamp_pages,
            desk::remove_stamps,
            desk::first_page_size,
            desk::set_outline,
            desk::page_label_ranges,
            desk::set_page_labels,
            app::properties,
            desk::set_properties,
            desk::save_with,
            app::security,
            desk::check_owner,
            desk::pictures,
            desk::picture_bytes,
            desk::picture_save,
            desk::save_pictures,
            desk::area_bytes,
            desk::area_save,
            desk::area_text,
            desk::attachments,
            desk::redact_mark,
            desk::redaction_pages,
            desk::redact_apply,
            desk::redact_clear,
            desk::sanitize,
            desk::attachment_add,
            desk::attachment_remove,
            desk::tables,
            desk::paragraphs,
            desk::picture_edit,
            desk::edit_text,
            desk::table_in,
            desk::table_save,
            desk::attachment_save,
            desk::attachment_open,
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
            desk::forms_init,
            desk::widgets,
            desk::widget_set,
            desk::widget_toggle,
            desk::doc_undo,
            desk::form_reset,
            desk::save_flattened,
            app::annots,
            app::doc_annots,
            desk::annot_edit,
            app::user_name,
            desk::sig_list,
            desk::read_picture,
            desk::certs,
            desk::sign_document,
            desk::signatures,
            desk::signed_version,
            desk::sig_add,
            desk::sig_delete,
            prefs::storage_write,
            desk::ocr_languages,
            desk::ocr_download,
            desk::ocr_download_stop,
            desk::ocr_delete,
            desk::ocr_check,
            desk::ocr_run,
            desk::ocr_remove,
            desk::ocr_layers,
            desk::office_engines,
            desk::office_set_preference,
            desk::office_install,
            desk::office_install_stop,
            desk::office_remove,
            desk::office_convert,
            desk::convert_options,
            desk::convert_from_pdf,
            desk::convert_stop,
            desk::compare_run,
            desk::compare_stop,
            desk::fonts_installed,
            desk::fonts_user,
            desk::font_add,
            desk::font_bytes,
            desk::font_remove,
            desk::skin_set,
            desk::skin_bytes,
            desk::skin_clear,
            desk::default_apps_status,
            desk::default_apps_set,
            desk::default_apps_open,
            desk::tts_voices,
            desk::snap_button,
            desk::tts_speak,
            desk::piper_voices,
            desk::piper_download,
            desk::piper_download_stop,
            desk::piper_delete,
            desk::piper_sample,
            desk::print_doc,
            desk::print_stop,
            desk::pfx_summary,
            desk::signature_revocation,
            desk::update_check,
            desk::update_install,
            desk::update_stop,
            desk::open_path,
            desk::pkcs11_module,
            app::book_cover,
            app::save_repaired,
            shelf::scan_books,
            desk::markdown_read,
            desk::markdown_preview,
            desk::markdown_save,
            app::cancel,
            desk::system_info,
            appwindows::window_start,
            appwindows::window_new,
            appwindows::window_list,
            appwindows::tab_send,
            appwindows::window_at,
            appwindows::window_report,
            appwindows::quit_all,
            appwindows::window_with,
            appwindows::tab_focus,
            appwindows::drag_ghost,
            appwindows::drag_ghost_hide,
            appwindows::tab_hover,
            memory_level,
        ])
        .setup(move |app| {
            // First, as WebView2 takes most of the start-up. Shown by the UI once the first frame is painted, so
            // there's no white flash.
            appwindows::open_main(app.handle(), &launch)?;
            // Only the one instance gets here (a second one hands its files over, or asks for a window, and quits first):
            // what earlier runs left in the temp folder can go.
            clear_temp();
            engine::mupdf::set_temp_dir(&std::env::temp_dir());
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
            office.await_preference();
            let o = office.clone();
            converter.set_converter(std::sync::Arc::new(move |path: &str, engine: Option<&str>| o.convert(path, engine)));
            app.manage(office);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while running Paginaut")
        .run(|_, event| {
            if let tauri::RunEvent::Exit = event {
                clear_temp();
            }
        });
}
