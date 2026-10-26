//! Desktop-only commands: editing, saving, signing, OCR, Office, conversion, compare, Windows tools.

use crate::engine::{DocId, DocInfo, InsertSource, Inserted, Part, Peek};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{Manager, State};

use crate::app::{blocking, AppState, Res};

// ---------------------------------------------------------------- documents

#[tauri::command]
pub async fn save_document(
    state: State<'_, AppState>,
    doc: DocId,
    order: Vec<usize>,
    rot: Vec<i32>,
    target: Option<String>,
) -> Res<DocInfo> {
    let engine = state.engine.clone();
    blocking(move || engine.save(doc, order, rot, target)).await
}

/// Saves and protects, compresses or rewrites (repairs) the PDF.
#[tauri::command]
pub async fn save_with(
    state: State<'_, AppState>,
    doc: DocId,
    order: Vec<usize>,
    rot: Vec<i32>,
    target: Option<String>,
    opts: crate::engine::SaveOptions,
) -> Res<DocInfo> {
    let engine = state.engine.clone();
    blocking(move || engine.save_with(doc, order, rot, target, opts)).await
}

/// Checks the permissions password (kept when right, so the protection can be changed).
#[tauri::command]
pub async fn check_owner(state: State<'_, AppState>, doc: DocId, password: String) -> Res<bool> {
    let engine = state.engine.clone();
    blocking(move || engine.check_owner(doc, &password)).await
}

/// Adds pages of other files at the end of the document (one undo step).
#[tauri::command]
pub async fn insert_pages(state: State<'_, AppState>, doc: DocId, sources: Vec<InsertSource>, fit: Option<[f32; 2]>) -> Res<Inserted> {
    let engine = state.engine.clone();
    blocking(move || engine.insert_pages(doc, sources, fit)).await
}

/// Combines files into a new PDF at `target` (see Engine::merge). Returns its page count.
#[tauri::command]
pub async fn merge_files(state: State<'_, AppState>, sources: Vec<InsertSource>, fit: Option<[f32; 2]>, bookmarks: bool, target: String) -> Res<usize> {
    let engine = state.engine.clone();
    blocking(move || engine.merge(sources, fit, bookmarks, &target)).await
}

/// Replaces the bookmarks (one undo step).
#[tauri::command]
pub async fn set_outline(state: State<'_, AppState>, doc: DocId, items: Vec<crate::engine::mupdf::OutlineNode>) -> Res<()> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.set_outline(&items)).await
}

#[tauri::command]
pub async fn page_label_ranges(state: State<'_, AppState>, doc: DocId) -> Res<Vec<crate::engine::mupdf::LabelRange>> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.page_label_ranges()).await
}

/// Replaces the page labels (one undo step).
#[tauri::command]
pub async fn set_page_labels(state: State<'_, AppState>, doc: DocId, ranges: Vec<crate::engine::mupdf::LabelRange>) -> Res<()> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.set_page_labels(&ranges)).await
}

/// Sets title, author, subject and keywords (one undo step).
#[tauri::command]
pub async fn set_properties(state: State<'_, AppState>, doc: DocId, props: crate::engine::Properties) -> Res<()> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.set_properties(&props)).await
}

/// Stamps pages (page numbers, headers, watermarks) as one undo step.
#[tauri::command]
pub async fn stamp_pages(state: State<'_, AppState>, doc: DocId, pages: Vec<crate::engine::StampPage>, kind: String, behind: bool, replace: bool) -> Res<usize> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.stamp_pages(&pages, &kind, behind, replace)).await
}

/// Takes stamps of one kind off the pages (all, or those listed). Returns the pages that had some.
#[tauri::command]
pub async fn remove_stamps(state: State<'_, AppState>, doc: DocId, kind: String, pages: Option<Vec<usize>>) -> Res<usize> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.remove_stamps(&kind, pages)).await
}

/// Crops and/or resizes pages (one undo step). Returns their new sizes.
#[tauri::command]
pub async fn page_size(state: State<'_, AppState>, doc: DocId, ops: Vec<crate::engine::PageSizeOp>, opts: crate::engine::mupdf::PageSizeOptions) -> Res<Vec<[f32; 2]>> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.page_size(&ops, &opts)).await
}

/// The box around everything drawn on a page (relative to its top-left), None if blank.
#[tauri::command]
pub async fn content_box(state: State<'_, AppState>, doc: DocId, page: usize) -> Res<Option<[f32; 4]>> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.content_box(page)).await
}

/// Sizes of some pages (after they changed).
#[tauri::command]
pub async fn page_sizes_of(state: State<'_, AppState>, doc: DocId, pages: Vec<usize>) -> Res<Vec<[f32; 2]>> {
    let engine = state.engine.clone();
    blocking(move || {
        let d = engine.doc(doc)?;
        pages.iter().map(|&p| d.bounds(p).map(|b| [b[2] - b[0], b[3] - b[1]])).collect()
    })
    .await
}

/// Copies pages of another open document to the end of this one (one undo step).
#[tauri::command]
pub async fn insert_from_open(state: State<'_, AppState>, doc: DocId, from: DocId, pages: Vec<usize>) -> Res<Inserted> {
    let engine = state.engine.clone();
    blocking(move || engine.insert_from_open(doc, from, pages)).await
}

/// Writes pages to new PDFs (extract, split). Returns each file's size.
#[tauri::command]
pub async fn save_parts(state: State<'_, AppState>, doc: DocId, parts: Vec<Part>, rot: Vec<i32>) -> Res<Vec<u64>> {
    let engine = state.engine.clone();
    blocking(move || engine.save_parts(doc, parts, rot)).await
}

/// Groups pages into parts of about `max_bytes` each.
#[tauri::command]
pub async fn split_by_size(state: State<'_, AppState>, doc: DocId, order: Vec<usize>, max_bytes: f64) -> Res<Vec<Vec<usize>>> {
    let engine = state.engine.clone();
    blocking(move || engine.split_by_size(doc, order, max_bytes)).await
}

/// Size of a file's first page (a picture's own size), in points.
#[tauri::command]
pub async fn first_page_size(state: State<'_, AppState>, path: String) -> Res<[f32; 2]> {
    let engine = state.engine.clone();
    blocking(move || engine.first_page_size(&path)).await
}

/// Which of these files exist already.
#[tauri::command]
pub fn existing_files(paths: Vec<String>) -> Vec<String> {
    paths.into_iter().filter(|p| std::path::Path::new(p).exists()).collect()
}

/// Page count and kind of a file (before inserting its pages).
#[tauri::command]
pub async fn peek_file(state: State<'_, AppState>, path: String, password: Option<String>) -> Res<Peek> {
    let engine = state.engine.clone();
    blocking(move || engine.peek(&path, password.as_deref())).await
}

/// Adds empty pages at the end of the document (one undo step).
#[tauri::command]
pub async fn insert_blank(state: State<'_, AppState>, doc: DocId, w: f32, h: f32, count: usize) -> Res<Inserted> {
    let engine = state.engine.clone();
    blocking(move || engine.insert_blank(doc, w, h, count)).await
}

// ---------------------------------------------------------------- forms

#[tauri::command]
pub async fn forms_init(state: State<'_, AppState>, doc: DocId) -> Res<crate::engine::FormInfo> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.forms_init()).await
}

/// Form fields on a page, as JSON text (see fo_widgets_json).
#[tauri::command]
pub async fn widgets(state: State<'_, AppState>, doc: DocId, page: usize) -> Res<String> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.widgets_json(page)).await
}

#[tauri::command]
pub async fn widget_set(state: State<'_, AppState>, doc: DocId, page: usize, num: i32, value: String) -> Res<crate::engine::FormChange> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.widget_set(page, num, &value)).await
}

#[tauri::command]
pub async fn widget_toggle(state: State<'_, AppState>, doc: DocId, page: usize, num: i32) -> Res<crate::engine::FormChange> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.widget_toggle(page, num)).await
}

/// Undo/redo one form or annotation edit.
#[tauri::command]
pub async fn doc_undo(state: State<'_, AppState>, doc: DocId, redo: bool) -> Res<crate::engine::FormChange> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.undo(redo)).await
}

