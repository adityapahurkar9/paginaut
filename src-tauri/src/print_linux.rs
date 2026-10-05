//! Printing on Linux: GTK's print dialog (the printers CUPS knows, pages, copies, paper, the printer's own
//! options), then the chosen pages as they're shown (in the order and turned as arranged, with comments and
//! form entries) written as a PDF and handed to the printer, which takes PDF itself. Text stays sharp, and
//! nothing is drawn on the window's thread. GTK's objects stay on that thread (the dialog's choice waits in
//! `CHOSEN` while the PDF is written elsewhere).
//!
//! The dialog and print jobs are GTK's "unix print" part, which gtk-rs doesn't cover: the few calls are
//! declared here (they're in libgtk-3 itself).

use crate::engine::{DocId, Engine};
use gtk::glib;
use gtk::glib::translate::{from_glib_full, from_glib_none, ToGlibPtr};
use gtk::prelude::*;
use serde::Deserialize;
use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CString};

#[allow(non_camel_case_types)]
type gpointer = *mut c_void;

extern "C" {
    fn gtk_print_unix_dialog_new(title: *const c_char, parent: *mut gtk::ffi::GtkWindow) -> *mut gtk::ffi::GtkWidget;
    fn gtk_print_unix_dialog_set_current_page(dialog: *mut gtk::ffi::GtkWidget, page: c_int);
    fn gtk_print_unix_dialog_set_embed_page_setup(dialog: *mut gtk::ffi::GtkWidget, embed: glib::ffi::gboolean);
    fn gtk_print_unix_dialog_get_selected_printer(dialog: *mut gtk::ffi::GtkWidget) -> gpointer;
    fn gtk_print_unix_dialog_get_settings(dialog: *mut gtk::ffi::GtkWidget) -> *mut gtk::ffi::GtkPrintSettings;
    fn gtk_print_unix_dialog_get_page_setup(dialog: *mut gtk::ffi::GtkWidget) -> *mut gtk::ffi::GtkPageSetup;
    fn gtk_print_job_new(title: *const c_char, printer: gpointer, settings: *mut gtk::ffi::GtkPrintSettings, setup: *mut gtk::ffi::GtkPageSetup) -> gpointer;
    fn gtk_print_job_set_source_file(job: gpointer, filename: *const c_char, error: *mut *mut glib::ffi::GError) -> glib::ffi::gboolean;
    fn gtk_print_job_send(
        job: gpointer,
        callback: unsafe extern "C" fn(job: gpointer, data: gpointer, error: *const glib::ffi::GError),
        data: gpointer,
        destroy: Option<unsafe extern "C" fn(data: gpointer)>,
    );
}

/// A page as shown: its index in the file and the extra turn it's shown with.
#[derive(Deserialize, Clone, Copy)]
pub struct Shown {
    pub page: usize,
    pub rot: i32,
}

thread_local! {
    /// The printer (a GtkPrinter), its settings and paper, from the dialog until the job is sent.
    static CHOSEN: RefCell<Option<(glib::Object, gtk::PrintSettings, gtk::PageSetup)>> = const { RefCell::new(None) };
}

