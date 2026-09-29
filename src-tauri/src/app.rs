//! Tauri glue: commands, the `paginaut://` pixel protocol, and app state.

use crate::engine::{DocId, DocInfo, Engine, InsertSource, Inserted, Part, Peek, RenderReq, SearchOptions};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::http::{Request, Response, StatusCode};
use tauri::ipc::Channel;
use tauri::{Emitter, Manager, State, UriSchemeContext, UriSchemeResponder};

pub struct AppState {
    pub engine: Arc<Engine>,
    pub launch_files: Mutex<Vec<String>>,
    pub started: std::time::Instant,
}

type Res<T> = Result<T, String>;

fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> impl std::future::Future<Output = Res<T>> {
    async move { tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())? }
}

// ---------------------------------------------------------------- documents

#[tauri::command]
pub async fn open_document(state: State<'_, AppState>, path: String) -> Res<DocInfo> {
    let engine = state.engine.clone();
    blocking(move || engine.open(&path)).await
}

#[tauri::command]
pub async fn unlock_document(state: State<'_, AppState>, doc: DocId, password: String) -> Res<Option<DocInfo>> {
    let engine = state.engine.clone();
    blocking(move || engine.unlock(doc, &password)).await
}

#[tauri::command]
pub fn close_document(state: State<'_, AppState>, doc: DocId) {
    state.engine.close(doc);
}

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

#[tauri::command]
pub fn trim_document(state: State<'_, AppState>, doc: DocId) {
    state.engine.trim(doc);
}

#[tauri::command]
pub async fn page_sizes(state: State<'_, AppState>, doc: DocId) -> Res<Vec<[f32; 2]>> {
    let engine = state.engine.clone();
    blocking(move || engine.page_sizes(doc)).await
}

#[tauri::command]
pub async fn page_labels(state: State<'_, AppState>, doc: DocId) -> Res<Option<Vec<String>>> {
    let engine = state.engine.clone();
    blocking(move || engine.page_labels(doc)).await
}

#[tauri::command]
pub async fn page_label(state: State<'_, AppState>, doc: DocId, page: usize) -> Res<String> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.label(page)).await
}

