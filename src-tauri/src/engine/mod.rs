//! Document engine: owns open documents, caches, and the worker pool.

pub mod ffi;
pub mod mupdf;
pub mod ocr;
pub mod pool;

use mupdf::{Document, List, Page, RenderOutcome, Text};
use parking_lot::{Mutex, RwLock};
use pool::{Ctl, Pool};
use serde::{Deserialize, Serialize};
use once_cell::sync::Lazy;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

pub type DocId = u64;

/// Memory budget for cached display lists per document. A scanned page's list
/// holds its compressed image, so the entry count adapts to bytes per page.
const LIST_BUDGET: u64 = 96 << 20;

/// Page size used to lay out reflowable formats (EPUB, FB2, HTML…), in points.
const REFLOW: (f32, f32, f32) = (450.0, 640.0, 11.0);

/// Small insertion-ordered LRU; capacities here are tiny so linear scans are fine.
struct Lru<K: PartialEq + Copy, V> {
    cap: usize,
    items: VecDeque<(K, V)>,
}

impl<K: PartialEq + Copy, V: Clone> Lru<K, V> {
    fn new(cap: usize) -> Self {
        Lru { cap, items: VecDeque::new() }
    }
    fn get(&mut self, k: K) -> Option<V> {
        let i = self.items.iter().position(|(key, _)| *key == k)?;
        let item = self.items.remove(i)?;
        let v = item.1.clone();
        self.items.push_back(item);
        Some(v)
    }
    fn put(&mut self, k: K, v: V) {
        if let Some(i) = self.items.iter().position(|(key, _)| *key == k) {
            self.items.remove(i);
        }
        self.items.push_back((k, v));
        while self.items.len() > self.cap {
            self.items.pop_front();
        }
    }
    fn clear(&mut self) {
        self.items.clear();
    }
    fn set_cap(&mut self, cap: usize) {
        self.cap = cap.max(1);
        while self.items.len() > self.cap {
            self.items.pop_front();
        }
    }
}

struct DocInner {
    doc: Document,
    pages: Lru<usize, Arc<Page>>,
}

pub struct Doc {
    pub id: DocId,
    /// Changes on "Save as".
    path: RwLock<String>,
    inner: Mutex<DocInner>,
    page_count: AtomicUsize,
    bounds: RwLock<Vec<Option<[f32; 4]>>>,
    lists: Mutex<Lru<(usize, u8), Arc<List>>>,
    texts: Mutex<Lru<usize, Arc<Mutex<Text>>>>,
    password: Mutex<Option<String>>,
    /// Bumped by each background job; lets a delayed release see whether newer work started.
    bg_gen: AtomicU64,
    /// Bumped by each form/annotation edit (cached display lists from before are stale).
    edit_rev: AtomicU64,
    /// Page content was changed in memory (recognised text): background jobs must
    /// read this document, not private copies of the file on disk. Cleared on saving.
    content_edited: AtomicBool,
}

/// Where a background job reads pages from: a private copy of the file (fast,
/// parallel), or the document in memory when its pages have unsaved changes.
pub enum PageSrc<'a> {
    Private(&'a Document),
    Shared(&'a Doc),
}

impl PageSrc<'_> {
    /// A fresh display list of the page's contents, and the page's bounds.
    pub fn list(&self, i: usize) -> Result<(List, [f32; 4]), String> {
        match self {
            PageSrc::Private(doc) => {
                let p = doc.load_page(i)?;
                Ok((p.display_list(0)?, p.bounds()?))
            }
            PageSrc::Shared(d) => Ok((d.fresh_list(i)?, d.bounds(i)?)),
        }
    }
}

/// What happens to private document copies once a background job finishes.
/// Default `now`: each copy caches whatever pages its thread touched, so kept
/// copies grow toward one full document each; reopening costs only ~45 ms per
/// search. PAGINAUT_PRIVATE_POLICY=keep|now|idle overrides it for benchmarking.
#[derive(Clone, Copy, Debug)]
enum PrivatePolicy {
    Keep,
    Now,
    Idle(u64),
}

