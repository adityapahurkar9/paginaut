//! Tauri glue shared by the desktop and Android apps: viewing commands, the `paginaut://` pixel protocol,
//! and app state. The desktop's editing and Windows tools are in desk.rs.

use crate::engine::{DocId, DocInfo, Engine, RenderReq, SearchOptions};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::http::{Request, Response, StatusCode};
use tauri::ipc::Channel;
use tauri::{Manager, State, UriSchemeContext, UriSchemeResponder};
#[cfg(desktop)]
use tauri::Emitter;

pub struct AppState {
    pub engine: Arc<Engine>,
    #[cfg_attr(mobile, allow(dead_code))]
    pub launch_files: Mutex<Vec<String>>,
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

/// A word (mode 3), paragraph (4) or the page (5) near a point, as a character range with its
/// quads: drawn and later copied exactly (see fo_text_snap_range).
#[derive(Serialize)]
pub struct TextRange {
    range: [i32; 2],
    quads: Vec<[f32; 8]>,
}

#[tauri::command]
pub async fn text_range(state: State<'_, AppState>, doc: DocId, page: usize, p: [f32; 2], mode: i32) -> Res<Option<TextRange>> {
    let engine = state.engine.clone();
    blocking(move || {
        let d = engine.doc(doc)?;
        let bounds = d.bounds(page)?;
        let text = d.text(page)?;
        let t = text.lock();
        Ok(t.snap_range(&bounds, p, mode).map(|[from, to]| TextRange { range: [from, to], quads: t.range_quads(&bounds, from, to) }))
    })
    .await
}

#[tauri::command]
pub async fn text_range_copy(state: State<'_, AppState>, doc: DocId, page: usize, from: i32, to: i32) -> Res<String> {
    let engine = state.engine.clone();
    blocking(move || {
        let d = engine.doc(doc)?;
        let text = d.text(page)?;
        let s = text.lock().range_copy(from, to);
        Ok(s)
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
#[tauri::command]
pub fn take_launch_files(state: State<'_, AppState>) -> Vec<String> {
    std::mem::take(&mut *state.launch_files.lock())
}

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

#[cfg(desktop)]
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