/// Shows GTK's print dialog over `window` (on the window's thread). The pages chosen (indices into the pages
/// shown), or None when cancelled.
pub fn ask(window: &tauri::WebviewWindow, count: usize, current: usize) -> Option<Vec<usize>> {
    let parent = window.gtk_window().ok();
    let title = CString::new("Print").unwrap();
    // SAFETY: GTK calls on the main thread; the dialog is a new floating widget wrapped (and sunk) once, the
    // returned printer, settings and setup are borrowed and wrapped with a reference of our own.
    unsafe {
        let raw = gtk_print_unix_dialog_new(title.as_ptr(), parent.as_ref().map_or(std::ptr::null_mut(), |w| w.upcast_ref::<gtk::Window>().to_glib_none().0));
        let dialog: gtk::Dialog = from_glib_none(raw as *mut gtk::ffi::GtkDialog);
        gtk_print_unix_dialog_set_current_page(raw, current as c_int);
        gtk_print_unix_dialog_set_embed_page_setup(raw, glib::ffi::GTRUE);
        dialog.set_modal(true);
        let ok = dialog.run() == gtk::ResponseType::Ok;
        let printer = gtk_print_unix_dialog_get_selected_printer(raw);
        let chosen = (ok && !printer.is_null()).then(|| {
            let printer: glib::Object = from_glib_none(printer as *mut glib::gobject_ffi::GObject);
            let settings: gtk::PrintSettings = from_glib_full(gtk_print_unix_dialog_get_settings(raw));
            let setup: gtk::PageSetup = from_glib_none(gtk_print_unix_dialog_get_page_setup(raw));
            (printer, settings, setup)
        });
        dialog.destroy();
        let (printer, settings, setup) = chosen?;
        let last = count.saturating_sub(1);
        let pages: Vec<usize> = match settings.print_pages() {
            gtk::PrintPages::Current => vec![current.min(last)],
            gtk::PrintPages::Ranges => settings
                .page_ranges()
                .iter()
                .flat_map(|r| (r.start().max(0) as usize)..=(r.end().max(r.start()).max(0) as usize))
                .filter(|&i| i < count)
                .collect(),
            _ => (0..count).collect(),
        };
        // The PDF holds just these pages: the printer prints all of it, fitted to the paper.
        settings.set_print_pages(gtk::PrintPages::All);
        settings.set("cups-fit-to-page", Some("true"));
        CHOSEN.with(|c| *c.borrow_mut() = Some((printer, settings, setup)));
        Some(pages)
    }
}

/// The chosen pages (`chosen`: indices into `shown`) as a PDF at `file`.
pub fn write(engine: &Engine, doc: DocId, shown: &[Shown], chosen: &[usize], file: &str) -> Result<(), String> {
    let pages: Vec<Shown> = chosen.iter().filter_map(|&i| shown.get(i).copied()).collect();
    let mut rot = vec![0; pages.iter().map(|s| s.page + 1).max().unwrap_or(0)];
    for s in &pages {
        rot[s.page] = s.rot;
    }
    let order: Vec<usize> = pages.iter().map(|s| s.page).collect();
    engine.write_document(doc, &order, &rot, file, "pdf", "")
}

type Done = Box<dyn FnOnce(Result<(), String>)>;

unsafe extern "C" fn sent(_: gpointer, data: gpointer, error: *const glib::ffi::GError) {
    // SAFETY: `data` is the Box made in `send`, given back once.
    let (file, done) = *Box::from_raw(data as *mut (String, Done));
    let _ = std::fs::remove_file(&file);
    done(if error.is_null() { Ok(()) } else { Err(format!("Couldn't print: {}", std::ffi::CStr::from_ptr((*error).message).to_string_lossy())) });
}

/// Sends `file` to the printer the dialog chose (on the window's thread); `done` when CUPS has it. The file
/// is removed then.
pub fn send(name: &str, file: String, done: impl FnOnce(Result<(), String>) + 'static) {
    let Some((printer, settings, setup)) = CHOSEN.with(|c| c.borrow_mut().take()) else {
        let _ = std::fs::remove_file(&file);
        return done(Err("No printer was chosen.".into()));
    };
    let (title, path) = (CString::new(name.replace('\0', "")).unwrap_or_default(), CString::new(file.as_str()).unwrap_or_default());
    // SAFETY: GTK calls on the main thread with live objects; the job is owned here until sent (GTK keeps its
    // own reference while sending); the callback's data is a Box given back exactly once in `sent`.
    unsafe {
        let job = gtk_print_job_new(title.as_ptr(), printer.as_ptr() as gpointer, settings.to_glib_none().0, setup.to_glib_none().0);
        let job_obj: glib::Object = from_glib_full(job as *mut glib::gobject_ffi::GObject);
        let mut err: *mut glib::ffi::GError = std::ptr::null_mut();
        if gtk_print_job_set_source_file(job, path.as_ptr(), &mut err) == glib::ffi::GFALSE {
            let msg = if err.is_null() { String::new() } else { std::ffi::CStr::from_ptr((*err).message).to_string_lossy().into_owned() };
            if !err.is_null() {
                glib::ffi::g_error_free(err);
            }
            let _ = std::fs::remove_file(&file);
            return done(Err(format!("The printer couldn't take the pages: {msg}")));
        }
        let data: Box<(String, Done)> = Box::new((file, Box::new(done)));
        gtk_print_job_send(job, sent, Box::into_raw(data) as gpointer, None);
        drop(job_obj);
    }
}
