//! Tauri glue shared by the desktop and Android apps: viewing commands, the `paginaut://` pixel protocol,
//! and app state. The desktop's editing and Windows tools are in desk.rs.

use crate::engine::{DocId, DocInfo, Engine, RenderReq, SearchOptions};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::http::{Request, Response, StatusCode};
use tauri::ipc::Channel;
use tauri::{Manager, State, UriSchemeContext, UriSchemeResponder};

pub struct AppState {
    pub engine: Arc<Engine>,
    #[cfg_attr(mobile, allow(dead_code))]
    pub started: std::time::Instant,
}

pub(crate) type Res<T> = Result<T, String>;

pub(crate) fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> impl std::future::Future<Output = Res<T>> {
    async move { tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())? }
}

// ---------------------------------------------------------------- documents

/// Opens a file; an Office file through a PDF made with `engine` ("msoffice", "libreoffice", "builtin"),
/// or the preferred converter.
#[tauri::command]
pub async fn open_document(state: State<'_, AppState>, path: String, engine: Option<String>) -> Res<DocInfo> {
    let engine_ = state.engine.clone();
    blocking(move || engine_.open_with(&path, engine.as_deref())).await
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
pub async fn security(state: State<'_, AppState>, doc: DocId) -> Res<crate::engine::Security> {
    let engine = state.engine.clone();
    blocking(move || engine.security(doc)).await
}

/// Saves a repaired e-book (an EPUB made from a PDF, shown as that PDF) at `target`: as the PDF (`pdf`),
/// or as an ordinary, flowing EPUB.
#[tauri::command]
pub async fn save_repaired(state: State<'_, AppState>, doc: DocId, target: String, pdf: bool) -> Res<()> {
    let engine = state.engine.clone();
    blocking(move || engine.save_repaired(doc, &target, pdf)).await
}

#[tauri::command]
pub async fn properties(state: State<'_, AppState>, doc: DocId) -> Res<crate::engine::Properties> {
    let engine = state.engine.clone();
    blocking(move || Ok(engine.doc(doc)?.properties())).await
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
    blocking(move || outline_of(&engine, doc)).await
}

// The bodies of commands that do more than one call, shared with the Android engine's process (engine_link.rs).

pub(crate) fn outline_of(engine: &Engine, doc: DocId) -> Res<serde_json::Value> {
    let json = engine.doc(doc)?.outline_json()?;
    serde_json::from_str(&json).map_err(|e| e.to_string())
}

pub(crate) fn page_links_of(engine: &Engine, doc: DocId, page: usize) -> Res<serde_json::Value> {
    let json = engine.doc(doc)?.links_json(page)?;
    serde_json::from_str(&json).map_err(|e| e.to_string())
}

pub(crate) fn text_select_of(engine: &Engine, doc: DocId, page: usize, a: [f32; 2], b: [f32; 2], mode: i32) -> Res<Vec<[f32; 8]>> {
    let d = engine.doc(doc)?;
    let bounds = d.bounds(page)?;
    let text = d.text(page)?;
    let t = text.lock();
    Ok(match mode {
        0 => t.highlight(&bounds, a, b),
        m => t.snap(&bounds, a, b, m).into_iter().collect(),
    })
}

pub(crate) fn text_range_of(engine: &Engine, doc: DocId, page: usize, p: [f32; 2], mode: i32) -> Res<Option<TextRange>> {
    let d = engine.doc(doc)?;
    let bounds = d.bounds(page)?;
    let text = d.text(page)?;
    let t = text.lock();
    Ok(t.snap_range(&bounds, p, mode).map(|[from, to]| TextRange { range: [from, to], quads: t.range_quads(&bounds, from, to) }))
}

pub(crate) fn text_copy_of(engine: &Engine, doc: DocId, page: usize, a: [f32; 2], b: [f32; 2]) -> Res<String> {
    let d = engine.doc(doc)?;
    let bounds = d.bounds(page)?;
    let text = d.text(page)?;
    let s = text.lock().copy(&bounds, a, b);
    Ok(s)
}

pub(crate) fn text_range_copy_of(engine: &Engine, doc: DocId, page: usize, from: i32, to: i32) -> Res<String> {
    let text = engine.doc(doc)?.text(page)?;
    let s = text.lock().range_copy(from, to);
    Ok(s)
}

/// A book's title, author and pages, and its first page as a JPEG (None without one): the shelf's cover.
pub(crate) fn book_cover_of(engine: &Engine, path: &str) -> Res<(serde_json::Value, Option<Vec<u8>>)> {
    // A book drawn again from its EPUB (pdfhtml.rs): only its first page is drawn for this.
    let preview = crate::engine::book_preview(path);
    let opened = engine.open(preview.as_ref().map_or(path, |p| p.pdf.as_str()));
    let info = match opened {
        Ok(info) => info,
        Err(e) => {
            if let Some(p) = &preview {
                crate::engine::sandbox::release(&p.pdf);
            }
            return Err(e);
        }
    };
    let jpg = (|| -> Result<Option<Vec<u8>>, String> {
        if info.needs_password || info.page_count == 0 {
            return Ok(None);
        }
        // About 360 px wide: sharp in a three-across grid on a phone, and in the Windows shelf's tiles.
        let dpi = (72.0 * 360.0 / info.first_size[0].max(1.0)).clamp(12.0, 300.0);
        Ok(Some(engine.doc(info.doc_id)?.page_image(0, 0, "jpg", dpi)?))
    })()
    .unwrap_or(None);
    engine.close(info.doc_id);
    let (title, author, pages) = match preview {
        Some(p) => {
            crate::engine::sandbox::release(&p.pdf);
            (p.title, p.author, p.pages)
        }
        None => (info.title, info.author, info.page_count),
    };
    Ok((serde_json::json!({ "title": title, "author": author, "pages": pages, "cover": jpg.is_some() }), jpg))
}

#[tauri::command]
pub async fn page_images(state: State<'_, AppState>, doc: DocId, page: usize) -> Res<Vec<[f32; 4]>> {
    let engine = state.engine.clone();
    blocking(move || engine.doc(doc)?.image_rects(page)).await
}

#[tauri::command]
pub async fn page_links(state: State<'_, AppState>, doc: DocId, page: usize) -> Res<serde_json::Value> {
    let engine = state.engine.clone();
    blocking(move || page_links_of(&engine, doc, page)).await
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
    blocking(move || text_select_of(&engine, doc, page, a, b, mode)).await
}

/// A word (mode 3), paragraph (4) or the page (5) near a point, as a character range with its
/// quads: drawn and later copied exactly (see fo_text_snap_range).
#[derive(Serialize, serde::Deserialize)]
pub struct TextRange {
    range: [i32; 2],
    quads: Vec<[f32; 8]>,
}

#[tauri::command]
pub async fn text_range(state: State<'_, AppState>, doc: DocId, page: usize, p: [f32; 2], mode: i32) -> Res<Option<TextRange>> {
    let engine = state.engine.clone();
    blocking(move || text_range_of(&engine, doc, page, p, mode)).await
}

#[tauri::command]
pub async fn text_range_copy(state: State<'_, AppState>, doc: DocId, page: usize, from: i32, to: i32) -> Res<String> {
    let engine = state.engine.clone();
    blocking(move || text_range_copy_of(&engine, doc, page, from, to)).await
}

#[tauri::command]
pub async fn text_copy(state: State<'_, AppState>, doc: DocId, page: usize, a: [f32; 2], b: [f32; 2]) -> Res<String> {
    let engine = state.engine.clone();
    blocking(move || text_copy_of(&engine, doc, page, a, b)).await
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

// ---------------------------------------------------------------- annotations

/// The user's name on this PC (Windows' USERNAME, Linux's USER): the default author of comments and marks.
#[tauri::command]
pub fn user_name() -> String {
    ["USERNAME", "USER", "LOGNAME"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty())).unwrap_or_default()
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

// ---------------------------------------------------------------- digital signatures

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

#[tauri::command]
pub fn cancel(state: State<'_, AppState>, ids: Vec<u64>) {
    for id in ids {
        state.engine.cancel(id);
    }
}

// ---------------------------------------------------------------- app

#[cfg(desktop)]
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

/// A book's title, author and pages, and its first page as a cover (`covers/<fp>.jpg` in the
/// cache, served as paginaut://…/cover?f=<fp>): for the shelf of e-books and comics, on Android and Windows.
#[tauri::command]
pub async fn book_cover(app: tauri::AppHandle, state: State<'_, AppState>, path: String, fp: String) -> Res<serde_json::Value> {
    if fp.is_empty() || !fp.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("no fingerprint".into());
    }
    let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("covers");
    let engine = state.engine.clone();
    blocking(move || {
        let (meta, jpg) = book_cover_of(&engine, &path)?;
        Ok(save_cover(meta, jpg, &dir, &fp))
    })
    .await
}

/// Keeps a cover as `covers/<fp>.jpg` (served by the protocol); `meta.cover` says whether there is one.
pub(crate) fn save_cover(mut meta: serde_json::Value, jpg: Option<Vec<u8>>, dir: &std::path::Path, fp: &str) -> serde_json::Value {
    let saved = jpg.is_some_and(|b| std::fs::create_dir_all(dir).and_then(|_| std::fs::write(dir.join(format!("{fp}.jpg")), b)).is_ok());
    meta["cover"] = serde_json::json!(saved);
    meta
}

// ---------------------------------------------------------------- paginaut:// protocol

pub(crate) fn query(uri: &str) -> HashMap<String, String> {
    uri.split_once('?')
        .map(|(_, q)| q)
        .unwrap_or("")
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

pub(crate) fn respond(responder: UriSchemeResponder, status: StatusCode, body: Vec<u8>) {
    let resp = Response::builder()
        .status(status)
        .header("Content-Type", "application/octet-stream")
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "no-store")
        .body(body)
        .unwrap();
    responder.respond(resp);
}

/// A book's cover for the shelf (book_cover): `/cover?f=<fingerprint>`, covers/<fingerprint>.jpg in `cache`.
pub(crate) fn cover(responder: UriSchemeResponder, cache: Option<std::path::PathBuf>, q: &HashMap<String, String>) {
    let f = q.get("f").filter(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_hexdigit()));
    match f.zip(cache).and_then(|(f, dir)| std::fs::read(dir.join("covers").join(format!("{f}.jpg"))).ok()) {
        Some(b) => {
            let resp = Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "image/jpeg")
                .header("Access-Control-Allow-Origin", "*")
                .body(b)
                .unwrap();
            responder.respond(resp)
        }
        None => respond(responder, StatusCode::NOT_FOUND, Vec::new()),
    }
}