#[tauri::command]
pub async fn outline(state: State<'_, AppState>, doc: DocId) -> Res<serde_json::Value> {
    let engine = state.engine.clone();
    blocking(move || {
        let json = engine.doc(doc)?.outline_json()?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn page_images(state: State<'_, AppState>, doc: DocId, page: usize) -> Res<Vec<[f32; 4]>> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.image_rects(page)).await
}

#[tauri::command]
pub async fn page_links(state: State<'_, AppState>, doc: DocId, page: usize) -> Res<serde_json::Value> {
    let engine = state.engine.clone();
    blocking(move || {
        let json = engine.doc(doc)?.links_json(page)?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    })
    .await
}

// ---------------------------------------------------------------- text

#[tauri::command]
pub async fn text_select(
    state: State<'_, AppState>,
    doc: DocId,
    page: usize,
    a: [f32; 2],
    b: [f32; 2],
    mode: i32,
) -> Res<Vec<[f32; 8]>> {
    let engine = state.engine.clone();
    blocking(move || {
        let d = engine.doc(doc)?;
        let bounds = d.bounds(page)?;
        let text = d.text(page)?;
        let t = text.lock();
        Ok(match mode {
            0 => t.highlight(&bounds, a, b),
            m => t.snap(&bounds, a, b, m).into_iter().collect(),
        })
    })
    .await
}

#[tauri::command]
pub async fn text_copy(state: State<'_, AppState>, doc: DocId, page: usize, a: [f32; 2], b: [f32; 2]) -> Res<String> {
    let engine = state.engine.clone();
    blocking(move || {
        let d = engine.doc(doc)?;
        let bounds = d.bounds(page)?;
        let text = d.text(page)?;
        let s = text.lock().copy(&bounds, a, b);
        Ok(s)
    })
    .await
}

#[tauri::command]
pub async fn page_text(state: State<'_, AppState>, doc: DocId, page: usize) -> Res<String> {
    let engine = state.engine.clone();
    blocking(move || Ok(engine.doc(doc)?.text(page)?.lock().all())).await
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TextStats {
    chars: usize,
}

#[tauri::command]
pub async fn page_text_stats(state: State<'_, AppState>, doc: DocId, page: usize) -> Res<TextStats> {
    let engine = state.engine.clone();
    blocking(move || Ok(TextStats { chars: engine.doc(doc)?.text(page)?.lock().char_count() })).await
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SearchEvent {
    Hits { page: usize, hits: Vec<Vec<f32>>, snippets: Vec<String> },
    /// Pages from..to (exclusive) have been searched.
    Scanned { from: usize, to: usize },
    Done { complete: bool, error: Option<String>, limited: bool },
}

#[tauri::command]
pub fn search(
    state: State<'_, AppState>,
    doc: DocId,
    needle: String,
    opts: Option<SearchOptions>,
    sid: u64,
    on_event: Channel<SearchEvent>,
) -> Res<()> {
    let ch = on_event.clone();
    let progress = on_event.clone();
    state.engine.search(
        doc,
        needle,
        opts.unwrap_or_default(),
        sid,
        move |page, hits| {
            let _ = ch.send(SearchEvent::Hits { page, hits: hits.quads, snippets: hits.snippets });
        },
        move |from, to| {
            let _ = progress.send(SearchEvent::Scanned { from, to });
        },
        move |end| {
            let _ = on_event.send(SearchEvent::Done { complete: end.complete, error: end.error, limited: end.limited });
        },
    )
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

/// The Windows user name: the default author of comments.
#[tauri::command]
pub fn user_name() -> String {
    std::env::var("USERNAME").unwrap_or_default()
}

#[tauri::command]
pub async fn annots(state: State<'_, AppState>, doc: DocId, page: usize) -> Res<String> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.annots_json(page)).await
}

#[tauri::command]
pub async fn doc_annots(state: State<'_, AppState>, doc: DocId) -> Res<String> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.doc_annots_json()).await
}

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
}

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
        let cert = match (opts.cert.as_str(), test_cert()?) {
            ("TEST", Some(c)) => c,
            _ => crate::dsc::Cert::find(&opts.cert)?,
        };
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
            // Room for the signature with a long certificate chain.
            max: 24 * 1024,
        };
        engine.sign(doc, target, opts.page, req, |data| cert.sign(data, hwnd))
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

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReflowEvent {
    /// One page's content as JSON (see fo_reflow_json).
    Page { page: usize, data: String },
    Done { complete: bool, error: Option<String> },
}

/// Streams the content of every page for Read and Book modes, starting at `start`.
#[tauri::command]
pub fn reflow(state: State<'_, AppState>, doc: DocId, start: usize, sid: u64, on_event: Channel<ReflowEvent>) -> Res<()> {
    let ch = on_event.clone();
    state.engine.reflow(
        doc,
        start,
        sid,
        move |page, data| {
            let _ = ch.send(ReflowEvent::Page { page, data });
        },
        move |complete, error| {
            let _ = on_event.send(ReflowEvent::Done { complete, error });
        },
    )
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
    sid: u64,
    on_event: Channel<OcrEvent>,
) -> Res<()> {
    let models = dirs.models(&langs, Quality::parse(&quality))?;
    let progress = on_event.clone();
    state.engine.ocr(
        doc,
        pages,
        models,
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

#[tauri::command]
pub fn cancel(state: State<'_, AppState>, ids: Vec<u64>) {
    for id in ids {
        state.engine.cancel(id);
    }
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
fn app_memory(sys: &mut sysinfo::System) -> u64 {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate};
    let Ok(me) = sysinfo::get_current_pid() else { return 0 };
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_memory());
    let procs = sys.processes();
    let mut total = 0;
    for (pid, p) in procs {
        let mut cur: Option<Pid> = Some(*pid);
        let mut depth = 0;
        while let Some(c) = cur {
            if c == me {
                total += p.virtual_memory();
                break;
            }
            cur = procs.get(&c).and_then(|x| x.parent());
            depth += 1;
            if depth > 8 {
                break;
            }
        }
    }
    total
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
            process_memory: app_memory(&mut sys),
            uptime_ms,
        })
    })
    .await
}

#[tauri::command]
pub fn take_launch_files(state: State<'_, AppState>) -> Vec<String> {
    std::mem::take(&mut *state.launch_files.lock())
}

pub fn files_from_args(args: &[String], cwd: Option<&str>) -> Vec<String> {
    args.iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .map(|a| {
            let p = std::path::Path::new(a);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                std::path::Path::new(cwd.unwrap_or(".")).join(p)
            }
        })
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

pub fn forward_files(app: &tauri::AppHandle, files: Vec<String>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    if !files.is_empty() {
        let _ = app.emit("open-files", files);
    }
}

// ---------------------------------------------------------------- paginaut:// protocol

fn query(uri: &str) -> HashMap<String, String> {
    uri.split_once('?')
        .map(|(_, q)| q)
        .unwrap_or("")
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn respond(responder: UriSchemeResponder, status: StatusCode, body: Vec<u8>) {
    let resp = Response::builder()
        .status(status)
        .header("Content-Type", "application/octet-stream")
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "no-store")
        .body(body)
        .unwrap();
    responder.respond(resp);
}

/// `paginaut://localhost/render?d=<doc>&p=<page>&s=<scale>&r=<rot>&e=<extras>&c=x0,y0,x1,y1&id=<rid>&q=<prio>`
/// Responds with 8-byte header (width, height LE) + RGBA pixels; 204 if cancelled.
pub fn protocol<R: tauri::Runtime>(ctx: UriSchemeContext<'_, R>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let state = ctx.app_handle().state::<AppState>();
    let uri = request.uri().to_string();
    let q = query(&uri);
    let num = |k: &str| q.get(k).and_then(|v| v.parse::<f64>().ok());
    let (Some(doc), Some(page), Some(scale)) = (num("d"), num("p"), num("s")) else {
        return respond(responder, StatusCode::BAD_REQUEST, b"bad render request".to_vec());
    };
    let clip = q.get("c").and_then(|c| {
        let v: Vec<i32> = c.split(',').filter_map(|x| x.parse().ok()).collect();
        (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
    });
    let req = RenderReq {
        page: page as usize,
        scale: scale as f32,
        rotate: num("r").unwrap_or(0.0) as i32,
        clip,
        extras: num("e").unwrap_or(1.0).clamp(0.0, 2.0) as u8,
    };
    let rid = num("id").unwrap_or(0.0) as u64;
    let prio = num("q").unwrap_or(0.0) as i32;
    state.engine.render(doc as DocId, req, rid, prio, move |res| match res {
        Ok(Some(r)) => respond(responder, StatusCode::OK, r.bytes),
        Ok(None) => respond(responder, StatusCode::NO_CONTENT, Vec::new()),
        Err(e) => respond(responder, StatusCode::INTERNAL_SERVER_ERROR, e.into_bytes()),
    });
}