#[tauri::command]
pub async fn form_reset(state: State<'_, AppState>, doc: DocId) -> Res<crate::engine::FormChange> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.reset_form()).await
}

// ---------------------------------------------------------------- saved signatures

fn sig_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    app.path().app_data_dir().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- pictures, areas, attachments

#[tauri::command]
pub async fn pictures(state: State<'_, AppState>, doc: DocId, page: usize, own: Option<bool>) -> Res<Vec<crate::engine::Picture>> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.pictures(page, own.unwrap_or(false))).await
}

/// Deletes (0), moves or resizes (1), replaces (2) or adds (3) a picture on a page (one undo step);
/// for 2 and 3 the picture comes from `path` (a picture file, up to 100 MB).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn picture_edit(state: State<'_, AppState>, doc: DocId, page: usize, target: [f32; 4], action: i32, dest: [f32; 4], path: Option<String>) -> Res<()> {
    let engine = state.engine.clone();
    blocking(move || {
        let data = match (action, path) {
            (2 | 3, Some(p)) => {
                let ext = std::path::Path::new(&p).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                if !["png", "jpg", "jpeg", "jfif", "bmp", "gif", "tif", "tiff", "webp", "jp2", "jpx"].contains(&ext.as_str()) {
                    return Err("That isn't a picture.".into());
                }
                if std::fs::metadata(&p).map_err(|e| e.to_string())?.len() > 100 << 20 {
                    return Err("That picture is too large (over 100 MB).".into());
                }
                std::fs::read(&p).map_err(|e| e.to_string())?
            }
            (2 | 3, None) => return Err("No picture chosen.".into()),
            _ => Vec::new(),
        };
        engine.doc(doc)?.picture_edit(page, target, action, dest, &data)
    })
    .await
}

/// A picture's bytes (for the clipboard).
#[tauri::command]
pub async fn picture_bytes(state: State<'_, AppState>, doc: DocId, page: usize, idx: usize, fmt: i32) -> Res<tauri::ipc::Response> {
    let engine = state.engine.clone();
    blocking(move || Ok(tauri::ipc::Response::new(engine.doc(doc)?.picture_data(page, idx, fmt)?.0))).await
}