/// Threads a single search may use (PAGINAUT_SEARCH_THREADS overrides; default 8).
fn search_threads() -> usize {
    std::env::var("PAGINAUT_SEARCH_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(8)
}

fn private_policy() -> PrivatePolicy {
    match std::env::var("PAGINAUT_PRIVATE_POLICY").as_deref() {
        Ok("keep") => PrivatePolicy::Keep,
        Ok("idle") => PrivatePolicy::Idle(30),
        _ => PrivatePolicy::Now,
    }
}

// Background jobs (search, later OCR and conversion) work on private copies of
// documents: one per worker thread, opened with that thread's private MuPDF
// context, so they run in parallel without sharing any MuPDF locks.
thread_local! {
    static PRIVATE_DOCS: RefCell<HashMap<DocId, Document>> = RefCell::new(HashMap::new());
}
/// Documents whose private copies may stay open on worker threads.
static PRIVATE_ALLOWED: Lazy<RwLock<HashSet<DocId>>> = Lazy::new(|| RwLock::new(HashSet::new()));

/// Drops this thread's private copies of closed or backgrounded documents
/// (runs on every worker when the pool's maintenance epoch changes).
pub fn purge_private_docs() {
    let allowed = PRIVATE_ALLOWED.read();
    let dropped = PRIVATE_DOCS.with(|m| {
        let mut m = m.borrow_mut();
        let before = m.len();
        m.retain(|id, _| allowed.contains(id));
        before - m.len()
    });
    drop(allowed);
    if dropped > 0 {
        mupdf::empty_private_store();
        mupdf::release_heap();
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DocInfo {
    pub doc_id: DocId,
    pub path: String,
    pub page_count: usize,
    pub needs_password: bool,
    pub is_pdf: bool,
    pub reflowable: bool,
    pub format: String,
    pub title: String,
    pub author: String,
    pub file_size: u64,
    /// Width/height of page 0 in points, after its own rotation.
    pub first_size: [f32; 2],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormInfo {
    pub fields: usize,
    /// The form is (also) an XFA form, which can't be filled here.
    pub xfa: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormChange {
    /// The value was taken (a form can reject values), or the undo/redo happened.
    pub accepted: bool,
    /// The document now differs from the file on disk.
    pub changed: bool,
}

/// Search options from the UI.
#[derive(Deserialize, Clone, Copy, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchOptions {
    pub match_case: bool,
    /// The needle is a regular expression (whole-word search is sent as \b…\b).
    pub regex: bool,
    /// Include the text of the line around each hit (for result lists).
    pub snippets: bool,
    /// Page to start from (the one you're looking at); the search wraps around.
    pub start: usize,
}

/// Searches stop after this many hits: a pattern like "." would otherwise
/// match every character in the document.
pub const MAX_SEARCH_HITS: usize = 20_000;

/// How a search ended.
pub struct SearchEnd {
    pub complete: bool,
    pub error: Option<String>,
    /// Stopped at MAX_SEARCH_HITS.
    pub limited: bool,
}

impl SearchOptions {
    fn flags(&self) -> i32 {
        (self.match_case as i32) | ((self.regex as i32) << 1)
    }
}

#[derive(Deserialize)]
struct FoundHit {
    q: Vec<f32>,
    #[serde(default)]
    s: Option<String>,
}

/// Hits on one page: quads per hit (8 floats per quad) and, if asked, snippets.
pub struct PageHits {
    pub quads: Vec<Vec<f32>>,
    pub snippets: Vec<String>,
}

pub struct RenderReq {
    pub page: usize,
    pub scale: f32,
    pub rotate: i32,
    pub clip: Option<[i32; 4]>,
    /// 0 contents only, 1 with annotations and fields, 2 without fillable fields.
    pub extras: u8,
}

/// Rendered pixels: 8-byte header (width, height as u32 LE) followed by RGBA.
pub struct Rendered {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Doc {
    /// Runs `f` on this worker thread's private copy of the document, opening
    /// it on first use. Everything `f` creates must stay inside `f`.
    pub fn with_private<R>(&self, f: impl FnOnce(&Document) -> Result<R, String>) -> Result<R, String> {
        PRIVATE_ALLOWED.write().insert(self.id);
        PRIVATE_DOCS.with(|m| {
            let mut m = m.borrow_mut();
            if !m.contains_key(&self.id) {
                let d = Document::open_private(&self.path())?;
                if d.needs_password() {
                    if let Some(pw) = self.password.lock().as_deref() {
                        d.authenticate(pw);
                    }
                }
                if d.is_reflowable() {
                    d.layout(REFLOW.0, REFLOW.1, REFLOW.2)?;
                }
                m.insert(self.id, d);
            }
            f(&m[&self.id])
        })
    }

    pub fn path(&self) -> String {
        self.path.read().clone()
    }

    fn page(&self, inner: &mut DocInner, index: usize) -> Result<Arc<Page>, String> {
        if let Some(p) = inner.pages.get(index) {
            return Ok(p);
        }
        let p = Arc::new(inner.doc.load_page(index)?);
        inner.pages.put(index, p.clone());
        Ok(p)
    }

    pub fn page_count(&self) -> usize {
        self.page_count.load(Ordering::Relaxed)
    }

    fn check(&self, index: usize) -> Result<(), String> {
        if index >= self.page_count() {
            Err(format!("page {index} out of range"))
        } else {
            Ok(())
        }
    }

    pub fn bounds(&self, index: usize) -> Result<[f32; 4], String> {
        self.check(index)?;
        if let Some(Some(b)) = self.bounds.read().get(index) {
            return Ok(*b);
        }
        let b = {
            let mut inner = self.inner.lock();
            let page = self.page(&mut inner, index)?;
            page.bounds()?
        };
        if let Some(slot) = self.bounds.write().get_mut(index) {
            *slot = Some(b);
        }
        Ok(b)
    }

    pub fn list(&self, index: usize, extras: u8) -> Result<Arc<List>, String> {
        self.check(index)?;
        if let Some(l) = self.lists.lock().get((index, extras)) {
            return Ok(l);
        }
        let (list, rev) = {
            let mut inner = self.inner.lock();
            let page = self.page(&mut inner, index)?;
            (Arc::new(page.display_list(extras)?), self.edit_rev.load(Ordering::Acquire))
        };
        // Don't keep a list made just before an edit changed the page.
        if self.edit_rev.load(Ordering::Acquire) == rev {
            self.lists.lock().put((index, extras), list.clone());
        }
        Ok(list)
    }

    /// A display list of the page's contents as they are now in memory, without
    /// touching the caches.
    fn fresh_list(&self, index: usize) -> Result<List, String> {
        let mut inner = self.inner.lock();
        let page = self.page(&mut inner, index)?;
        page.display_list(0)
    }

    // ------------------------------------------------------------ forms

    /// Fields and annotations were edited: everything drawn from the pages is stale.
    fn edited(&self) {
        self.edit_rev.fetch_add(1, Ordering::AcqRel);
        self.lists.lock().clear();
        self.texts.lock().clear();
    }

    pub fn forms_init(&self) -> Result<FormInfo, String> {
        let inner = self.inner.lock();
        if !inner.doc.is_pdf() {
            return Ok(FormInfo { fields: 0, xfa: false });
        }
        let (fields, xfa) = inner.doc.forms_init()?;
        Ok(FormInfo { fields, xfa })
    }

    pub fn widgets_json(&self, index: usize) -> Result<String, String> {
        let b = self.bounds(index)?;
        let mut inner = self.inner.lock();
        let page = self.page(&mut inner, index)?;
        page.widgets_json(&b)
    }

    pub fn widget_set(&self, index: usize, num: i32, value: &str) -> Result<FormChange, String> {
        let mut inner = self.inner.lock();
        let page = self.page(&mut inner, index)?;
        let accepted = page.widget_set(num, value)?;
        self.edited();
        Ok(FormChange { accepted, changed: inner.doc.has_changes() })
    }

    pub fn widget_toggle(&self, index: usize, num: i32) -> Result<FormChange, String> {
        let mut inner = self.inner.lock();
        let page = self.page(&mut inner, index)?;
        page.widget_toggle(num)?;
        self.edited();
        Ok(FormChange { accepted: true, changed: inner.doc.has_changes() })
    }

    pub fn undo(&self, redo: bool) -> Result<FormChange, String> {
        let inner = self.inner.lock();
        let done = inner.doc.undo(redo)?;
        if done {
            self.edited();
        }
        Ok(FormChange { accepted: done, changed: inner.doc.has_changes() })
    }

    pub fn reset_form(&self) -> Result<FormChange, String> {
        let inner = self.inner.lock();
        inner.doc.reset_form()?;
        self.edited();
        Ok(FormChange { accepted: true, changed: inner.doc.has_changes() })
    }

    pub fn has_changes(&self) -> bool {
        self.inner.lock().doc.has_changes()
    }

    // ------------------------------------------------------------ annotations

    pub fn annots_json(&self, index: usize) -> Result<String, String> {
        let b = self.bounds(index)?;
        let mut inner = self.inner.lock();
        let page = self.page(&mut inner, index)?;
        page.annots_json(&b)
    }

    pub fn doc_annots_json(&self) -> Result<String, String> {
        let inner = self.inner.lock();
        if !inner.doc.is_pdf() {
            return Ok("[]".into());
        }
        inner.doc.doc_annots_json()
    }

    /// Creates (num = None), changes, or deletes an annotation, as one undo step.
    /// Coordinates in `edit` are relative to the page's top-left (as the app uses them).
    pub fn annot_edit(&self, index: usize, num: Option<i32>, kind: Option<&str>, mut edit: mupdf::AnnotEdit, delete: bool, author: &str) -> Result<(i32, FormChange), String> {
        let [ox, oy, _, _] = self.bounds(index)?;
        let shift = |v: &mut [f32]| {
            for (i, x) in v.iter_mut().enumerate() {
                *x += if i % 2 == 0 { ox } else { oy };
            }
        };
        if let Some(r) = edit.rect.as_mut() {
            shift(r);
        }
        if let Some(q) = edit.quads.as_mut() {
            shift(q);
        }
        if let Some(l) = edit.line.as_mut() {
            shift(l);
        }
        if let Some(ink) = edit.ink.as_mut() {
            for s in ink.iter_mut() {
                shift(s);
            }
        }
        let mut inner = self.inner.lock();
        let page = self.page(&mut inner, index)?;
        let name = if delete { "Delete annotation" } else if num.is_none() { "Add annotation" } else { "Change annotation" };
        inner.doc.begin_op(name);
        let res = (|| -> Result<i32, String> {
            if delete {
                let n = num.ok_or("which annotation?")?;
                page.annot_delete(n)?;
                return Ok(n);
            }
            let n = match num {
                Some(n) => n,
                None => page.annot_create(kind.ok_or("annotation type missing")?, author)?,
            };
            page.annot_apply(n, &edit)?;
            Ok(n)
        })();
        inner.doc.end_op();
        let n = res?;
        self.edited();
        Ok((n, FormChange { accepted: true, changed: inner.doc.has_changes() }))
    }

    pub fn text(&self, index: usize) -> Result<Arc<Mutex<Text>>, String> {
        if let Some(t) = self.texts.lock().get(index) {
            return Ok(t);
        }
        let t = Arc::new(Mutex::new(self.list(index, 0)?.text()?));
        self.texts.lock().put(index, t.clone());
        Ok(t)
    }

    pub fn image_rects(&self, index: usize) -> Result<Vec<[f32; 4]>, String> {
        let b = self.bounds(index)?;
        self.list(index, 0)?.image_rects(&b)
    }

    pub fn links_json(&self, index: usize) -> Result<String, String> {
        self.check(index)?;
        let mut inner = self.inner.lock();
        let page = self.page(&mut inner, index)?;
        inner.doc.links_json(&page)
    }

    pub fn outline_json(&self) -> Result<String, String> {
        self.inner.lock().doc.outline_json()
    }

    pub fn label(&self, index: usize) -> Result<String, String> {
        self.check(index)?;
        let mut inner = self.inner.lock();
        let page = self.page(&mut inner, index)?;
        Ok(page.label())
    }

    fn info(&self) -> DocInfo {
        let inner = self.inner.lock();
        let doc = &inner.doc;
        let needs_password = doc.needs_password();
        let path = self.path();
        let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let mut info = DocInfo {
            doc_id: self.id,
            path,
            page_count: self.page_count(),
            needs_password,
            is_pdf: doc.is_pdf(),
            reflowable: false,
            format: String::new(),
            title: String::new(),
            author: String::new(),
            file_size,
            first_size: [612.0, 792.0],
        };
        if needs_password {
            return info;
        }
        info.reflowable = doc.is_reflowable();
        info.format = doc.metadata("format").unwrap_or_default();
        info.title = doc.metadata("info:Title").unwrap_or_default();
        info.author = doc.metadata("info:Author").unwrap_or_default();
        drop(inner);
        if let Ok(b) = self.bounds(0) {
            info.first_size = [b[2] - b[0], b[3] - b[1]];
        }
        info
    }

    /// Runs after the document is readable (opened or unlocked).
    fn prepare(&self) -> Result<(), String> {
        let inner = self.inner.lock();
        if inner.doc.is_reflowable() {
            inner.doc.layout(REFLOW.0, REFLOW.1, REFLOW.2)?;
        }
        let n = inner.doc.page_count()?;
        self.page_count.store(n, Ordering::Relaxed);
        *self.bounds.write() = vec![None; n];
        let file = std::fs::metadata(self.path()).map(|m| m.len()).unwrap_or(0);
        let per_page = (file / n.max(1) as u64).max(1);
        self.lists.lock().set_cap((LIST_BUDGET / per_page).clamp(6, 96) as usize);
        Ok(())
    }
}

pub struct Engine {
    pool: Pool,
    docs: RwLock<HashMap<DocId, Arc<Doc>>>,
    next_id: AtomicU64,
}

impl Engine {
    pub fn new(workers: usize, store_bytes: usize) -> Result<Arc<Self>, String> {
        mupdf::init(store_bytes)?;
        Ok(Arc::new(Engine { pool: Pool::new(workers, Arc::new(purge_private_docs)), docs: RwLock::new(HashMap::new()), next_id: AtomicU64::new(1) }))
    }

    pub fn workers(&self) -> usize {
        self.pool.workers()
    }

    /// Makes workers drop their private copies of one document.
    pub fn release_private_doc(&self, id: DocId) {
        PRIVATE_ALLOWED.write().remove(&id);
        self.pool.maintain();
    }

    /// Applies the release policy after a background job on `d` finished.
    fn after_background(self: &Arc<Self>, d: &Arc<Doc>, gen: u64) {
        match private_policy() {
            PrivatePolicy::Keep => {}
            PrivatePolicy::Now => self.release_private_doc(d.id),
            PrivatePolicy::Idle(secs) => {
                let (engine, doc) = (Arc::downgrade(self), Arc::downgrade(d));
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(secs));
                    if let (Some(e), Some(d)) = (engine.upgrade(), doc.upgrade()) {
                        if d.bg_gen.load(Ordering::Acquire) == gen {
                            e.release_private_doc(d.id);
                        }
                    }
                });
            }
        }
    }

    /// Makes workers release private copies of this engine's documents.
    fn release_private(&self) {
        let ids: Vec<DocId> = self.docs.read().keys().copied().collect();
        let mut allowed = PRIVATE_ALLOWED.write();
        for id in ids {
            allowed.remove(&id);
        }
        drop(allowed);
        self.pool.maintain();
    }

    pub fn load(&self) -> (usize, usize) {
        (self.pool.busy(), self.pool.queued())
    }

    pub fn doc(&self, id: DocId) -> Result<Arc<Doc>, String> {
        self.docs.read().get(&id).cloned().ok_or_else(|| format!("document {id} is not open"))
    }

    pub fn open(&self, path: &str) -> Result<DocInfo, String> {
        let doc = Document::open(path)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let d = Arc::new(Doc {
            id,
            path: RwLock::new(path.to_string()),
            inner: Mutex::new(DocInner { doc, pages: Lru::new(32) }),
            page_count: AtomicUsize::new(0),
            bounds: RwLock::new(Vec::new()),
            lists: Mutex::new(Lru::new(96)),
            texts: Mutex::new(Lru::new(24)),
            password: Mutex::new(None),
            bg_gen: AtomicU64::new(0),
            edit_rev: AtomicU64::new(0),
            content_edited: AtomicBool::new(false),
        });
        let locked = d.inner.lock().doc.needs_password();
        if !locked {
            d.prepare()?;
        }
        self.docs.write().insert(id, d.clone());
        Ok(d.info())
    }

    pub fn unlock(&self, id: DocId, password: &str) -> Result<Option<DocInfo>, String> {
        let d = self.doc(id)?;
        if !d.inner.lock().doc.authenticate(password) {
            return Ok(None);
        }
        *d.password.lock() = Some(password.to_string());
        d.prepare()?;
        Ok(Some(d.info()))
    }

    pub fn close(&self, id: DocId) {
        if let Some(d) = self.docs.write().remove(&id) {
            d.lists.lock().clear();
            d.texts.lock().clear();
            d.inner.lock().pages.clear();
        }
        PRIVATE_ALLOWED.write().remove(&id);
        self.pool.maintain();
        if self.docs.read().is_empty() {
            mupdf::shrink_store(100);
        }
    }

    /// Drops per-document caches (display lists, text, pages) for a background tab.
    pub fn trim(&self, id: DocId) {
        if let Ok(d) = self.doc(id) {
            d.lists.lock().clear();
            d.texts.lock().clear();
            d.inner.lock().pages.clear();
        }
        PRIVATE_ALLOWED.write().remove(&id);
        self.pool.maintain();
    }

    /// Saves page edits (rotation per original page, new page order).
    ///
    /// - PDF, same file: appends only the changes (incremental save), so even
    ///   a 900 MB scan saves in a moment and the original bytes are untouched.
    ///   Files MuPDF had to repair can't be appended to; those are rewritten to
    ///   a temporary file first and then copied over the original.
    /// - PDF, new file ("Save as"): full, clean rewrite without deleted pages.
    /// - Other formats (EPUB, images, XPS…): converted to a new PDF.
    ///
    /// Afterwards the document is reopened from disk, so what you see is what was saved.
    pub fn save(&self, id: DocId, order: Vec<usize>, rot: Vec<i32>, target: Option<String>) -> Result<DocInfo, String> {
        let d = self.doc(id)?;
        self.release_private_doc(id);
        let cur = d.path();
        let out = target.unwrap_or_else(|| cur.clone());
        let in_place = same_file(&out, &cur);
        let tmp = format!("{cur}.paginaut-tmp");
        {
            let mut inner = d.inner.lock();
            inner.pages.clear();
            d.lists.lock().clear();
            d.texts.lock().clear();
            let res = (|| -> Result<(), String> {
                if inner.doc.is_pdf() {
                    inner.doc.apply_page_edits(&rot, &order)?;
                    if in_place && inner.doc.can_save_incrementally() {
                        inner.doc.save_pdf(&cur, true)
                    } else if in_place {
                        inner.doc.save_pdf(&tmp, false)?;
                        // Switch to the new file so ours no longer holds the original open.
                        inner.doc = Document::open(&tmp)?;
                        copy_with_retry(&tmp, &cur)
                    } else {
                        inner.doc.save_pdf(&out, false)
                    }
                } else if in_place {
                    Err("This kind of file can't be saved in place. Use Save as to make a PDF.".into())
                } else {
                    inner.doc.convert_to_pdf(&out, &order, &rot)
                }
            })();
            // Reopen from disk: on success the saved file, on failure the untouched original.
            let reopen = if res.is_ok() { out.clone() } else { cur.clone() };
            let fresh = Document::open(&reopen)?;
            if fresh.needs_password() {
                if let Some(pw) = d.password.lock().as_deref() {
                    fresh.authenticate(pw);
                }
            }
            inner.doc = fresh;
            drop(inner);
            // The temporary copy (repaired files only) is closed now and can go.
            let _ = std::fs::remove_file(&tmp);
            if let Err(e) = res {
                d.prepare()?;
                return Err(e);
            }
        }
        *d.path.write() = out;
        d.content_edited.store(false, Ordering::Release);
        d.prepare()?;
        Ok(d.info())
    }

    /// Signs the document with a digital signature and saves it: in place, or
    /// as `target` (a copy of the file with the signature appended). The
    /// document must have no unsaved changes, so the signature covers exactly
    /// what's on disk. `sign` makes the CMS blob for the bytes MuPDF hands it.
    /// `req.rect` is relative to the page origin, like annotations.
    pub fn sign(
        &self,
        id: DocId,
        target: Option<String>,
        index: usize,
        mut req: mupdf::SignRequest,
        sign: impl FnMut(&[u8]) -> Result<Vec<u8>, String>,
    ) -> Result<DocInfo, String> {
        let d = self.doc(id)?;
        self.release_private_doc(id);
        let cur = d.path();
        let out = target.unwrap_or_else(|| cur.clone());
        let in_place = same_file(&out, &cur);
        {
            let inner = d.inner.lock();
            if !inner.doc.is_pdf() {
                return Err("Only PDFs can be signed. Save this file as a PDF first (Save as).".into());
            }
            if inner.doc.has_changes() {
                return Err("Save your changes first: a signature covers the document as it is saved.".into());
            }
            if !inner.doc.can_save_incrementally() {
                return Err("This file was damaged and repaired when it was opened, so it can't be signed as it is. Save a copy (Save as), open the copy and sign that.".into());
            }
        }
        if req.rect != [0.0; 4] {
            let [ox, oy, _, _] = d.bounds(index)?;
            for (i, v) in req.rect.iter_mut().enumerate() {
                *v += if i % 2 == 0 { ox } else { oy };
            }
        }
        if !in_place {
            std::fs::copy(&cur, &out).map_err(|e| format!("Couldn't write {out}: {e}"))?;
        }
        let mut signing = mupdf::Signing { sign: Box::new(sign), error: None };
        let mut inner = d.inner.lock();
        let res = (|| -> Result<(), String> {
            let page = d.page(&mut inner, index)?;
            page.sign(&req, &mut signing)?;
            drop(page);
            inner.pages.clear();
            inner.doc.save_pdf(&out, true)
        })()
        .map_err(|e| signing.error.take().unwrap_or(e));
        inner.pages.clear();
        d.lists.lock().clear();
        d.texts.lock().clear();
        // Reopen from disk: the signed file, or on failure the untouched original.
        let reopen = if res.is_ok() { out.clone() } else { cur.clone() };
        let fresh = Document::open(&reopen)?;
        if fresh.needs_password() {
            if let Some(pw) = d.password.lock().as_deref() {
                fresh.authenticate(pw);
            }
        }
        inner.doc = fresh;
        drop(inner);
        if let Err(e) = res {
            if !in_place {
                let _ = std::fs::remove_file(&out);
            }
            d.prepare()?;
            return Err(e);
        }
        *d.path.write() = out;
        d.edited();
        d.prepare()?;
        Ok(d.info())
    }

    /// The document's signature fields (JSON, see fo_sigs_json).
    pub fn sigs_json(&self, id: DocId) -> Result<String, String> {
        let d = self.doc(id)?;
        let inner = d.inner.lock();
        if !inner.doc.is_pdf() {
            return Ok("[]".into());
        }
        inner.doc.sigs_json()
    }

    /// The bytes signature `i` covers, and its CMS blob.
    pub fn sig_data(&self, id: DocId, i: usize) -> Result<(Vec<u8>, Vec<u8>), String> {
        self.doc(id)?.inner.lock().doc.sig_data(i)
    }

    /// Saves a copy with form fields (and annotations, if asked) turned into plain
    /// page content, with the given page order and rotations. The open document
    /// is left as it is.
    pub fn save_flattened(&self, id: DocId, target: String, order: Vec<usize>, rot: Vec<i32>, annots: bool) -> Result<(), String> {
        let d = self.doc(id)?;
        let tmp = format!("{target}.paginaut-tmp");
        d.inner.lock().doc.save_copy(&tmp)?;
        let res = (|| -> Result<(), String> {
            let copy = Document::open(&tmp)?;
            if copy.needs_password() {
                if let Some(pw) = d.password.lock().as_deref() {
                    copy.authenticate(pw);
                }
            }
            copy.apply_page_edits(&rot, &order)?;
            copy.bake(annots)?;
            copy.save_pdf(&target, false)
        })();
        let _ = std::fs::remove_file(&tmp);
        res
    }

    /// The document's own page labels ("iv", "A-3"…), or None when they are
    /// just 1, 2, 3… (most files).
    pub fn page_labels(&self, id: DocId) -> Result<Option<Vec<String>>, String> {
        let d = self.doc(id)?;
        if !d.inner.lock().doc.is_pdf() {
            return Ok(None);
        }
        let n = d.page_count();
        let labels: Vec<String> = (0..n).map(|i| d.label(i).unwrap_or_default()).collect();
        let plain = labels.iter().enumerate().all(|(i, l)| l.is_empty() || *l == (i + 1).to_string());
        Ok((!plain).then_some(labels))
    }

    pub fn page_sizes(&self, id: DocId) -> Result<Vec<[f32; 2]>, String> {
        let d = self.doc(id)?;
        (0..d.page_count())
            .map(|i| d.bounds(i).map(|b| [b[2] - b[0], b[3] - b[1]]))
            .collect()
    }

    pub fn cancel(&self, id: u64) {
        self.pool.cancel(id);
    }

    /// Queues a render. `done` always runs exactly once: with pixels, `None`
    /// if cancelled, or an error.
    pub fn render(
        self: &Arc<Self>,
        doc: DocId,
        req: RenderReq,
        rid: u64,
        prio: i32,
        done: impl FnOnce(Result<Option<Rendered>, String>) + Send + 'static,
    ) {
        let d = match self.doc(doc) {
            Ok(d) => d,
            Err(e) => return done(Err(e)),
        };
        self.pool.submit(rid, prio, move |ctl| done(render_now(&d, &req, ctl)));
    }

    /// Searches every page in parallel. `hit` runs per page with matches;
    /// `finished` runs once at the end (true if it ran to completion).
    /// Runs `work` on every page in parallel, starting at `start` and wrapping
    /// around, on private copies of the document (so caches and the page you're
    /// viewing aren't disturbed). `work` returns Ok(false) to stop everything,
    /// Err to stop with an error. `scanned(from, to)` reports finished page
    /// ranges; `finished(complete, stopped, error)` runs once at the end.
    fn scan_pages(
        self: &Arc<Self>,
        doc: DocId,
        start: usize,
        sid: u64,
        work: impl Fn(usize, &PageSrc) -> Result<bool, String> + Send + Sync + 'static,
        scanned: impl Fn(usize, usize) + Send + Sync + 'static,
        finished: impl FnOnce(bool, bool, Option<String>) + Send + 'static,
    ) -> Result<(), String> {
        let d = self.doc(doc)?;
        let gen = d.bg_gen.fetch_add(1, Ordering::AcqRel) + 1;
        let n = d.page_count();
        let chunk = 8usize;
        let chunks = n.div_ceil(chunk).max(1);
        // A fixed number of runners pull chunks in page order, starting at the
        // chunk you're looking at and wrapping around. Each runner costs
        // one private copy of the document, so cap it (and leave threads free
        // for rendering the page you're looking at).
        let runners = search_threads().min(self.workers()).min(chunks).max(1);
        let first_chunk = start.min(n.saturating_sub(1)) / chunk;
        let next = Arc::new(AtomicUsize::new(0));
        let remaining = Arc::new(AtomicUsize::new(runners));
        let stopped = Arc::new(AtomicBool::new(false));
        let work = Arc::new(work);
        let scanned = Arc::new(scanned);
        let (engine, doc_ref) = (self.clone(), d.clone());
        let error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let (err_out, stopped_out) = (error.clone(), stopped.clone());
        let finished = Arc::new(Mutex::new(Some(Box::new(move |complete: bool| {
            finished(complete, stopped_out.load(Ordering::Acquire), err_out.lock().take());
            engine.after_background(&doc_ref, gen);
        }) as Box<dyn FnOnce(bool) + Send>)));
        for _ in 0..runners {
            let (d, work, scanned, finished, remaining, next, error, stopped) =
                (d.clone(), work.clone(), scanned.clone(), finished.clone(), remaining.clone(), next.clone(), error.clone(), stopped.clone());
            self.pool.submit(sid, -10, move |ctl| {
                // Any runner stopping, failing or being cancelled stops them all.
                let halted = || ctl.is_cancelled() || stopped.load(Ordering::Acquire) || error.lock().is_some();
                let run = |src: &PageSrc| {
                    loop {
                        let k = next.fetch_add(1, Ordering::AcqRel);
                        if k >= chunks || halted() {
                            break;
                        }
                        let c = (first_chunk + k) % chunks;
                        let (from, to) = (c * chunk, ((c + 1) * chunk).min(n));
                        for i in from..to {
                            if halted() {
                                break;
                            }
                            match work(i, src) {
                                Ok(true) => {}
                                Ok(false) => stopped.store(true, Ordering::Release),
                                Err(e) => {
                                    let mut slot = error.lock();
                                    if slot.is_none() {
                                        *slot = Some(e);
                                    }
                                }
                            }
                        }
                        if !halted() {
                            scanned(from, to);
                        }
                    }
                };
                if d.content_edited.load(Ordering::Acquire) {
                    run(&PageSrc::Shared(&d));
                } else {
                    let _ = d.with_private(|doc| {
                        run(&PageSrc::Private(doc));
                        Ok(())
                    });
                }
                if remaining.fetch_sub(1, Ordering::AcqRel) == 1 {
                    if let Some(f) = finished.lock().take() {
                        f(!ctl.is_cancelled() && error.lock().is_none());
                    }
                }
            });
        }
        Ok(())
    }

    pub fn search(
        self: &Arc<Self>,
        doc: DocId,
        needle: String,
        opts: SearchOptions,
        sid: u64,
        hit: impl Fn(usize, PageHits) + Send + Sync + 'static,
        scanned: impl Fn(usize, usize) + Send + Sync + 'static,
        finished: impl FnOnce(SearchEnd) + Send + 'static,
    ) -> Result<(), String> {
        let total = AtomicUsize::new(0);
        self.scan_pages(
            doc,
            opts.start,
            sid,
            move |i, src| {
                // Text is built from fresh lists so a search doesn't flood the caches.
                let json = src.list(i).and_then(|(list, b)| list.text()?.find(&b, &needle, opts.flags(), opts.snippets))?;
                let found = serde_json::from_str::<Vec<FoundHit>>(&json).map_err(|e| e.to_string())?;
                if found.is_empty() {
                    return Ok(true);
                }
                let mut quads = Vec::with_capacity(found.len());
                let mut snippets = Vec::new();
                for f in found {
                    quads.push(f.q);
                    if let Some(s) = f.s {
                        snippets.push(s);
                    }
                }
                let count = quads.len();
                hit(i, PageHits { quads, snippets });
                Ok(total.fetch_add(count, Ordering::AcqRel) + count < MAX_SEARCH_HITS)
            },
            scanned,
            // An error is e.g. an invalid regular expression, reported once.
            move |complete, limited, error| finished(SearchEnd { complete: complete || limited, error, limited }),
        )
    }

    /// Streams every page's reflow content (see fo_reflow_json), starting at `start`.
    pub fn reflow(
        self: &Arc<Self>,
        doc: DocId,
        start: usize,
        sid: u64,
        page: impl Fn(usize, String) + Send + Sync + 'static,
        finished: impl FnOnce(bool, Option<String>) + Send + 'static,
    ) -> Result<(), String> {
        self.scan_pages(
            doc,
            start,
            sid,
            move |i, src| {
                let json = src.list(i).and_then(|(list, b)| list.reflow(&b))?;
                page(i, json);
                Ok(true)
            },
            |_, _| {},
            move |complete, _, error| finished(complete, error),
        )
    }
}

fn same_file(a: &str, b: &str) -> bool {
    let norm = |p: &str| {
        std::fs::canonicalize(p).map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_else(|_| p.replace('/', "\\").to_lowercase())
    };
    norm(a) == norm(b)
}

/// Copies over a file that other readers may still have open for a moment.
fn copy_with_retry(from: &str, to: &str) -> Result<(), String> {
    let mut last = String::new();
    for _ in 0..30 {
        match std::fs::copy(from, to) {
            Ok(_) => return Ok(()),
            Err(e) => last = e.to_string(),
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(format!("couldn't replace the file: {last}"))
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Runs before the pool is dropped, so workers purge while their
        // private MuPDF contexts are still alive.
        self.release_private();
    }
}

fn render_now(d: &Doc, req: &RenderReq, ctl: &Ctl) -> Result<Option<Rendered>, String> {
    if ctl.is_cancelled() {
        return Ok(None);
    }
    let bounds = d.bounds(req.page)?;
    let list = d.list(req.page, req.extras)?;
    if ctl.is_cancelled() {
        return Ok(None);
    }
    let (w, h) = mupdf::render_size(&bounds, req.scale, req.rotate);
    let clip = match req.clip {
        Some(c) => [c[0].max(0), c[1].max(0), c[2].min(w as i32), c[3].min(h as i32)],
        None => [0, 0, w as i32, h as i32],
    };
    let (cw, ch) = ((clip[2] - clip[0]).max(0) as u32, (clip[3] - clip[1]).max(0) as u32);
    if cw == 0 || ch == 0 {
        return Err("empty render area".into());
    }
    if (cw as u64) * (ch as u64) > 64 * 1024 * 1024 {
        return Err("render area too large".into());
    }
    let mut bytes = vec![0u8; 8 + (cw * ch * 4) as usize];
    bytes[0..4].copy_from_slice(&cw.to_le_bytes());
    bytes[4..8].copy_from_slice(&ch.to_le_bytes());
    match list.render(&bounds, req.scale, req.rotate, clip, &mut bytes[8..], Some(&ctl.cookie))? {
        RenderOutcome::Done => Ok(Some(Rendered { bytes, width: cw, height: ch })),
        RenderOutcome::Aborted => Ok(None),
    }
}