/// A render request's document, page, scale, turn, part, extras, id and priority (`/render?d=&p=&s=…`).
pub(crate) fn render_request(q: &HashMap<String, String>) -> Option<(DocId, RenderReq, u64, i32)> {
    let num = |k: &str| q.get(k).and_then(|v| v.parse::<f64>().ok());
    let (doc, page, scale) = (num("d")?, num("p")?, num("s")?);
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
    Some((doc as DocId, req, num("id").unwrap_or(0.0) as u64, num("q").unwrap_or(0.0) as i32))
}

/// `paginaut://localhost/render?d=<doc>&p=<page>&s=<scale>&r=<rot>&e=<extras>&c=x0,y0,x1,y1&id=<rid>&q=<prio>`
/// Responds with 8-byte header (width, height LE) + RGBA pixels; 204 if cancelled.
pub fn protocol<R: tauri::Runtime>(ctx: UriSchemeContext<'_, R>, request: Request<Vec<u8>>, responder: UriSchemeResponder) {
    let state = ctx.app_handle().state::<AppState>();
    let uri = request.uri().to_string();
    let q = query(&uri);
    if request.uri().path() == "/cover" {
        return cover(responder, ctx.app_handle().path().app_cache_dir().ok(), &q);
    }
    let Some((doc, req, rid, prio)) = render_request(&q) else {
        return respond(responder, StatusCode::BAD_REQUEST, b"bad render request".to_vec());
    };
    state.engine.render(doc, req, rid, prio, move |res| match res {
        Ok(Some(r)) => respond(responder, StatusCode::OK, r.bytes),
        Ok(None) => respond(responder, StatusCode::NO_CONTENT, Vec::new()),
        Err(e) => respond(responder, StatusCode::INTERNAL_SERVER_ERROR, e.into_bytes()),
    });
}