/// Saves a picture at `path` (its extension follows how the picture is saved); returns the path used.
#[tauri::command]
pub async fn picture_save(state: State<'_, AppState>, doc: DocId, page: usize, idx: usize, fmt: i32, path: String) -> Res<String> {
    let engine = state.engine.clone();
    blocking(move || {
        let (bytes, ext) = engine.doc(doc)?.picture_data(page, idx, fmt)?;
        let p = std::path::Path::new(&path).with_extension(ext);
        std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
        Ok(p.to_string_lossy().into_owned())
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn save_pictures(
    state: State<'_, AppState>,
    doc: DocId,
    pages: Vec<usize>,
    names: Vec<String>,
    min: i32,
    fmt: i32,
    dir: String,
    stem: String,
) -> Res<crate::engine::SavedPictures> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.save_pictures(&pages, &names, min, fmt, &dir, &stem)).await
}

#[tauri::command]
pub async fn area_bytes(state: State<'_, AppState>, doc: DocId, page: usize, area: [f32; 4], rot: i32, kind: String, dpi: f32) -> Res<tauri::ipc::Response> {
    let engine = state.engine.clone();
    blocking(move || Ok(tauri::ipc::Response::new(engine.doc(doc)?.area_data(page, area, rot, &kind, dpi)?))).await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn area_save(state: State<'_, AppState>, doc: DocId, page: usize, area: [f32; 4], rot: i32, kind: String, dpi: f32, path: String) -> Res<()> {
    let engine = state.engine.clone();
    blocking(move || {
        let bytes = engine.doc(doc)?.area_data(page, area, rot, &kind, dpi)?;
        std::fs::write(&path, bytes).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn area_text(state: State<'_, AppState>, doc: DocId, page: usize, area: [f32; 4]) -> Res<String> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.area_text(page, area)).await
}

#[tauri::command]
pub async fn paragraphs(state: State<'_, AppState>, doc: DocId, page: usize) -> Res<Vec<crate::engine::Paragraph>> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.paragraphs(page)).await
}

/// Replaces a paragraph's text (one undo step).
#[tauri::command]
pub async fn edit_text(state: State<'_, AppState>, doc: DocId, page: usize, edit: crate::engine::TextEdit) -> Res<()> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.edit_text(page, &edit)).await
}

#[tauri::command]
pub async fn tables(state: State<'_, AppState>, doc: DocId, page: usize) -> Res<Vec<crate::engine::table::Table>> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.tables(page)).await
}

/// Reads the table in `area`, with the column lines `cols` if given (two or more).
#[tauri::command]
pub async fn table_in(state: State<'_, AppState>, doc: DocId, page: usize, area: [f32; 4], cols: Vec<f32>, rows: Option<Vec<f32>>) -> Res<Option<crate::engine::table::Table>> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.table_in(page, area, &cols, &rows.unwrap_or_default())).await
}

/// Writes a table as CSV or Excel at `path` (which must end in .csv or .xlsx).
#[tauri::command]
pub async fn table_save(table: crate::engine::table::Table, path: String) -> Res<()> {
    blocking(move || {
        let ext = std::path::Path::new(&path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let bytes = match ext.as_str() {
            "csv" => table.csv(),
            "xlsx" => table.xlsx(),
            _ => return Err("Tables are saved as .csv or .xlsx files.".into()),
        };
        std::fs::write(&path, bytes).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn attachments(state: State<'_, AppState>, doc: DocId) -> Res<Vec<crate::engine::Attachment>> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.attachments()).await
}

/// Saves attachment `i` at `path`, or (`dir` set) into that folder under its own name; returns the path used.
#[tauri::command]
pub async fn attachment_save(state: State<'_, AppState>, doc: DocId, i: usize, path: Option<String>, dir: Option<String>) -> Res<String> {
    let engine = state.engine.clone();
    blocking(move || {
        let d = engine.doc(doc)?;
        let bytes = d.attachment_data(i)?;
        let p = match (path, dir) {
            (Some(p), _) => std::path::PathBuf::from(p),
            (None, Some(dir)) => {
                let name = d.attachments()?.into_iter().find(|a| a.i == i).map(|a| a.name).unwrap_or_default();
                let file = std::path::Path::new(&name);
                let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("attachment").to_string();
                let ext = file.extension().and_then(|s| s.to_str()).unwrap_or("").to_string();
                crate::engine::free_path(&dir, &stem, &ext)
            }
            _ => return Err("Nowhere to save the attachment.".into()),
        };
        std::fs::write(&p, bytes).map_err(|e| e.to_string())?;
        Ok(p.to_string_lossy().into_owned())
    })
    .await
}

/// Attaches the file at `path` to the document (one undo step).
#[tauri::command]
pub async fn attachment_add(state: State<'_, AppState>, doc: DocId, path: String) -> Res<()> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.attachment_add(&path)).await
}

#[tauri::command]
pub async fn attachment_remove(state: State<'_, AppState>, doc: DocId, i: usize) -> Res<()> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.attachment_remove(i)).await
}

/// Adds redaction marks (one undo step); returns how many.
#[tauri::command]
pub async fn redact_mark(state: State<'_, AppState>, doc: DocId, marks: Vec<crate::engine::RedactMark>, author: String) -> Res<usize> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.redact_mark(&marks, &author)).await
}

#[tauri::command]
pub async fn redaction_pages(state: State<'_, AppState>, doc: DocId) -> Res<Vec<(usize, usize)>> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.redaction_pages()).await
}

/// Takes every redaction mark off (one undo step).
#[tauri::command]
pub async fn redact_clear(state: State<'_, AppState>, doc: DocId) -> Res<usize> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.redact_clear()).await
}

/// Applies every redaction mark (one undo step).
#[tauri::command]
pub async fn redact_apply(state: State<'_, AppState>, doc: DocId, cover: bool, rgb: [f32; 3], label: String) -> Res<crate::engine::Redacted> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.redact_apply(cover, rgb, &label)).await
}

/// Checks the document against PDF/A-`part` (1, 2 or 3) level b.
#[tauri::command]
pub async fn pdfa_check(state: State<'_, AppState>, doc: DocId, part: i32) -> Res<crate::engine::pdfa::Report> {
    let engine = state.engine.clone();
    blocking(move || engine.pdfa_check(doc, part)).await
}

/// For a file that says it's PDF/A: what its unsaved changes would break of that (null: nothing, or it doesn't).
#[tauri::command]
pub async fn pdfa_changes(state: State<'_, AppState>, doc: DocId) -> Res<Option<crate::engine::pdfa::Changes>> {
    let engine = state.engine.clone();
    blocking(move || engine.pdfa_changes(doc)).await
}

/// Makes the document's changes keep it PDF/A-`part` before it's saved (one undo step).
#[tauri::command]
pub async fn pdfa_keep(state: State<'_, AppState>, doc: DocId, part: i32) -> Res<crate::engine::pdfa::Kept> {
    let engine = state.engine.clone();
    blocking(move || engine.pdfa_keep(doc, part, &pdfa_find)).await
}

/// Installed fonts for PDF/A, as the engine asks for them.
fn pdfa_find(name: &str, bold: bool, italic: bool) -> Result<(std::path::PathBuf, String, bool), crate::engine::pdfa::FontMissing> {
    crate::fonts::find(name, bold, italic).map(|f| (f.path, f.family, f.exact)).map_err(|m| match m {
        crate::fonts::Missing::Licence => crate::engine::pdfa::FontMissing::Licence,
        crate::fonts::Missing::NotInstalled => crate::engine::pdfa::FontMissing::NotInstalled,
    })
}

/// Saves the pages in `order` (turned by `rot`) as a new PDF/A-`part` file at `out`, then checks it.
#[tauri::command]
pub async fn pdfa_save(state: State<'_, AppState>, doc: DocId, order: Vec<usize>, rot: Vec<i32>, out: String, part: i32) -> Res<crate::engine::pdfa::Saved> {
    let engine = state.engine.clone();
    blocking(move || engine.save_pdfa(doc, order, rot, &out, part, &pdfa_find)).await
}

/// Takes metadata (1), scripts (2), attached files (4), comments (8) or hidden text (16) out (one undo step).
#[tauri::command]
pub async fn sanitize(state: State<'_, AppState>, doc: DocId, what: i32) -> Res<crate::engine::Sanitized> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.sanitize(what)).await
}

/// Kinds of file that run code when opened: attachments like these are only saved, never opened.
const RISKY: &[&str] = &[
    "exe", "com", "bat", "cmd", "scr", "pif", "cpl", "msi", "msp", "msc", "dll", "sys", "jar", "js", "jse", "vbs", "vbe", "wsf", "wsh",
    "ps1", "psm1", "hta", "lnk", "url", "reg", "inf", "scf", "appref-ms", "application", "gadget", "chm", "iso", "img", "vhd", "vhdx", "docm",
    "xlsm", "pptm", "html", "htm", "svg", "xhtml", "mht",
];

/// Opens attachment `i` with its usual app (from a temporary copy). Files that could run code are refused.
#[tauri::command]
pub async fn attachment_open(app: tauri::AppHandle, state: State<'_, AppState>, doc: DocId, i: usize) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let engine = state.engine.clone();
    let path = blocking(move || {
        let d = engine.doc(doc)?;
        let name = d.attachments()?.into_iter().find(|a| a.i == i).map(|a| a.name).unwrap_or_default();
        let file = std::path::Path::new(&name);
        let ext = file.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext.is_empty() || RISKY.contains(&ext.as_str()) {
            return Err(format!("\"{name}\" could run programs on this PC, so Paginaut won't open it. Save it instead, and open it only if you trust where it came from."));
        }
        let dir = std::env::temp_dir().join("Paginaut").join("attachments");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("attachment").to_string();
        let p = crate::engine::free_path(&dir.to_string_lossy(), &stem, &ext);
        std::fs::write(&p, d.attachment_data(i)?).map_err(|e| e.to_string())?;
        Ok(p.to_string_lossy().into_owned())
    })
    .await?;
    app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
}

/// A picture dropped on the signature dialog from Explorer (pictures only, up to 40 MB).
#[tauri::command]
pub async fn read_picture(path: String) -> Res<tauri::ipc::Response> {
    blocking(move || {
        let p = std::path::Path::new(&path);
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if !["png", "jpg", "jpeg", "jfif", "bmp", "gif", "webp"].contains(&ext.as_str()) {
            return Err("That isn't a picture.".into());
        }
        let len = std::fs::metadata(p).map_err(|e| e.to_string())?.len();
        if len > 40 << 20 {
            return Err("That picture is too large.".into());
        }
        std::fs::read(p).map(tauri::ipc::Response::new).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn sig_list(app: tauri::AppHandle) -> Res<Vec<crate::sigstore::SigEntry>> {
    let dir = sig_dir(&app)?;
    blocking(move || crate::sigstore::load(&dir)).await
}

#[tauri::command]
pub async fn sig_add(app: tauri::AppHandle, kind: String, name: String, png: String) -> Res<crate::sigstore::SigEntry> {
    let dir = sig_dir(&app)?;
    blocking(move || {
        let mut list = crate::sigstore::load(&dir)?;
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let e = crate::sigstore::SigEntry { id: format!("{now:x}-{:x}", list.len()), kind, name, created: now, png };
        list.push(e.clone());
        crate::sigstore::save(&dir, &list)?;
        Ok(e)
    })
    .await
}

#[tauri::command]
pub async fn sig_delete(app: tauri::AppHandle, id: String) -> Res<()> {
    let dir = sig_dir(&app)?;
    blocking(move || {
        let mut list = crate::sigstore::load(&dir)?;
        list.retain(|e| e.id != id);
        crate::sigstore::save(&dir, &list)
    })
    .await
}

// ---------------------------------------------------------------- annotations

/// Properties of an annotation from the app (coordinates relative to the page's top-left).
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AnnotProps {
    rect: Option<[f32; 4]>,
    quads: Option<Vec<f32>>,
    ink: Option<Vec<Vec<f32>>>,
    line: Option<[f32; 4]>,
    line_endings: Option<[i32; 2]>,
    color: Option<Vec<f32>>,
    interior: Option<Vec<f32>>,
    opacity: Option<f32>,
    border: Option<f32>,
    icon: Option<String>,
    font: Option<(String, f32, Vec<f32>)>,
    /// Base64 PNG/JPEG.
    image: Option<String>,
    contents: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotResult {
    num: i32,
    changed: bool,
}

/// Adds (`num` absent), changes, or deletes an annotation; one undo step.
#[tauri::command]
pub async fn annot_edit(
    state: State<'_, AppState>,
    doc: DocId,
    page: usize,
    num: Option<i32>,
    kind: Option<String>,
    props: Option<AnnotProps>,
    delete: Option<bool>,
    author: Option<String>,
) -> Res<AnnotResult> {
    let engine = state.engine.clone();
    blocking(move || {
        let p = props.unwrap_or_default();
        let image = match p.image {
            Some(b64) => Some(crate::base64::decode(&b64).ok_or("bad image data")?),
            None => None,
        };
        let edit = crate::engine::mupdf::AnnotEdit {
            rect: p.rect, quads: p.quads, ink: p.ink, line: p.line, line_endings: p.line_endings, color: p.color, interior: p.interior,
            opacity: p.opacity, border: p.border, icon: p.icon, font: p.font, image, contents: p.contents,
        };
        let (num, change) = engine.doc(doc)?.annot_edit(page, num, kind.as_deref(), edit, delete.unwrap_or(false), author.as_deref().unwrap_or(""))?;
        Ok(AnnotResult { num, changed: change.changed })
    })
    .await
}

#[tauri::command]
pub async fn save_flattened(state: State<'_, AppState>, doc: DocId, target: String, order: Vec<usize>, rot: Vec<i32>, annots: bool) -> Res<()> {
    let engine = state.engine.clone();
    blocking(move || engine.save_flattened(doc, target, order, rot, annots)).await
}

// ---------------------------------------------------------------- digital signatures

/// Certificates you can sign with (from the Windows store, where tokens put theirs).
#[tauri::command]
pub async fn certs() -> Res<Vec<crate::dsc::CertSummary>> {
    blocking(|| {
        let mut list = crate::dsc::list()?;
        if let Some(c) = test_cert()? {
            let mut s = c.summary();
            s.thumb = "TEST".into();
            s.likely = true;
            list.insert(0, s);
        }
        Ok(list)
    })
    .await
}

/// Developers only: with PAGINAUT_TEST_CERT set, a throwaway certificate held in
/// memory is offered, to try signing without a token.
fn test_cert() -> Res<Option<crate::dsc::Cert>> {
    #[cfg(windows)]
    if std::env::var_os("PAGINAUT_TEST_CERT").is_some() {
        return crate::dsc::test_cert("Paginaut Test Signer").map(Some);
    }
    Ok(None)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignOptions {
    /// Certificate thumbprint.
    pub cert: String,
    /// Source page index; with `rect` relative to its origin. No rect: invisible.
    pub page: usize,
    pub rect: Option<[f32; 4]>,
    /// An empty signature field of the form to sign in (object number).
    pub field: Option<i32>,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub location: String,
    /// Base64 PNG shown in the box (e.g. your handwritten signature).
    pub image: Option<String>,
    /// PDF_SIGNATURE_SHOW_* flags.
    pub flags: i32,
    #[serde(default)]
    pub lock: bool,
    /// A .pfx/.p12 file to sign with instead of a certificate from the store (`cert` is "PFX").
    pub pfx: Option<PfxFile>,
    /// A timestamp service (RFC 3161) to get a trusted timestamp from.
    pub timestamp: Option<String>,
    /// Linux: the token's PIN (Windows' drivers ask for it themselves).
    #[serde(default)]
    #[cfg_attr(windows, allow(dead_code))]
    pub pin: Option<String>,
}

#[derive(Deserialize)]
pub struct PfxFile {
    pub path: String,
    pub password: String,
}

/// Linux: the PKCS#11 driver chosen in Settings for a token Paginaut doesn't find by itself (None: only those).
#[tauri::command]
pub fn pkcs11_module(path: Option<String>) {
    #[cfg(target_os = "linux")]
    crate::pkcs11::set_module(path);
    #[cfg(not(target_os = "linux"))]
    let _ = path;
}

/// A certificate file's certificate, to show before signing with it (checks the password too).
#[tauri::command]
pub async fn pfx_summary(path: String, password: String) -> Res<crate::dsc::CertSummary> {
    blocking(move || Ok(crate::dsc::Cert::from_pfx(&path, &password)?.summary())).await
}

/// Asks, online, whether signature `i`'s certificate has been revoked.
#[tauri::command]
pub async fn signature_revocation(state: State<'_, AppState>, doc: DocId, i: usize) -> Res<crate::dsc::Revocation> {
    let engine = state.engine.clone();
    blocking(move || {
        let (_, cms) = engine.sig_data(doc, i)?;
        Ok(crate::dsc::revocation(&cms))
    })
    .await
}

/// A signing error that means only the timestamp failed (the app offers to sign without one).
const TIMESTAMP_FAILED: &str = "TIMESTAMP|";

/// Signs with a certificate and saves (in place, or as `target`). The token's
/// driver asks for its PIN in its own window, over ours.
#[tauri::command]
pub async fn sign_document(window: tauri::WebviewWindow, state: State<'_, AppState>, doc: DocId, target: Option<String>, opts: SignOptions) -> Res<DocInfo> {
    #[cfg(windows)]
    let hwnd = window.hwnd().map(|h| h.0 as isize).unwrap_or(0);
    #[cfg(not(windows))]
    let hwnd = {
        let _ = &window;
        0isize
    };
    let engine = state.engine.clone();
    blocking(move || {
        #[allow(unused_mut)]
        let mut cert = match (opts.cert.as_str(), test_cert()?, &opts.pfx) {
            ("TEST", Some(c), _) => c,
            ("PFX", _, Some(f)) => crate::dsc::Cert::from_pfx(&f.path, &f.password)?,
            _ => crate::dsc::Cert::find(&opts.cert)?,
        };
        #[cfg(not(windows))]
        cert.set_pin(opts.pin.clone());
        let s = cert.summary();
        if !s.current {
            return Err(format!("This certificate isn't valid today (valid until {}).", fmt_day(s.not_after)));
        }
        let image = match opts.image.as_deref() {
            Some(b) => Some(crate::base64::decode(b).ok_or("bad picture")?),
            None => None,
        };
        let req = crate::engine::mupdf::SignRequest {
            field: opts.field.unwrap_or(-1),
            rect: opts.rect.unwrap_or([0.0; 4]),
            dn: cert.dn(),
            reason: opts.reason,
            location: opts.location,
            image,
            flags: opts.flags,
            lock: opts.lock,
            // Room for the signature with a long certificate chain (and a timestamp, which brings its own).
            max: if opts.timestamp.is_some() { 40 * 1024 } else { 24 * 1024 },
        };
        let tsa = opts.timestamp.filter(|u| !u.trim().is_empty());
        engine.sign(doc, target, opts.page, req, |data| {
            let cms = cert.sign(data, hwnd)?;
            match &tsa {
                Some(url) => crate::dsc::add_timestamp(&cms, url.trim()).map(|(c, _)| c).map_err(|e| format!("{TIMESTAMP_FAILED}{e}")),
                None => Ok(cms),
            }
        })
    })
    .await
}

fn fmt_day(unix: i64) -> String {
    let days = unix.div_euclid(86400);
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{d:02}-{m:02}-{y}")
}

/// A PDF date ("D:20260929143000+05'30'") as seconds since 1970.
fn pdf_date(s: &str) -> Option<i64> {
    let s = s.strip_prefix("D:").unwrap_or(s);
    let num = |a: usize, b: usize, def: i64| s.get(a..b).and_then(|x| x.parse::<i64>().ok()).unwrap_or(def);
    if s.len() < 4 {
        return None;
    }
    let (y, mo, d, h, mi, se) = (num(0, 4, 1970), num(4, 6, 1), num(6, 8, 1), num(8, 10, 0), num(10, 12, 0), num(12, 14, 0));
    // Days from civil (inverse of the above).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let mut t = days * 86400 + h * 3600 + mi * 60 + se;
    if let Some(z) = s.get(14..15) {
        let off = num(15, 17, 0) * 3600 + num(18, 20, 0) * 60;
        match z {
            "+" => t -= off,
            "-" => t += off,
            _ => {}
        }
    }
    Some(t)
}

/// Every signature in the document, each checked: intact, signer, trust.
#[tauri::command]
pub async fn signatures(state: State<'_, AppState>, doc: DocId) -> Res<Vec<serde_json::Value>> {
    let engine = state.engine.clone();
    blocking(move || {
        let list: Vec<serde_json::Value> = serde_json::from_str(&engine.sigs_json(doc)?).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for mut s in list {
            if s["signed"] == true {
                let i = s["i"].as_u64().unwrap_or(0) as usize;
                let when = s["m"].as_str().and_then(pdf_date);
                let check = match engine.sig_data(doc, i) {
                    Ok((data, cms)) => crate::dsc::verify(&data, &cms, when),
                    Err(e) => crate::dsc::SigCheck { trust: "invalid".into(), detail: format!("The signature is damaged: {e}"), ..Default::default() },
                };
                s["check"] = serde_json::to_value(check).map_err(|e| e.to_string())?;
                s["time"] = when.map_or(serde_json::Value::Null, |t| t.into());
            }
            out.push(s);
        }
        Ok(out)
    })
    .await
}

/// The document exactly as signature `i` signed it (its bytes up to that
/// signature), written to a temporary file to open.
#[tauri::command]
pub async fn signed_version(state: State<'_, AppState>, doc: DocId, i: usize) -> Res<String> {
    let engine = state.engine.clone();
    blocking(move || {
        let list: Vec<serde_json::Value> = serde_json::from_str(&engine.sigs_json(doc)?).map_err(|e| e.to_string())?;
        let s = list.iter().find(|s| s["i"].as_u64() == Some(i as u64)).ok_or("no such signature")?;
        let end = s["end"].as_u64().ok_or("not signed")? as usize;
        let path = engine.doc(doc)?.path();
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        if end == 0 || end > bytes.len() {
            return Err("The signature's byte range doesn't fit the file.".into());
        }
        let stem = std::path::Path::new(&path).file_stem().and_then(|x| x.to_str()).unwrap_or("document").to_string();
        let dir = std::env::temp_dir().join("Paginaut");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let name = s["name"].as_str().unwrap_or("signature").replace(|c: char| !c.is_alphanumeric() && c != ' ', "");
        let out = dir.join(format!("{stem} (as signed - {name}).pdf"));
        std::fs::write(&out, &bytes[..end]).map_err(|e| e.to_string())?;
        Ok(out.to_string_lossy().into_owned())
    })
    .await
}

// ---------------------------------------------------------------- OCR

use crate::ocr::{ModelDirs, Quality};

/// OCR languages, with which models are built in or downloaded.
#[tauri::command]
pub fn ocr_languages(dirs: State<'_, ModelDirs>) -> Vec<crate::ocr::LangInfo> {
    dirs.languages()
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DownloadEvent {
    Progress { done: u64, total: u64 },
}

/// Downloads a language model; resolves when it's ready to use.
#[tauri::command]
pub async fn ocr_download(dirs: State<'_, ModelDirs>, code: String, quality: String, on_event: Channel<DownloadEvent>) -> Res<()> {
    let dirs = dirs.inner().clone();
    let q = Quality::parse(&quality);
    let key = crate::ocr::download_key(&code, q);
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    crate::ocr::DOWNLOADS.lock().insert(key.clone(), stop.clone());
    let res = blocking(move || {
        let mut last = 0u64;
        dirs.download(&code, q, &stop, |done, total| {
            // A progress event per 1% is plenty.
            if done == total || done - last >= total / 100 {
                last = done;
                let _ = on_event.send(DownloadEvent::Progress { done, total });
            }
        })
    })
    .await;
    crate::ocr::DOWNLOADS.lock().remove(&key);
    res
}

#[tauri::command]
pub fn ocr_download_stop(code: String, quality: String) {
    crate::ocr::stop_download(&code, Quality::parse(&quality));
}

#[tauri::command]
pub async fn ocr_delete(dirs: State<'_, ModelDirs>, code: String, quality: String) -> Res<()> {
    let dirs = dirs.inner().clone();
    blocking(move || dirs.delete(&code, Quality::parse(&quality))).await
}

/// How many of a sample of pages are scans without text.
#[tauri::command]
pub async fn ocr_check(state: State<'_, AppState>, doc: DocId) -> Res<crate::engine::ocr::ScanCheck> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.scan_check()).await
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OcrEvent {
    Progress { done: usize, total: usize },
    Done { end: crate::engine::ocr::OcrEnd },
}

/// Recognises text on `pages` (source indices, in the order to read them) with
/// the given languages. Cancelling `sid` stops it and keeps what was read.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn ocr_run(
    state: State<'_, AppState>,
    dirs: State<'_, ModelDirs>,
    doc: DocId,
    pages: Vec<usize>,
    langs: Vec<String>,
    quality: String,
    replace: Option<bool>,
    sid: u64,
    on_event: Channel<OcrEvent>,
) -> Res<()> {
    let models = dirs.models(&langs, Quality::parse(&quality))?;
    let progress = on_event.clone();
    state.engine.ocr(
        doc,
        pages,
        models,
        replace.unwrap_or(false),
        sid,
        move |done, total| {
            let _ = progress.send(OcrEvent::Progress { done, total });
        },
        move |end| {
            let _ = on_event.send(OcrEvent::Done { end });
        },
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrRemoved {
    pages: usize,
    dirty: bool,
}

/// How many pages carry text Paginaut recognised (which recognising again can replace).
#[tauri::command]
pub async fn ocr_layers(state: State<'_, AppState>, doc: DocId) -> Res<usize> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.ocr_layer_pages()).await
}

/// Removes the text Paginaut recognised on every page (one undo step).
#[tauri::command]
pub async fn ocr_remove(state: State<'_, AppState>, doc: DocId) -> Res<OcrRemoved> {
    let engine = state.engine.clone();
    blocking(move || {
        let d = engine.doc(doc)?;
        let pages = d.remove_ocr()?;
        Ok(OcrRemoved { pages, dirty: d.has_changes() })
    })
    .await
}

// ---------------------------------------------------------------- Office files

use crate::office::{libre::Stage, Engines, Office};

/// What can convert Office files here (`fresh`: look again, e.g. after installing Office).
#[tauri::command]
pub async fn office_engines(office: State<'_, Arc<Office>>, fresh: bool) -> Res<Engines> {
    let office = office.inner().clone();
    blocking(move || {
        if fresh {
            office.forget();
        }
        Ok(office.engines())
    })
    .await
}

#[tauri::command]
pub fn office_set_preference(office: State<'_, Arc<Office>>, preference: String) {
    office.set_preference(&preference);
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum InstallEvent {
    Progress { stage: Stage, done: u64, total: u64 },
}

static OFFICE_INSTALL_STOP: once_cell::sync::Lazy<Arc<std::sync::atomic::AtomicBool>> = once_cell::sync::Lazy::new(|| Arc::new(std::sync::atomic::AtomicBool::new(false)));

/// Downloads and sets up Paginaut's LibreOffice; resolves when it's ready.
#[tauri::command]
pub async fn office_install(office: State<'_, Arc<Office>>, on_event: Channel<InstallEvent>) -> Res<Engines> {
    let office = office.inner().clone();
    OFFICE_INSTALL_STOP.store(false, std::sync::atomic::Ordering::Release);
    blocking(move || {
        let mut last = (0u64, 99u8);
        office.install_pack(&OFFICE_INSTALL_STOP, &mut |stage, done, total| {
            // A progress event per 1% is plenty.
            let s = stage as u8;
            if s != last.1 || done == total || done.saturating_sub(last.0) >= total / 100 {
                last = (done, s);
                let _ = on_event.send(InstallEvent::Progress { stage, done, total });
            }
        })?;
        Ok(office.engines())
    })
    .await
}

#[tauri::command]
pub fn office_install_stop() {
    OFFICE_INSTALL_STOP.store(true, std::sync::atomic::Ordering::Release);
}

#[tauri::command]
pub async fn office_remove(office: State<'_, Arc<Office>>) -> Res<Engines> {
    let office = office.inner().clone();
    blocking(move || {
        office.remove_pack()?;
        Ok(office.engines())
    })
    .await
}

/// A file made into a PDF: who made it, and for PDF/A what couldn't be put right and which fonts stood in.
#[derive(Serialize)]
pub struct FileConverted {
    by: String,
    left: Vec<String>,
    substitutes: Vec<String>,
}

/// Converts a file into the PDF `out`: an Office file with `engine` (or the preferred converter), anything else
/// Paginaut opens (web pages, e-books, pictures, XPS) by itself. With `pdfa` (a part), saved as PDF/A.
#[tauri::command]
pub async fn office_convert(
    state: State<'_, AppState>,
    office: State<'_, Arc<Office>>,
    path: String,
    out: String,
    engine: Option<String>,
    pdfa: Option<i32>,
) -> Res<FileConverted> {
    let office = office.inner().clone();
    let engine_ = state.engine.clone();
    blocking(move || {
        let Some(part) = pdfa else {
            let by = if crate::office::kind(&path).is_some() {
                office.convert_to(&path, &out, engine.as_deref())?
            } else {
                engine_.convert_file(&path, &out)?;
                "Paginaut".into()
            };
            return Ok(FileConverted { by, left: vec![], substitutes: vec![] });
        };
        // PDF/A: the PDF the file becomes (an Office file's cached conversion, else one made beside it), saved as PDF/A.
        let name = std::path::Path::new(&path).file_name().map_or(path.clone(), |n| n.to_string_lossy().into_owned());
        let made = format!("{out}.paginaut-pdf");
        let (src, by) = if crate::office::kind(&path).is_some() {
            let c = office.convert(&path, engine.as_deref())?.ok_or_else(|| format!("{name} needs Microsoft Office or LibreOffice to become a PDF."))?;
            (c.pdf, c.engine_name)
        } else {
            engine_.convert_file(&path, &made)?;
            (made.clone(), "Paginaut".into())
        };
        let saved = engine_.pdfa_from_file(&src, &out, part, &pdfa_find);
        let _ = std::fs::remove_file(&made);
        let saved = saved?;
        Ok(FileConverted { by, left: saved.left.issues.iter().map(|i| i.text.clone()).collect(), substitutes: saved.substitutes })
    })
    .await
}

// ---------------------------------------------------------------- Markdown

fn markdown_path(path: &str) -> Res<()> {
    let ext = std::path::Path::new(path).extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "md" || ext == "markdown" {
        Ok(())
    } else {
        Err("Only Markdown files (.md) can be edited here.".into())
    }
}

/// A Markdown file's text, to edit.
#[tauri::command]
pub async fn markdown_read(path: String) -> Res<String> {
    blocking(move || {
        markdown_path(&path)?;
        let data = std::fs::read(&path).map_err(|e| e.to_string())?;
        if data.len() > 8 << 20 {
            return Err("The file is too large to edit here.".into());
        }
        Ok(String::from_utf8_lossy(data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&data)).into_owned())
    })
    .await
}

/// Shows `text` as the Markdown file at `path` (its pictures read from its folder): a new document.
#[tauri::command]
pub async fn markdown_preview(state: State<'_, AppState>, path: String, text: String) -> Res<DocInfo> {
    let engine = state.engine.clone();
    blocking(move || {
        markdown_path(&path)?;
        engine.open_markdown(&path, &text)
    })
    .await
}

/// Writes the Markdown file (beside it first, so a failure never leaves half a file).
#[tauri::command]
pub async fn markdown_save(path: String, text: String) -> Res<()> {
    blocking(move || {
        markdown_path(&path)?;
        let tmp = format!("{path}.paginaut-tmp");
        std::fs::write(&tmp, text.as_bytes()).map_err(|e| format!("Couldn't save: {e}"))?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("Couldn't save: {e}")
        })
    })
    .await
}

// ---------------------------------------------------------------- converting from PDF

#[tauri::command]
pub async fn convert_options(office: State<'_, Arc<Office>>) -> Res<crate::convert::Options> {
    let office = office.inner().clone();
    blocking(move || Ok(crate::convert::options(&office))).await
}

/// The Stop of each running conversion or comparison, by the id the page gave the job (ids are unique across
/// windows, see api.ts newId), so a Stop reaches that job alone.
static JOB_STOPS: once_cell::sync::Lazy<Mutex<std::collections::HashMap<u64, Arc<std::sync::atomic::AtomicBool>>>> = once_cell::sync::Lazy::new(Default::default);

/// A running job's Stop, forgotten when the job ends.
struct JobStop(u64, Arc<std::sync::atomic::AtomicBool>);

impl JobStop {
    fn new(sid: u64) -> Self {
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        JOB_STOPS.lock().insert(sid, flag.clone());
        JobStop(sid, flag)
    }
}

impl Drop for JobStop {
    fn drop(&mut self) {
        JOB_STOPS.lock().remove(&self.0);
    }
}

fn stop_job(sid: u64) {
    if let Some(f) = JOB_STOPS.lock().get(&sid) {
        f.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// The document's pages as Word, Excel, PowerPoint, pictures or text (see convert.rs).
#[tauri::command]
pub async fn convert_from_pdf(
    state: State<'_, AppState>,
    office: State<'_, Arc<Office>>,
    req: crate::convert::Request,
    sid: u64,
    on_event: Channel<DownloadEvent>,
) -> Res<crate::convert::Done> {
    let office = office.inner().clone();
    let engine = state.engine.clone();
    let job = JobStop::new(sid);
    blocking(move || {
        crate::convert::run(&engine, &office, &req, &job.1, &mut |done, total| {
            let _ = on_event.send(DownloadEvent::Progress { done: done as u64, total: total as u64 });
        })
    })
    .await
}

#[tauri::command]
pub fn convert_stop(sid: u64) {
    stop_job(sid);
}

// ---------------------------------------------------------------- compare

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CompareEvent {
    Reading { side: String, done: usize, total: usize },
    Comparing,
    Pictures { done: usize, total: usize },
}

/// Lines two documents up word by word (see compare.rs). Its background work runs under `sid`, so Stop can
/// cancel it.
#[tauri::command]
pub async fn compare_run(
    state: State<'_, AppState>,
    a: crate::compare::SideReq,
    b: crate::compare::SideReq,
    options: crate::compare::Options,
    sid: u64,
    on_event: Channel<CompareEvent>,
) -> Res<crate::compare::Outcome> {
    let engine = state.engine.clone();
    let job = JobStop::new(sid);
    blocking(move || {
        crate::compare::run(&engine, &a, &b, options, sid, &job.1, &|p| {
            let _ = on_event.send(match p {
                crate::compare::Progress::Reading(side, done, total) => CompareEvent::Reading { side: side.into(), done, total },
                crate::compare::Progress::Comparing => CompareEvent::Comparing,
                crate::compare::Progress::Pictures(done, total) => CompareEvent::Pictures { done, total },
            });
        })
    })
    .await
}

#[tauri::command]
pub fn compare_stop(state: State<'_, AppState>, sid: u64) {
    stop_job(sid);
    state.engine.cancel(sid);
}

// ---------------------------------------------------------------- fonts (themes)

fn fonts_dir(app: &tauri::AppHandle) -> Res<std::path::PathBuf> {
    app.path().app_local_data_dir().map(|d| d.join("fonts")).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fonts_installed() -> Res<Vec<String>> {
    blocking(|| Ok(crate::fonts::installed())).await
}

#[tauri::command]
pub async fn fonts_user(app: tauri::AppHandle) -> Res<Vec<crate::fonts::UserFont>> {
    let dir = fonts_dir(&app)?;
    blocking(move || Ok(crate::fonts::user(&dir))).await
}

#[tauri::command]
pub async fn font_add(app: tauri::AppHandle, path: String) -> Res<crate::fonts::UserFont> {
    let dir = fonts_dir(&app)?;
    blocking(move || crate::fonts::add(&dir, &path)).await
}

#[tauri::command]
pub async fn font_bytes(app: tauri::AppHandle, file: String) -> Res<tauri::ipc::Response> {
    let dir = fonts_dir(&app)?;
    blocking(move || std::fs::read(crate::fonts::file(&dir, &file)?).map(tauri::ipc::Response::new).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn font_remove(app: tauri::AppHandle, file: String) -> Res<()> {
    let dir = fonts_dir(&app)?;
    blocking(move || std::fs::remove_file(crate::fonts::file(&dir, &file)?).map_err(|e| e.to_string())).await
}

// ---------------------------------------------------------------- skin (a photo behind the bars and Home)

fn skin_dir(app: &tauri::AppHandle) -> Res<std::path::PathBuf> {
    app.path().app_local_data_dir().map(|d| d.join("skin")).map_err(|e| e.to_string())
}

fn skin_file(dir: &std::path::Path, name: &str) -> Res<std::path::PathBuf> {
    let ok = name.starts_with("skin-") && name.ends_with(".jpg") && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'.');
    if ok { Ok(dir.join(name)) } else { Err("Unknown skin.".into()) }
}

/// Keeps a skin's photo (a JPEG the app made, as base64) in place of the one before; returns its file name.
#[tauri::command]
pub async fn skin_set(app: tauri::AppHandle, jpeg: String) -> Res<String> {
    let dir = skin_dir(&app)?;
    blocking(move || {
        let bytes = crate::base64::decode(&jpeg).ok_or("bad picture")?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let name = format!("skin-{t}.jpg");
        // The new photo first: if writing it fails, the one before is still there for the settings that name it.
        std::fs::write(dir.join(&name), bytes).map_err(|e| e.to_string())?;
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if e.file_name().to_str() != Some(name.as_str()) {
                let _ = std::fs::remove_file(e.path());
            }
        }
        Ok(name)
    })
    .await
}

#[tauri::command]
pub async fn skin_bytes(app: tauri::AppHandle, file: String) -> Res<tauri::ipc::Response> {
    let dir = skin_dir(&app)?;
    blocking(move || std::fs::read(skin_file(&dir, &file)?).map(tauri::ipc::Response::new).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn skin_clear(app: tauri::AppHandle) -> Res<()> {
    let dir = skin_dir(&app)?;
    blocking(move || {
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    })
    .await
}

// ---------------------------------------------------------------- updates

static UPDATE_STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Checks the releases for a newer Paginaut, and fetches the newest signed add-on list on the way (a list that's
/// missing or doesn't check out is ignored: the one there already stays).
#[tauri::command]
pub async fn update_check(app: tauri::AppHandle) -> Res<crate::update::Checked> {
    let data = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    blocking(move || {
        let stop = std::sync::atomic::AtomicBool::new(false);
        let checked = crate::update::check(&stop)?;
        let _ = crate::addons::refresh(&data, &stop);
        Ok(checked)
    })
    .await
}

/// Downloads the newer version found by the check, checks its signature, starts its installer and quits. False
/// when Paginaut stays open (Linux's .deb: the software installer takes over).
#[tauri::command]
pub async fn update_install(window: tauri::WebviewWindow, on_event: Channel<DownloadEvent>) -> Res<bool> {
    let app = window.app_handle().clone();
    // This window has checked its own tabs; quitting closes the others too.
    if crate::appwindows::others_dirty(&app, window.label()) {
        return Err("Another Paginaut window has unsaved changes: save or close them first.".into());
    }
    let dir = std::env::temp_dir().join("Paginaut update");
    // Installers from earlier updates (and a stopped download) go first: only the new one is kept.
    let _ = std::fs::remove_dir_all(&dir);
    UPDATE_STOP.store(false, std::sync::atomic::Ordering::Release);
    let installer = blocking(move || {
        let mut last = 0u64;
        crate::update::download(&dir, &UPDATE_STOP, &mut |done, total| {
            if done == total || done - last >= total / 100 {
                last = done;
                let _ = on_event.send(DownloadEvent::Progress { done, total });
            }
        })
    })
    .await?;
    // (On Linux this waits for apt, after the password prompt.)
    if blocking(move || crate::update::run_installer(&installer)).await? {
        app.exit(0);
        return Ok(true);
    }
    Ok(false)
}

#[tauri::command]
pub fn update_stop() {
    UPDATE_STOP.store(true, std::sync::atomic::Ordering::Release);
}

// ---------------------------------------------------------------- default app

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefaultApps {
    /// This copy is installed: only then may it offer itself (a development build or a copied folder would take
    /// the installed copy's place).
    installed: bool,
    kinds: Vec<crate::defaults::KindStatus>,
}

/// Which file types Paginaut is offered for, and where it's the default now.
#[tauri::command]
pub async fn default_apps_status() -> Res<DefaultApps> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    blocking(move || Ok(DefaultApps { installed: crate::defaults::installed(&exe), kinds: crate::defaults::status(&crate::defaults::PAGINAUT, &exe) })).await
}

/// Offers Paginaut for exactly these kinds of file (withdrawn from the rest). Installed copies only.
#[tauri::command]
pub async fn default_apps_set(kinds: Vec<String>) -> Res<DefaultApps> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if !crate::defaults::installed(&exe) {
        return Err("Only an installed Paginaut can be offered in Windows; this copy isn't installed.".into());
    }
    blocking(move || {
        let ids: Vec<&str> = kinds.iter().map(String::as_str).collect();
        crate::defaults::register(&crate::defaults::PAGINAUT, &exe, &ids)?;
        Ok(DefaultApps { installed: true, kinds: crate::defaults::status(&crate::defaults::PAGINAUT, &exe) })
    })
    .await
}

/// Windows' Default apps page, opened at Paginaut: Windows' own call for it first (what browsers use), else the
/// Settings link (which some Windows versions open at the list of apps instead).
#[tauri::command]
pub async fn default_apps_open(app: tauri::AppHandle) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    if blocking(|| crate::defaults::open_settings(&crate::defaults::PAGINAUT)).await.is_ok() {
        return Ok(());
    }
    app.opener().open_url("ms-settings:defaultapps?registeredAppUser=Paginaut", None::<&str>).map_err(|e| e.to_string())
}

/// Opens a file Paginaut just made in the program Windows uses for it.
#[tauri::command]
pub fn open_path(app: tauri::AppHandle, path: String) -> Res<()> {
    use tauri_plugin_opener::OpenerExt;
    let ext = std::path::Path::new(&path).extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    if !["docx", "xlsx", "pptx", "md", "png", "jpg", "tiff", "txt", "pdf"].contains(&ext.as_str()) {
        return Err("Paginaut only opens the files it made.".into());
    }
    app.opener().open_path(path, None::<&str>).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- app

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    cpus: usize,
    workers: usize,
    total_memory: u64,
    busy: usize,
    queued: usize,
    process_memory: u64,
    uptime_ms: u128,
}

/// Private memory of this process plus everything it spawned (the WebView2
/// browser, GPU and renderer processes). On Windows sysinfo's `virtual_memory`
/// is the private commit; `memory` is the working set, which double-counts
/// DLLs and other memory shared between those processes.
///
/// Finding those processes means reading every process on the machine (a few
/// hundred, ~6 ms), so that is done every 30 s; in between, only the ones found
/// are asked, straight from Windows (well under a millisecond). WebView2 starts
/// and ends processes rarely.
fn app_memory() -> u64 {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    static TREE: Mutex<Option<(Vec<Pid>, std::time::Instant)>> = Mutex::new(None);
    let Ok(me) = sysinfo::get_current_pid() else { return 0 };
    let mut guard = TREE.lock();
    if let Some((pids, found)) = guard.as_ref() {
        if found.elapsed() < std::time::Duration::from_secs(30) {
            if let Some(total) = pids.iter().map(|p| private_bytes(p.as_u32())).sum::<Option<u64>>() {
                return total;
            }
            // One has ended: look again.
        }
    }
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_memory());
    let procs = sys.processes();
    let mine = |pid: Pid| {
        let mut cur = Some(pid);
        for _ in 0..8 {
            match cur {
                Some(c) if c == me => return true,
                Some(c) => cur = procs.get(&c).and_then(|x| x.parent()),
                None => return false,
            }
        }
        false
    };
    // (On Linux sysinfo lists each thread as a process too: those share their process's memory.)
    let pids: Vec<Pid> = procs.iter().filter(|(p, x)| x.thread_kind().is_none() && mine(**p)).map(|(p, _)| *p).collect();
    // (sysinfo's figure is the commit charge on Windows, but the whole address space on Linux.)
    let total = pids.iter().filter_map(|p| if cfg!(windows) { procs.get(p).map(|x| x.virtual_memory()) } else { private_bytes(p.as_u32()) }).sum();
    *guard = Some((pids, std::time::Instant::now()));
    total
}

/// A process's private memory (commit), as sysinfo reports it, without listing every process.
#[cfg(windows)]
fn private_bytes(pid: u32) -> Option<u64> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: the handle is checked, used once and closed; the counters struct is sized for the call.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut c = PROCESS_MEMORY_COUNTERS_EX::default();
        let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let ok = GetProcessMemoryInfo(h, &mut c as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS, size);
        let _ = CloseHandle(h);
        ok.ok()?;
        Some(c.PrivateUsage as u64)
    }
}

/// Linux: the process's private memory (resident pages no other process shares), from /proc.
#[cfg(not(windows))]
fn private_bytes(pid: u32) -> Option<u64> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).ok()?;
    let kb: u64 = text
        .lines()
        .filter(|l| l.starts_with("Private_Clean:") || l.starts_with("Private_Dirty:"))
        .filter_map(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        .sum();
    Some(kb * 1024)
}

#[tauri::command]
pub async fn system_info(state: State<'_, AppState>) -> Res<SystemInfo> {
    let (busy, queued) = state.engine.load();
    let workers = state.engine.workers();
    let uptime_ms = state.started.elapsed().as_millis();
    blocking(move || {
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        Ok(SystemInfo {
            cpus: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            workers,
            total_memory: sys.total_memory(),
            busy,
            queued,
            process_memory: app_memory(),
            uptime_ms,
        })
    })
    .await
}

// ---------------------------------------------------------------- read aloud

/// Windows' voices, then the Piper voices installed.
#[tauri::command]
pub async fn tts_voices(piper: State<'_, crate::piper::Voices>) -> Res<Vec<crate::tts::Voice>> {
    let piper = piper.inner().clone();
    blocking(move || {
        // Without Windows' voices (some editions), Piper's still read.
        let mut list = crate::tts::voices().unwrap_or_default();
        list.extend(piper.installed());
        Ok(list)
    })
    .await
}

/// The spoken text as [u32 length of the words' JSON][the words' JSON][WAV], all in one reply.
#[tauri::command]
pub async fn tts_speak(piper: State<'_, crate::piper::Voices>, text: String, voice: String, rate: f64) -> Res<tauri::ipc::Response> {
    let piper = piper.inner().clone();
    blocking(move || {
        let (wav, words) = match voice.strip_prefix(crate::piper::PREFIX) {
            Some(id) => piper.speak(id, &text, rate)?,
            // Linux has only Piper's voices: "automatic" is the first one installed (the page picks one for
            // the text's language when there is one).
            #[cfg(not(windows))]
            None => {
                let first = piper.installed().into_iter().next().ok_or("Get a voice first: Settings › Read aloud.")?;
                piper.speak(first.id.strip_prefix(crate::piper::PREFIX).unwrap_or(&first.id), &text, rate)?
            }
            #[cfg(windows)]
            None => crate::tts::speak(&text, &voice, rate)?,
        };
        let json = serde_json::to_vec(&words).map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(4 + json.len() + wav.len());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(&json);
        out.extend_from_slice(&wav);
        Ok(tauri::ipc::Response::new(out))
    })
    .await
}

/// The Piper voices on offer, with which are installed.
#[tauri::command]
pub fn piper_voices(piper: State<'_, crate::piper::Voices>) -> Vec<crate::piper::VoiceInfo> {
    piper.list()
}

/// Downloads a Piper voice (and Piper, the first time); resolves when it's ready to use.
#[tauri::command]
pub async fn piper_download(piper: State<'_, crate::piper::Voices>, id: String, on_event: Channel<DownloadEvent>) -> Res<()> {
    let piper = piper.inner().clone();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    if crate::piper::DOWNLOADS.lock().insert(id.clone(), stop.clone()).is_some() {
        return Err("That voice is already downloading.".into());
    }
    let key = id.clone();
    let res = blocking(move || {
        let mut last = 0u64;
        piper.download(&id, &stop, &mut |done, total| {
            if done == total || done.saturating_sub(last) >= total / 100 {
                last = done;
                let _ = on_event.send(DownloadEvent::Progress { done, total });
            }
        })
    })
    .await;
    crate::piper::DOWNLOADS.lock().remove(&key);
    res
}

#[tauri::command]
pub fn piper_download_stop(id: String) {
    if let Some(flag) = crate::piper::DOWNLOADS.lock().get(&id) {
        flag.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// A voice's sample recording (MP3).
#[tauri::command]
pub async fn piper_sample(piper: State<'_, crate::piper::Voices>, id: String) -> Res<tauri::ipc::Response> {
    let piper = piper.inner().clone();
    blocking(move || piper.sample(&id).map(tauri::ipc::Response::new)).await
}

#[tauri::command]
pub async fn piper_delete(piper: State<'_, crate::piper::Voices>, id: String) -> Res<()> {
    let piper = piper.inner().clone();
    blocking(move || piper.delete(&id)).await
}

// ---------------------------------------------------------------- printing

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PrintEvent {
    /// The dialog said print: this many pages.
    Started { total: usize },
    Page { done: usize },
}

static PRINTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static PRINT_STOP: once_cell::sync::Lazy<Arc<std::sync::atomic::AtomicBool>> = once_cell::sync::Lazy::new(Default::default);

/// Shows Windows' print dialog and prints `pages` (as shown: file index and extra turn); `current` is the page
/// in view (an index into `pages`). Resolves with the pages printed, or None when the dialog was cancelled.
#[tauri::command]
pub async fn print_doc(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    doc: DocId,
    name: String,
    pages: Vec<serde_json::Value>,
    current: usize,
    on_event: Channel<PrintEvent>,
) -> Res<Option<usize>> {
    #[cfg(windows)]
    {
        use std::sync::atomic::Ordering;
        let pages: Vec<crate::print::Shown> = pages.into_iter().map(serde_json::from_value).collect::<Result<_, _>>().map_err(|e| e.to_string())?;
        let hwnd = window.hwnd().map(|h| h.0 as isize).unwrap_or(0);
        let d = state.engine.doc(doc)?;
        if PRINTING.swap(true, Ordering::AcqRel) {
            return Err("Something is already printing.".into());
        }
        let r = async {
            // The dialog runs on the window's thread (see print::ask), modal: the window still paints meanwhile.
            let (tx, rx) = tauri::async_runtime::channel(1);
            let count = pages.len();
            window
                .run_on_main_thread(move || {
                    let _ = tx.try_send(crate::print::ask(hwnd, count, current));
                })
                .map_err(|e| e.to_string())?;
            let mut rx = rx;
            let Some(chosen) = rx.recv().await.ok_or("The print dialog closed unexpectedly.")?? else { return Ok(None) };
            let _ = on_event.send(PrintEvent::Started { total: chosen.pages.len() });
            PRINT_STOP.store(false, Ordering::Release);
            let stop = PRINT_STOP.clone();
            blocking(move || {
                crate::print::print(chosen, &d, &name, &pages, &stop, |done| {
                    let _ = on_event.send(PrintEvent::Page { done });
                })
            })
            .await
            .map(Some)
        }
        .await;
        PRINTING.store(false, Ordering::Release);
        r
    }
    #[cfg(target_os = "linux")]
    {
        use std::sync::atomic::Ordering;
        let pages: Vec<crate::print_linux::Shown> = pages.into_iter().map(serde_json::from_value).collect::<Result<_, _>>().map_err(|e| e.to_string())?;
        if PRINTING.swap(true, Ordering::AcqRel) {
            return Err("Something is already printing.".into());
        }
        let engine = state.engine.clone();
        let r = async {
            let (tx, mut rx) = tauri::async_runtime::channel(1);
            let (w, count) = (window.clone(), pages.len());
            window.run_on_main_thread(move || {
                let _ = tx.try_send(crate::print_linux::ask(&w, count, current));
            })
            .map_err(|e| e.to_string())?;
            let Some(chosen) = rx.recv().await.ok_or("The print dialog closed unexpectedly.")? else { return Ok(None) };
            let total = chosen.len();
            let _ = on_event.send(PrintEvent::Started { total });
            let file = std::env::temp_dir().join(format!("paginaut-print-{}.pdf", std::process::id())).to_string_lossy().into_owned();
            let f = file.clone();
            blocking(move || crate::print_linux::write(&engine, doc, &pages, &chosen, &f)).await?;
            let (tx, mut rx) = tauri::async_runtime::channel(1);
            window
                .run_on_main_thread(move || {
                    crate::print_linux::send(&name, file, move |r| {
                        let _ = tx.try_send(r);
                    })
                })
                .map_err(|e| e.to_string())?;
            rx.recv().await.ok_or("The printer stopped answering.")??;
            let _ = on_event.send(PrintEvent::Page { done: total });
            Ok(Some(total))
        }
        .await;
        PRINTING.store(false, Ordering::Release);
        r
    }
}

#[tauri::command]
pub fn print_stop() {
    PRINT_STOP.store(true, std::sync::atomic::Ordering::Release);
}

/// Where the page's maximize button is (physical pixels of the client area), for Snap Layouts (snap.rs).
#[tauri::command]
pub fn snap_button(window: tauri::WebviewWindow, x: i32, y: i32, w: i32, h: i32, shown: bool) -> Res<()> {
    #[cfg(windows)]
    return crate::snap::place(window.hwnd().map_err(|e| e.to_string())?.0 as isize, x, y, w, h, shown);
    #[cfg(not(windows))]
    {
        let _ = (window, x, y, w, h, shown);
        Ok(())
    }
}
