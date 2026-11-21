//! Document engine: owns open documents, caches, and the worker pool.

pub mod ffi;
pub mod mupdf;
pub mod ocr;
mod pdfhtml;
pub mod sandbox;
pub use pdfhtml::Preview as BookPreview;
pub mod pool;
pub mod table;
#[cfg(desktop)]
pub mod pdfa;

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

/// Office files MuPDF shows itself (text only) when no converter is available: laid out on A4.
fn layout_reflowable(d: &Document, path: &str) -> Result<(), String> {
    if !d.is_reflowable() {
        return Ok(());
    }
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    // Documents (Office files shown by MuPDF itself, Markdown) are laid out on A4; e-books and web pages smaller.
    if matches!(ext.as_str(), "docx" | "xlsx" | "pptx" | "hwpx" | "md" | "markdown") {
        d.layout(595.0, 842.0, 10.0)
    } else {
        d.layout(REFLOW.0, REFLOW.1, REFLOW.2)
    }
}

/// An Office file (Word, Excel, PowerPoint…) opened through a PDF made from it.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Converted {
    /// The original file.
    pub from: String,
    /// The converter ("msoffice", "libreoffice") and its name for people ("Microsoft Word").
    pub engine: String,
    pub engine_name: String,
    /// The PDF made from it (in the app's cache).
    #[serde(skip)]
    pub pdf: String,
}

/// Turns an Office file into a PDF, with a named converter or the preferred one. Ok(None): the file
/// isn't an Office file, or only MuPDF's own (text only) preview is available for it.
pub type Converter = dyn Fn(&str, Option<&str>) -> Result<Option<Converted>, String> + Send + Sync;

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
    /// Pages in the file on disk; pages from index on were added in memory.
    file_pages: AtomicUsize,
    /// Office file this PDF was made from, until it's saved as a PDF of its own.
    converted: RwLock<Option<Converted>>,
    /// The file the open document reads when it isn't `path`: a save wrote everything into a copy beside the file
    /// but couldn't put it in the file's place, and that copy is kept (with the changes) until a save succeeds.
    backing: Mutex<Option<String>>,
    /// Something was taken out of the document since it was last saved (Sanitize, a deleted comment or attached
    /// file, text or a picture edited away, properties changed): the next save writes the file afresh. An ordinary
    /// save only adds the changes to the file's end, and what was taken out would still be in its earlier bytes.
    took_out: AtomicBool,
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
    /// Set for an Office file shown through a PDF made from it (`path` is that PDF).
    pub converted: Option<Converted>,
    /// An EPUB made from a PDF, shown as the PDF it was made from (pdfhtml.rs; `converted` says from
    /// which file); `save_repaired` makes a flowing EPUB of it.
    pub repaired: bool,
    /// Set by a save that wrote everything but couldn't put it in the file's place: why. The document shown is the
    /// saved copy (page edits applied, nothing to undo), and its changes still have to be saved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_saved: Option<String>,
}

/// For the shelf's cover: an EPUB made from a PDF (pdfhtml.rs) as a PDF of its first page alone, with its title,
/// author and pages (the caller removes the PDF). None for any other file, which opens as it is.
pub fn book_preview(path: &str) -> Option<BookPreview> {
    pdfhtml::preview(path, &mupdf::temp_dir())
}

/// A file to take pages from (all of them unless `pages` lists some, 0-based).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsertSource {
    pub path: String,
    #[serde(default)]
    pub pages: Option<Vec<usize>>,
    #[serde(default)]
    pub password: Option<String>,
}

/// A quick look at a file before taking pages from it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Peek {
    pub pages: usize,
    pub is_pdf: bool,
    /// It needs a password (none given, or the one given is wrong).
    pub locked: bool,
}

/// One page to crop or resize: the part to keep (relative to its top-left) and,
/// if it changes size, the new size (as the page is shown without extra rotation).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSizeOp {
    pub page: usize,
    pub crop: [f32; 4],
    #[serde(default)]
    pub size: Option<[f32; 2]>,
}

/// A stamp for one page: its pieces (see mupdf::StampPart).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampPage {
    pub page: usize,
    pub parts: Vec<mupdf::StampPart>,
}

/// The document information shown and edited in Document properties.
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Properties {
    pub title: String,
    pub author: String,
    pub subject: String,
    pub keywords: String,
    #[serde(default)]
    pub creator: String,
    #[serde(default)]
    pub producer: String,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub modified: String,
    /// The PDF/A part and level the file says it meets ("PDF/A-2b"), or "".
    #[serde(default)]
    pub pdfa: String,
}

/// A picture on a page: where (relative to the page origin), its size in pixels, how it's
/// stored ("jpg", "jp2", "png"), whether it has transparency, stored bytes, and an id
/// (the same picture used again has the same id; empty when unknown).
#[derive(Serialize, Deserialize, Clone)]
pub struct Picture {
    pub r: [f32; 4],
    pub w: i32,
    pub h: i32,
    pub k: String,
    pub m: i32,
    pub n: usize,
    pub id: String,
}

/// A file embedded in a PDF: on the document (page -1) or pinned to a page.
#[derive(Serialize, Deserialize, Clone)]
pub struct Attachment {
    pub i: usize,
    pub page: i32,
    pub size: i64,
    pub modified: i64,
    pub name: String,
    pub mime: String,
    pub desc: String,
}

/// A paragraph as the text editor sees it: box, lines ([x0, y0, x1, y1, baseline]), size, baseline
/// to baseline, colour, bold, italic, serif, monospace, alignment (l, r, c, j), font name and text.
#[derive(Serialize, Deserialize, Clone)]
pub struct Paragraph {
    pub r: [f32; 4],
    pub l: Vec<[f32; 5]>,
    /// How many pictures the page draws before it (what's drawn later is on top).
    #[serde(default)]
    pub z: i32,
    pub s: f32,
    pub lead: f32,
    pub c: String,
    pub b: i32,
    pub i: i32,
    pub serif: i32,
    pub mono: i32,
    pub a: String,
    pub font: String,
    pub t: String,
    /// The text in runs of one style ([text, 1 bold | 2 italic]) when it mixes them; empty when it doesn't.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub u: Vec<(String, u8)>,
}

/// A paragraph's new text, with the paragraph as it was found (the text goes from its lines). With `runs`, the
/// text in runs of one style ([text, 1 bold | 2 italic]; `text` is then their letters).
#[derive(Deserialize)]
pub struct TextEdit {
    pub para: Paragraph,
    pub text: String,
    #[serde(default)]
    pub runs: Option<Vec<(String, u8)>>,
}

/// Text for the HTML engine: escaped, line breaks as <br/>.
fn html_text(t: &str) -> String {
    t.chars().fold(String::new(), |mut o, ch| {
        match ch {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '\n' => o.push_str("<br/>"),
            '\r' => {}
            _ => o.push(ch),
        }
        o
    })
}

/// A redaction mark to add: text (quads, 8 numbers each) or an area, relative to the page origin.
#[derive(Deserialize)]
pub struct RedactMark {
    pub page: usize,
    #[serde(default)]
    pub quads: Vec<f32>,
    #[serde(default)]
    pub rect: Option<[f32; 4]>,
}

/// What applying redactions did: marks applied on how many pages, and letters still readable
/// under the boxes afterwards (0 when removing worked; covering leaves them all).
#[derive(Serialize)]
pub struct Redacted {
    pub pages: usize,
    pub marks: usize,
    pub left: usize,
}

/// What Sanitize took out.
#[derive(Serialize, Deserialize, Default)]
pub struct Sanitized {
    pub meta: usize,
    pub scripts: usize,
    pub files: usize,
    pub comments: usize,
    pub hidden: usize,
}

/// What "Save all pictures" did.
#[derive(Serialize)]
pub struct SavedPictures {
    pub saved: usize,
    pub small: usize,
    pub repeats: usize,
}

/// `dir/name.ext`, or `dir/name (2).ext`… when that's taken. Names are made safe for Windows.
pub fn free_path(dir: &str, name: &str, ext: &str) -> std::path::PathBuf {
    let clean: String = name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let clean = clean.trim().trim_end_matches('.').to_string();
    let clean = if clean.is_empty() { "file".to_string() } else { clean };
    let dot = if ext.is_empty() { String::new() } else { format!(".{ext}") };
    let mut p = std::path::Path::new(dir).join(format!("{clean}{dot}"));
    let mut n = 2;
    while p.exists() {
        p = std::path::Path::new(dir).join(format!("{clean} ({n}){dot}"));
        n += 1;
    }
    p
}

/// A PDF's protection, as the Protect dialog shows it.
#[derive(Serialize, Deserialize, Default)]
pub struct Security {
    /// "AES" or "RC4" (with `bits`), or "" when not protected.
    pub method: String,
    pub bits: i32,
    pub perms: i32,
    /// Opened with the owner (permissions) password, or it has none: its protection can be changed.
    pub owner: bool,
    /// Needs a password to open.
    pub opens: bool,
    /// Damaged, and repaired when it was opened.
    pub repaired: bool,
}

/// New protection for a saved PDF: `remove` it, or AES-256 with these passwords (either may be
/// empty) and permissions (PDF permission bits).
#[derive(Deserialize, Clone, Default)]
pub struct Protection {
    #[serde(default)]
    pub remove: bool,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default = "all_permissions")]
    pub perms: i32,
}

fn all_permissions() -> i32 {
    -1
}

/// How to write a PDF beyond a plain save.
#[derive(Deserialize, Clone, Default)]
pub struct SaveOptions {
    #[serde(default)]
    pub protect: Option<Protection>,
    /// 0 lossless, 1 smaller (pictures at 150 dpi), 2 smallest (100 dpi, all pictures JPEG).
    #[serde(default)]
    pub compress: Option<u8>,
    /// Write every object again from scratch (repairs a damaged file).
    #[serde(default)]
    pub rewrite: bool,
}

impl SaveOptions {
    fn any(&self) -> bool {
        self.protect.is_some() || self.compress.is_some() || self.rewrite
    }
}

/// One file to write: these pages (document indices, in order) at `path`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    pub pages: Vec<usize>,
    pub path: String,
}

/// Pages added at the end of a document: indices first..first + count.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inserted {
    pub first: usize,
    pub count: usize,
    /// Width and height of each new page, in points.
    pub sizes: Vec<[f32; 2]>,
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
                let d = Document::open_private(&self.file())?.unlocked_with(self.password.lock().as_deref());
                layout_reflowable(&d, &self.path())?;
                m.insert(self.id, d);
            }
            f(&m[&self.id])
        })
    }

    pub fn path(&self) -> String {
        self.path.read().clone()
    }

    /// The file the open document reads: its path, or the copy a failed save kept (see `backing`).
    pub fn file(&self) -> String {
        self.backing.lock().clone().unwrap_or_else(|| self.path())
    }

    /// A copy of the document for the calling thread alone (its private MuPDF context: nothing shared with other
    /// threads), or None when its pages were changed in memory and only this document has them.
    pub fn private_copy(&self) -> Result<Option<Document>, String> {
        if self.content_edited.load(Ordering::Acquire) {
            return Ok(None);
        }
        let d = Document::open_private(&self.file())?.unlocked_with(self.password.lock().as_deref());
        layout_reflowable(&d, &self.path())?;
        Ok(Some(d))
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

    /// The pages' own content changed in memory (text, pictures, redactions, stamps, pages added or resized): as
    /// `edited`, and background jobs (search, Read and Book mode, Compare) read this document from now on, not
    /// copies of the file on disk. Every edit of page content goes through here.
    pub(crate) fn content_changed(&self) {
        self.content_edited.store(true, Ordering::Release);
        self.edited();
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
        let mut inner = self.inner.lock();
        let step = inner.doc.undo(redo)?;
        if let Some(layout) = step {
            if layout {
                // Pages were added, removed or resized: what's known about them is stale.
                self.resync_pages(&mut inner)?;
                inner.pages.clear();
                self.bounds.write().iter_mut().for_each(|b| *b = None);
            }
            self.edited();
        }
        Ok(FormChange { accepted: step.is_some(), changed: inner.doc.has_changes() })
    }

    /// After a save that failed: the save's own changes (page order, rotations…) are taken back, so the document is
    /// as it was before (`start`: where its journal stood). Without a journal, the file is opened again (the app
    /// keeps page order and rotations itself, and there were no other changes).
    fn after_failed_save(&self, start: Option<usize>) -> Result<(), String> {
        let mut inner = self.inner.lock();
        inner.pages.clear();
        match start {
            Some(start) => {
                while inner.doc.journal_pos().is_some_and(|p| p > start) {
                    if inner.doc.undo(false)?.is_none() {
                        break;
                    }
                }
                self.resync_pages(&mut inner)?;
                inner.pages.clear();
                drop(inner);
                self.bounds.write().iter_mut().for_each(|b| *b = None);
                self.edited();
                Ok(())
            }
            None => {
                let fresh = Document::open(&self.file())?.unlocked_with(self.password.lock().as_deref());
                inner.doc = fresh;
                drop(inner);
                self.prepare()
            }
        }
    }

    /// Picks up a changed page count (pages inserted, or that undone).
    fn resync_pages(&self, inner: &mut DocInner) -> Result<(), String> {
        let n = inner.doc.page_count()?;
        if n != self.page_count() {
            inner.pages.clear();
            self.bounds.write().resize(n, None);
            self.page_count.store(n, Ordering::Release);
        }
        Ok(())
    }

    // ------------------------------------------------------------ page size

    /// The box around everything drawn on a page (relative to its top-left), None if blank.
    pub fn content_box(&self, index: usize) -> Result<Option<[f32; 4]>, String> {
        self.check(index)?;
        let mut inner = self.inner.lock();
        let page = self.page(&mut inner, index)?;
        page.content_box()
    }

    /// Crops and/or resizes pages as one undo step. Returns their new sizes.
    pub fn page_size(&self, ops: &[PageSizeOp], opts: &mupdf::PageSizeOptions) -> Result<Vec<[f32; 2]>, String> {
        let mut inner = self.inner.lock();
        if !inner.doc.is_pdf() {
            return Err("Only PDF pages can be cropped or resized. Save this file as a PDF first (Save as PDF).".into());
        }
        for op in ops {
            self.check(op.page)?;
        }
        inner.doc.begin_op("Page size");
        let res = (|| -> Result<(), String> {
            for op in ops {
                let page = self.page(&mut inner, op.page)?;
                page.page_size(op.crop, op.size, opts)?;
            }
            Ok(())
        })();
        if let Err(e) = res {
            inner.doc.abandon_op();
            inner.pages.clear();
            return Err(e);
        }
        inner.doc.end_op();
        // Pages know their size from when they were loaded.
        inner.pages.clear();
        drop(inner);
        {
            let mut b = self.bounds.write();
            for op in ops {
                if let Some(slot) = b.get_mut(op.page) {
                    *slot = None;
                }
            }
        }
        self.content_changed();
        ops.iter().map(|op| self.bounds(op.page).map(|b| [b[2] - b[0], b[3] - b[1]])).collect()
    }

    // ------------------------------------------------------------ pictures, areas, attachments

    /// Pictures on page `index`; `own`: only those in the page's own content (not comments, stamps or fields).
    pub fn pictures(&self, index: usize, own: bool) -> Result<Vec<Picture>, String> {
        let b = self.bounds(index)?;
        let json = self.list(index, if own { 0 } else { 1 })?.pictures_json(&b)?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    }

    /// Deletes (0), moves or resizes (1), replaces (2) or adds (3) a picture on page `index` (one undo step).
    pub fn picture_edit(&self, index: usize, target: [f32; 4], action: i32, dest: [f32; 4], data: &[u8]) -> Result<(), String> {
        if action != 3 {
            self.took_out.store(true, Ordering::Release);
        }
        {
            let mut inner = self.inner.lock();
            if !inner.doc.is_pdf() {
                return Err("Only PDFs can be edited. Save this file as a PDF first (Save as PDF).".into());
            }
            let page = self.page(&mut inner, index)?;
            page.picture_edit(&target, action, &dest, data)?;
        }
        self.content_changed();
        Ok(())
    }

    /// Picture `idx` of page `index` (fmt 0 as stored, 1 PNG, 2 PDF) and its extension.
    pub fn picture_data(&self, index: usize, idx: usize, fmt: i32) -> Result<(Vec<u8>, String), String> {
        self.list(index, 1)?.picture_data(idx, fmt)
    }

    /// Saves the pictures on `pages` (named with `names`) into `dir`: each picture once, none
    /// with a side shorter than `min` pixels.
    pub fn save_pictures(&self, pages: &[usize], names: &[String], min: i32, fmt: i32, dir: &str, stem: &str) -> Result<SavedPictures, String> {
        let mut seen = std::collections::HashSet::new();
        let mut out = SavedPictures { saved: 0, small: 0, repeats: 0 };
        for (k, &i) in pages.iter().enumerate() {
            let list = self.list(i, 1)?;
            let pics: Vec<Picture> = serde_json::from_str(&list.pictures_json(&self.bounds(i)?)?).map_err(|e| e.to_string())?;
            let mut n = 0;
            for (j, p) in pics.iter().enumerate() {
                if p.w.min(p.h) < min {
                    out.small += 1;
                    continue;
                }
                if !p.id.is_empty() && !seen.insert(p.id.clone()) {
                    out.repeats += 1;
                    continue;
                }
                let (bytes, ext) = list.picture_data(j, fmt)?;
                n += 1;
                let name = format!("{stem} page {} picture {n}", names.get(k).map(String::as_str).unwrap_or(""));
                std::fs::write(free_path(dir, &name, &ext), bytes).map_err(|e| e.to_string())?;
                out.saved += 1;
            }
        }
        Ok(out)
    }

    /// An area of page `index` (relative to its origin), turned by `rot`: "png" at `dpi`, "svg" or "pdf".
    pub fn area_data(&self, index: usize, area: [f32; 4], rot: i32, kind: &str, dpi: f32) -> Result<Vec<u8>, String> {
        let b = self.bounds(index)?;
        match kind {
            "pdf" => {
                let abs = [area[0] + b[0], area[1] + b[1], area[2] + b[0], area[3] + b[1]];
                let mut inner = self.inner.lock();
                if !inner.doc.is_pdf() {
                    return Err("Only PDF pages can be saved as PDF areas. Save as a picture instead.".into());
                }
                let page = self.page(&mut inner, index)?;
                page.area_pdf(&abs, rot)
            }
            "svg" => self.list(index, 1)?.area_image(&b, &area, rot, 72.0, 1),
            "jpg" => self.list(index, 1)?.area_image(&b, &area, rot, dpi.clamp(36.0, 1200.0), 2),
            "pnm" => self.list(index, 1)?.area_image(&b, &area, rot, dpi.clamp(36.0, 1200.0), 3),
            _ => self.list(index, 1)?.area_image(&b, &area, rot, dpi.clamp(36.0, 1200.0), 0),
        }
    }

    /// The whole page `index` as a picture ("png", "jpg" or "pnm"), turned by `rot`, at `dpi`.
    pub fn page_image(&self, index: usize, rot: i32, kind: &str, dpi: f32) -> Result<Vec<u8>, String> {
        let b = self.bounds(index)?;
        self.area_data(index, [0.0, 0.0, b[2] - b[0], b[3] - b[1]], rot, kind, dpi)
    }

    /// Paragraphs of text on page `index` (its own content, not comments or form fields).
    pub fn paragraphs(&self, index: usize) -> Result<Vec<Paragraph>, String> {
        let b = self.bounds(index)?;
        serde_json::from_str(&self.list(index, 0)?.paragraphs_json(&b)?).map_err(|e| e.to_string())
    }

    /// Replaces a paragraph's text on page `index` (one undo step), laid out in its box with its
    /// size, spacing, colour and alignment, in its own font where that has the letters.
    pub fn edit_text(&self, index: usize, e: &TextEdit) -> Result<(), String> {
        self.took_out.store(true, Ordering::Release);
        let p = &e.para;
        // Runs carry their own weight and style (<b>, <i>) over a plain paragraph.
        let runs = e.runs.as_ref().filter(|r| !r.is_empty());
        let esc = match runs {
            Some(runs) => {
                let n = runs.len();
                runs.iter()
                    .enumerate()
                    .map(|(k, (t, st))| {
                        let t = if k + 1 == n { t.trim_end() } else { t.as_str() };
                        let mut s = html_text(t);
                        if st & 2 != 0 {
                            s = format!("<i>{s}</i>");
                        }
                        if st & 1 != 0 {
                            s = format!("<b>{s}</b>");
                        }
                        s
                    })
                    .collect::<String>()
            }
            None => html_text(e.text.trim_end()),
        };
        let html = if e.text.trim().is_empty() { String::new() } else { format!("<p>{esc}</p>") };
        let align = match p.a.as_str() {
            "r" => "right",
            "c" => "center",
            "j" => "justify",
            _ => "left",
        };
        let family = if p.mono != 0 { "monospace" } else if p.serif != 0 { "serif" } else { "sans-serif" };
        let color = if p.c.len() == 7 && p.c.starts_with('#') && p.c[1..].chars().all(|c| c.is_ascii_hexdigit()) { p.c.as_str() } else { "#000000" };
        let size = p.s.clamp(1.0, 500.0);
        let css = format!(
            "p{{margin:0;font-size:{size}pt;line-height:{}pt;color:{color};text-align:{align};font-family:{family};font-weight:{};font-style:{}}}",
            p.lead.clamp(size * 0.8, size * 4.0),
            if p.b != 0 && runs.is_none() { "bold" } else { "normal" },
            if p.i != 0 && runs.is_none() { "italic" } else { "normal" },
        );
        let lines: Vec<[f32; 4]> = p.l.iter().map(|l| [l[0], l[1], l[2], l[3]]).collect();
        let first = p.l.first().ok_or("That paragraph has no lines.")?;
        // A little room on the right, so a line that just fitted still does.
        let width = (p.r[2] - p.r[0]) + size * 0.3;
        let probe = [(first[0] + first[2]) / 2.0, (first[1] + first[3]) / 2.0];
        {
            let mut inner = self.inner.lock();
            if !inner.doc.is_pdf() {
                return Err("Only PDFs can be edited. Save this file as a PDF first (Save as PDF).".into());
            }
            let page = self.page(&mut inner, index)?;
            page.edit_text(&lines, probe, &html, &css, p.r[0], first[4], width)?;
        }
        self.content_changed();
        Ok(())
    }

    /// Tables found on page `index`.
    pub fn tables(&self, index: usize) -> Result<Vec<table::Table>, String> {
        let b = self.bounds(index)?;
        let list: Vec<Option<table::Table>> = serde_json::from_str(&self.list(index, 1)?.tables_json(&b)?).map_err(|e| e.to_string())?;
        Ok(list.into_iter().flatten().filter(|t| t.rows() > 0 && t.cols() > 0).collect())
    }

    /// The table in `area` of page `index`: found, or with exactly these column lines, and row lines too if given.
    pub fn table_in(&self, index: usize, area: [f32; 4], cols: &[f32], rows: &[f32]) -> Result<Option<table::Table>, String> {
        let b = self.bounds(index)?;
        let json = self.list(index, 1)?.table_in_json(&b, &area, cols, rows)?;
        let list: Vec<Option<table::Table>> = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        Ok(list.into_iter().flatten().filter(|t| t.rows() > 0 && t.cols() > 0).max_by_key(|t| t.cells.len()))
    }

    pub fn area_text(&self, index: usize, area: [f32; 4]) -> Result<String, String> {
        let b = self.bounds(index)?;
        self.list(index, 1)?.area_text(&b, &area)
    }

    /// The words on page `index` in reading order, with their boxes (JSON, see fo_text_words_json).
    pub fn words_json(&self, index: usize) -> Result<String, String> {
        let b = self.bounds(index)?;
        let t = self.text(index)?;
        let json = t.lock().words(&b)?;
        Ok(json)
    }

    /// The text in each of several areas of page `index`.
    pub fn area_texts(&self, index: usize, areas: &[[f32; 4]]) -> Result<Vec<String>, String> {
        let b = self.bounds(index)?;
        self.list(index, 1)?.area_texts(&b, areas)
    }

    pub fn attachments(&self) -> Result<Vec<Attachment>, String> {
        let inner = self.inner.lock();
        if !inner.doc.is_pdf() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&inner.doc.attachments_json()?).map_err(|e| e.to_string())
    }

    pub fn attachment_data(&self, i: usize) -> Result<Vec<u8>, String> {
        self.inner.lock().doc.attachment_data(i)
    }

    /// Attaches the file at `path` to the document (one undo step).
    pub fn attachment_add(&self, path: &str) -> Result<(), String> {
        let p = std::path::Path::new(path);
        let meta = std::fs::metadata(p).map_err(|e| e.to_string())?;
        if meta.len() > 500 << 20 {
            return Err("That file is too large to attach (over 500 MB).".into());
        }
        let data = std::fs::read(p).map_err(|e| e.to_string())?;
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("attachment").to_string();
        let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(-1, |d| d.as_secs() as i64);
        let mime = match p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase().as_str() {
            "pdf" => "application/pdf",
            "txt" => "text/plain",
            "csv" => "text/csv",
            "xml" => "application/xml",
            "json" => "application/json",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "zip" => "application/zip",
            _ => "",
        };
        let inner = self.inner.lock();
        if !inner.doc.is_pdf() {
            return Err("Only PDFs can have attachments. Save this file as a PDF first (Save as PDF).".into());
        }
        inner.doc.attachment_add(&name, mime, &data, modified)?;
        drop(inner);
        self.edited();
        Ok(())
    }

    pub fn attachment_remove(&self, i: usize) -> Result<(), String> {
        self.took_out.store(true, Ordering::Release);
        self.inner.lock().doc.attachment_remove(i)?;
        self.edited();
        Ok(())
    }

    // ------------------------------------------------------------ redaction

    /// Adds redaction marks (red outlines until applied), all as one undo step. Returns how many.
    pub fn redact_mark(&self, marks: &[RedactMark], author: &str) -> Result<usize, String> {
        // Page bounds first: reading one not known yet takes the document's lock.
        let origins = marks.iter().map(|m| self.bounds(m.page)).collect::<Result<Vec<_>, _>>()?;
        let mut inner = self.inner.lock();
        if !inner.doc.is_pdf() {
            return Err("Only PDFs can be redacted. Save this file as a PDF first (Save as PDF).".into());
        }
        inner.doc.begin_op("Mark for redaction");
        let res = (|| -> Result<usize, String> {
            let mut n = 0;
            for (m, &[ox, oy, _, _]) in marks.iter().zip(&origins) {
                let shift = |v: &[f32]| v.iter().enumerate().map(|(i, x)| x + if i % 2 == 0 { ox } else { oy }).collect::<Vec<f32>>();
                let edit = mupdf::AnnotEdit {
                    quads: (!m.quads.is_empty()).then(|| shift(&m.quads)),
                    rect: m.rect.map(|r| {
                        let s = shift(&r);
                        [s[0], s[1], s[2], s[3]]
                    }),
                    color: Some(vec![0.86, 0.1, 0.1]),
                    ..Default::default()
                };
                if edit.quads.is_none() && edit.rect.is_none() {
                    continue;
                }
                let page = self.page(&mut inner, m.page)?;
                let num = page.annot_create("Redact", author)?;
                page.annot_apply(num, &edit)?;
                n += 1;
            }
            Ok(n)
        })();
        match res {
            Ok(n) => {
                inner.doc.end_op();
                drop(inner);
                self.edited();
                Ok(n)
            }
            Err(e) => {
                // The marks made so far are taken back in the document; the pages kept in memory still had them.
                inner.doc.abandon_op();
                inner.pages.clear();
                drop(inner);
                self.edited();
                Err(e)
            }
        }
    }

    /// Pages with redaction marks, and how many on each.
    pub fn redaction_pages(&self) -> Result<Vec<(usize, usize)>, String> {
        let inner = self.inner.lock();
        if !inner.doc.is_pdf() {
            return Ok(Vec::new());
        }
        inner.doc.redaction_pages()
    }

    /// Takes every redaction mark off (one undo step). Returns how many.
    pub fn redact_clear(&self) -> Result<usize, String> {
        let pages = self.redaction_pages()?;
        let mut n = 0;
        if pages.is_empty() {
            return Ok(0);
        }
        let bounds = pages.iter().map(|&(i, _)| self.bounds(i)).collect::<Result<Vec<_>, _>>()?;
        let mut inner = self.inner.lock();
        inner.doc.begin_op("Clear redaction marks");
        let res = (|| -> Result<(), String> {
            for (&(i, _), &b) in pages.iter().zip(&bounds) {
                let page = self.page(&mut inner, i)?;
                let list: Vec<serde_json::Value> = serde_json::from_str(&page.annots_json(&b)?).map_err(|e| e.to_string())?;
                for a in list.iter().filter(|a| a["t"] == "Redact") {
                    if let Some(num) = a["n"].as_i64() {
                        page.annot_delete(num as i32)?;
                        n += 1;
                    }
                }
            }
            Ok(())
        })();
        match res {
            Ok(()) => inner.doc.end_op(),
            Err(e) => {
                inner.doc.abandon_op();
                inner.pages.clear();
                drop(inner);
                self.edited();
                return Err(e);
            }
        }
        drop(inner);
        self.edited();
        Ok(n)
    }

    /// Applies every redaction mark (one undo step): removes what's under them, or with `cover`
    /// only covers it; boxes in `rgb`, with `label` in them if not empty.
    pub fn redact_apply(&self, cover: bool, rgb: [f32; 3], label: &str) -> Result<Redacted, String> {
        let pages = self.redaction_pages()?;
        let mut out = Redacted { pages: 0, marks: 0, left: 0 };
        if pages.is_empty() {
            return Ok(out);
        }
        let mut inner = self.inner.lock();
        inner.doc.begin_op(if cover { "Cover marked areas" } else { "Apply redactions" });
        let res = (|| -> Result<(), String> {
            for &(i, _) in &pages {
                let page = self.page(&mut inner, i)?;
                let (n, left) = page.redact_apply(cover, rgb, label)?;
                out.pages += 1;
                out.marks += n;
                out.left += left;
            }
            Ok(())
        })();
        match res {
            Ok(()) => inner.doc.end_op(),
            Err(e) => {
                inner.doc.abandon_op();
                return Err(e);
            }
        }
        drop(inner);
        self.content_changed();
        Ok(out)
    }

    /// Takes out (one undo step): what & 1 metadata, 2 scripts, 4 attached files, 8 comments, 16 hidden text.
    pub fn sanitize(&self, what: i32) -> Result<Sanitized, String> {
        self.took_out.store(true, Ordering::Release);
        let mut out = Sanitized::default();
        // Hidden text and scripts come out of the pages' content.
        self.doc_step("Sanitize", true, |doc| {
            out = serde_json::from_str(&doc.sanitize(what)?).map_err(|e| e.to_string())?;
            Ok(())
        })?;
        Ok(out)
    }

    // ------------------------------------------------------------ bookmarks, labels, properties

    /// Runs `f` on the PDF as one undo step named `name`.
    fn doc_op(&self, name: &str, f: impl FnOnce(&Document) -> Result<(), String>) -> Result<(), String> {
        self.doc_step(name, false, f)
    }

    /// As `doc_op`; `content`: the step changes the pages' content (see `content_changed`).
    fn doc_step(&self, name: &str, content: bool, f: impl FnOnce(&Document) -> Result<(), String>) -> Result<(), String> {
        let inner = self.inner.lock();
        if !inner.doc.is_pdf() {
            return Err("Only PDFs can be changed this way. Save this file as a PDF first (Save as PDF).".into());
        }
        inner.doc.begin_op(name);
        if let Err(e) = f(&inner.doc) {
            inner.doc.abandon_op();
            return Err(e);
        }
        inner.doc.end_op();
        drop(inner);
        if content {
            self.content_changed();
        } else {
            self.edited();
        }
        Ok(())
    }

    pub fn set_outline(&self, items: &[mupdf::OutlineNode]) -> Result<(), String> {
        self.doc_op("Bookmarks", |doc| doc.set_outline(items))
    }

    pub fn page_label_ranges(&self) -> Result<Vec<mupdf::LabelRange>, String> {
        let json = self.inner.lock().doc.page_label_ranges_json()?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    }

    pub fn set_page_labels(&self, ranges: &[mupdf::LabelRange]) -> Result<(), String> {
        for r in ranges {
            self.check(r.s)?;
        }
        self.doc_op("Page labels", |doc| doc.set_page_labels(ranges))
    }

    pub fn properties(&self) -> Properties {
        let inner = self.inner.lock();
        let m = |k: &str| inner.doc.metadata(k).unwrap_or_default();
        Properties {
            title: m("info:Title"),
            author: m("info:Author"),
            subject: m("info:Subject"),
            keywords: m("info:Keywords"),
            creator: m("info:Creator"),
            producer: m("info:Producer"),
            created: m("info:CreationDate"),
            modified: m("info:ModDate"),
            pdfa: {
                let claim = inner.doc.pdfa_claim();
                let (part, level) = claim.split_at(claim.find(|c: char| !c.is_ascii_digit()).unwrap_or(claim.len()));
                if part.is_empty() { String::new() } else { format!("PDF/A-{part}{}", level.to_lowercase()) }
            },
        }
    }

    pub fn set_properties(&self, p: &Properties) -> Result<(), String> {
        self.took_out.store(true, Ordering::Release);
        self.doc_op("Properties", |doc| {
            for (k, v) in [("info:Title", &p.title), ("info:Author", &p.author), ("info:Subject", &p.subject), ("info:Keywords", &p.keywords)] {
                doc.set_metadata(k, v.trim())?;
            }
            Ok(())
        })
    }

    // ------------------------------------------------------------ stamps

    /// Stamps pages (page numbers, headers and footers, watermarks) as one undo
    /// step. `replace`: this kind of stamp added earlier is taken off those pages first.
    pub fn stamp_pages(&self, pages: &[StampPage], kind: &str, behind: bool, replace: bool) -> Result<usize, String> {
        // Pictures are read once, whatever the number of pages.
        let mut images = HashMap::new();
        for part in pages.iter().flat_map(|p| &p.parts) {
            if let Some(path) = &part.image {
                if !images.contains_key(path) {
                    images.insert(path.clone(), std::fs::read(path).map_err(|e| format!("Couldn't read {}: {e}", file_name(path)))?);
                }
            }
        }
        let mut inner = self.inner.lock();
        if !inner.doc.is_pdf() {
            return Err("Only PDFs can be stamped. Save this file as a PDF first (Save as PDF).".into());
        }
        for p in pages {
            self.check(p.page)?;
        }
        inner.doc.begin_op(if kind == "Watermark" { "Watermark" } else { "Headers and page numbers" });
        let res = (|| -> Result<(), String> {
            for p in pages {
                let page = self.page(&mut inner, p.page)?;
                if replace {
                    page.remove_stamps(kind)?;
                }
                if !p.parts.is_empty() {
                    page.stamp(&p.parts, &images, kind, behind)?;
                }
            }
            Ok(())
        })();
        if let Err(e) = res {
            inner.doc.abandon_op();
            return Err(e);
        }
        inner.doc.end_op();
        drop(inner);
        self.content_changed();
        Ok(pages.len())
    }

    /// Takes stamps of one kind off every page (or the ones listed), one undo step.
    /// Returns the pages that had some.
    pub fn remove_stamps(&self, kind: &str, pages: Option<Vec<usize>>) -> Result<usize, String> {
        let mut inner = self.inner.lock();
        if !inner.doc.is_pdf() {
            return Ok(0);
        }
        let list = pages.unwrap_or_else(|| (0..self.page_count()).collect());
        let mut count = 0;
        inner.doc.begin_op(if kind == "Watermark" { "Remove watermark" } else { "Remove headers and page numbers" });
        let res = (|| -> Result<(), String> {
            for &i in &list {
                self.check(i)?;
                let page = self.page(&mut inner, i)?;
                if page.remove_stamps(kind)? {
                    count += 1;
                }
            }
            Ok(())
        })();
        if res.is_err() || count == 0 {
            inner.doc.abandon_op();
            return res.map(|_| 0);
        }
        inner.doc.end_op();
        drop(inner);
        self.content_changed();
        Ok(count)
    }

    // ------------------------------------------------------------ inserting pages

    /// Adds pages at the end of the document as one undo step: pages of other
    /// files, or blank pages. The app puts them in place through the page order.
    fn insert(&self, name: &str, add: impl FnOnce(&Document) -> Result<(), String>) -> Result<Inserted, String> {
        let mut inner = self.inner.lock();
        if !inner.doc.is_pdf() {
            return Err("Pages can only be added to a PDF. Save this file as a PDF first (Save as PDF).".into());
        }
        let first = self.page_count();
        inner.doc.begin_op(name);
        if let Err(e) = add(&inner.doc) {
            inner.doc.abandon_op();
            return Err(e);
        }
        inner.doc.end_op();
        self.resync_pages(&mut inner)?;
        drop(inner);
        // Background jobs (search, Read mode) must read these pages from memory.
        self.content_changed();
        let count = self.page_count().saturating_sub(first);
        let sizes = (first..first + count).map(|i| self.bounds(i).map(|b| [b[2] - b[0], b[3] - b[1]])).collect::<Result<_, _>>()?;
        Ok(Inserted { first, count, sizes })
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
                self.took_out.store(true, Ordering::Release);
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
        let n = match res {
            Ok(n) => {
                inner.doc.end_op();
                n
            }
            Err(e) => {
                // Nothing of a half-made comment stays, and no undo step is left that the app doesn't know of.
                inner.doc.abandon_op();
                drop(page);
                inner.pages.clear();
                drop(inner);
                self.edited();
                return Err(e);
            }
        };
        self.edited();
        Ok((n, FormChange { accepted: true, changed: inner.doc.has_changes() }))
    }

    pub fn text(&self, index: usize) -> Result<Arc<Mutex<Text>>, String> {
        if let Some(t) = self.texts.lock().get(index) {
            return Ok(t);
        }
        let rev = self.edit_rev.load(Ordering::Acquire);
        let t = Arc::new(Mutex::new(self.list(index, 0)?.text()?));
        // Don't keep text read just before an edit changed the page (as list() does for its lists).
        if self.edit_rev.load(Ordering::Acquire) == rev {
            self.texts.lock().put(index, t.clone());
        }
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
        let repaired = pdfhtml::is_rebuilt(&path);
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
            converted: self.converted.read().clone(),
            repaired,
            not_saved: None,
        };
        if needs_password {
            return info;
        }
        info.reflowable = doc.is_reflowable();
        info.format = doc.metadata("format").unwrap_or_default();
        info.title = doc.metadata("info:Title").unwrap_or_default();
        info.author = doc.metadata("info:Author").unwrap_or_default();
        drop(inner);
        // A book drawn again as a PDF keeps the title and author its EPUB gave.
        if info.repaired {
            if let Some(c) = &info.converted {
                (info.title, info.author) = pdfhtml::title_author(&c.from);
            }
        }
        if let Ok(b) = self.bounds(0) {
            info.first_size = [b[2] - b[0], b[3] - b[1]];
        }
        info
    }

    /// Runs after the document is readable (opened or unlocked).
    fn prepare(&self) -> Result<(), String> {
        let inner = self.inner.lock();
        layout_reflowable(&inner.doc, &self.path())?;
        let n = inner.doc.page_count()?;
        self.page_count.store(n, Ordering::Relaxed);
        self.file_pages.store(n, Ordering::Relaxed);
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
    /// Makes PDFs of Office files (set by the app; none in tests).
    converter: RwLock<Option<Arc<Converter>>>,
}

impl Engine {
    pub fn new(workers: usize, store_bytes: usize) -> Result<Arc<Self>, String> {
        mupdf::init(store_bytes)?;
        Ok(Arc::new(Engine {
            pool: Pool::new(workers, Arc::new(purge_private_docs)),
            docs: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            converter: RwLock::new(None),
        }))
    }

    pub fn set_converter(&self, f: Arc<Converter>) {
        *self.converter.write() = Some(f);
    }

    /// The file to read for `path`: a PDF made from it when it's an Office file (with `engine`, or the
    /// preferred converter), or the file itself.
    fn source(&self, path: &str, engine: Option<&str>) -> Result<(String, Option<Converted>), String> {
        let convert = self.converter.read().clone();
        match convert.map(|f| f(path, engine)).transpose()?.flatten() {
            Some(c) => Ok((c.pdf.clone(), Some(c))),
            None => match pdfhtml::as_pdf(path, &mupdf::temp_dir()) {
                // EPUBs that are PDFs in disguise open as the PDF they were made from (pdfhtml.rs).
                Some(pdf) => Ok((pdf.clone(), Some(Converted { from: path.to_string(), engine: "pdf2htmlex".into(), engine_name: "Paginaut".into(), pdf }))),
                None => Ok((path.to_string(), None)),
            },
        }
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
        self.open_with(path, None)
    }

    /// A Markdown file shown from `text` (being edited) rather than from disk: it keeps the file's path (and reads
    /// pictures from its folder), and background jobs read it from memory.
    pub fn open_markdown(&self, path: &str, text: &str) -> Result<DocInfo, String> {
        let dir = std::path::Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let doc = Document::open_buffer("md", text.as_bytes(), &dir)?;
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
            content_edited: AtomicBool::new(true),
            file_pages: AtomicUsize::new(0),
            converted: RwLock::new(None),
            backing: Mutex::new(None),
            took_out: AtomicBool::new(false),
        });
        d.prepare()?;
        self.docs.write().insert(id, d.clone());
        Ok(d.info())
    }

    /// Opens a file; an Office file through a PDF made with `engine` (or the preferred converter).
    pub fn open_with(&self, path: &str, engine: Option<&str>) -> Result<DocInfo, String> {
        let (file, converted) = self.source(path, engine)?;
        let doc = Document::open(&file)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let d = Arc::new(Doc {
            id,
            path: RwLock::new(file),
            inner: Mutex::new(DocInner { doc, pages: Lru::new(32) }),
            page_count: AtomicUsize::new(0),
            bounds: RwLock::new(Vec::new()),
            lists: Mutex::new(Lru::new(96)),
            texts: Mutex::new(Lru::new(24)),
            password: Mutex::new(None),
            bg_gen: AtomicU64::new(0),
            edit_rev: AtomicU64::new(0),
            content_edited: AtomicBool::new(false),
            file_pages: AtomicUsize::new(0),
            converted: RwLock::new(converted),
            backing: Mutex::new(None),
            took_out: AtomicBool::new(false),
        });
        let locked = d.inner.lock().doc.needs_password();
        if !locked {
            d.prepare()?;
        }
        self.docs.write().insert(id, d.clone());
        Ok(d.info())
    }

    /// Saves a repaired e-book (see `DocInfo::repaired`): as the PDF it is shown as (`pdf`), or as an
    /// ordinary, flowing EPUB.
    pub fn save_repaired(&self, id: DocId, target: &str, pdf: bool) -> Result<(), String> {
        let d = self.doc(id)?;
        let from = d.converted.read().as_ref().filter(|_| pdfhtml::is_rebuilt(&d.path())).map(|c| c.from.clone());
        let from = from.ok_or("This e-book didn't need repairing.")?;
        if !pdf {
            return pdfhtml::repaired_epub(&from, target);
        }
        sandbox::copy_file(&d.path(), target)
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
            // A copy a failed save kept: its changes weren't wanted (closed without saving).
            if let Some(kept) = d.backing.lock().take() {
                if let Ok(doc) = Document::new_pdf() {
                    d.inner.lock().doc = doc;
                }
                let _ = std::fs::remove_file(kept);
            }
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
        self.save_with(id, order, rot, target, SaveOptions::default())
    }

    /// The document's protection.
    pub fn security(&self, id: DocId) -> Result<Security, String> {
        let d = self.doc(id)?;
        let pw = d.password.lock().clone().unwrap_or_default();
        let inner = d.inner.lock();
        if !inner.doc.is_pdf() {
            return Ok(Security { owner: true, ..Default::default() });
        }
        let json = inner.doc.security_json(&pw)?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    }

    /// Checks the owner (permissions) password; when right, it's kept so the protection can be changed.
    pub fn check_owner(&self, id: DocId, candidate: &str) -> Result<bool, String> {
        let d = self.doc(id)?;
        let cur = d.password.lock().clone().unwrap_or_default();
        let ok = d.inner.lock().doc.check_owner(candidate, &cur);
        if ok {
            *d.password.lock() = Some(candidate.to_string());
        }
        Ok(ok)
    }

    /// Saves as `save` does, and can also protect, compress or rewrite the file (always a full rewrite then).
    pub fn save_with(&self, id: DocId, order: Vec<usize>, rot: Vec<i32>, target: Option<String>, opts: SaveOptions) -> Result<DocInfo, String> {
        let d = self.doc(id)?;
        if opts.any() {
            if !d.inner.lock().doc.is_pdf() {
                return Err("Only PDFs can be protected, compressed or repaired. Save this file as a PDF first (Save as PDF).".into());
            }
            if opts.protect.is_some() && !self.security(id)?.owner {
                return Err("Enter the permissions password first: only its owner can change how this file is protected.".into());
            }
        }
        self.release_private_doc(id);
        let cur = d.path();
        let out = target.unwrap_or_else(|| cur.clone());
        let in_place = same_file(&out, &cur);
        if in_place && d.converted.read().is_some() {
            return Err("This PDF was made from an Office file. Use Save as PDF to keep it.".into());
        }
        let backing = d.backing.lock().clone();
        // Never the file a kept copy is (the open document reads it).
        let tmp = [format!("{cur}.paginaut-tmp"), format!("{cur}.paginaut-tmp2")].into_iter().find(|t| backing.as_deref() != Some(t.as_str())).unwrap_or_default();
        // The passwords that open the saved file (new ones when the protection changes; the owner's keeps it changeable).
        let new_pw = match &opts.protect {
            Some(p) if p.remove => None,
            Some(p) => Some(if !p.owner.is_empty() { p.owner.clone() } else { p.user.clone() }).filter(|pw| !pw.is_empty()),
            None => d.password.lock().clone(),
        };
        let open_saved = |path: &str| -> Result<Document, String> {
            let doc = Document::open(path)?.unlocked_with(new_pw.as_deref());
            Ok(doc)
        };
        let mut inner = d.inner.lock();
        inner.pages.clear();
        d.lists.lock().clear();
        d.texts.lock().clear();
        if !inner.doc.is_pdf() {
            if in_place {
                return Err("This kind of file can't be saved in place. Use Save as PDF.".into());
            }
            // Drawn into a new PDF: the document in memory isn't changed, so a failure leaves it as it was.
            write_beside(&out, |t| inner.doc.convert_to_pdf(t, &order, &rot))?;
        } else {
            // Page order and rotations, a rebuilt page tree and smaller pictures are put into the document itself before
            // it's written: as one step of the undo journal, which is taken back if writing fails, so the document is
            // as it was before Save (unsaved changes and their undo steps kept). Without a journal (no edits but page
            // order and rotations, which the app keeps) the file is simply opened again.
            let start = inner.doc.journal_pos();
            let file_pages = d.file_pages.load(Ordering::Relaxed);
            // Pages added in memory and then removed again would still be appended to the file; a full rewrite leaves them out.
            let kept: std::collections::HashSet<usize> = order.iter().copied().collect();
            let unused = (file_pages..d.page_count()).any(|i| !kept.contains(&i));
            // Privacy: a deleted page, and whatever else was taken out (see `took_out`), must not stay in the file's
            // earlier bytes, so the file is written afresh. Except a signed file: its signatures cover those very
            // bytes, and only adding to the file keeps them valid.
            let deleted = (0..file_pages.min(d.page_count())).any(|i| !kept.contains(&i));
            let afresh = (deleted || d.took_out.load(Ordering::Acquire)) && !inner.doc.sigs_json().is_ok_and(|j| j.contains("\"signed\":true"));
            inner.doc.begin_op("Save");
            let prep = (|| -> Result<(), String> {
                inner.doc.apply_page_edits(&rot, &order, file_pages)?;
                // A damaged file's page tree is rebuilt, or the saved file can't be opened again.
                inner.doc.fix_page_tree()?;
                match opts.compress {
                    Some(1) => inner.doc.rewrite_images(150, 70, false),
                    Some(2) => inner.doc.rewrite_images(100, 55, true),
                    _ => Ok(()),
                }
            })();
            if let Err(e) = prep {
                inner.doc.abandon_op();
                drop(inner);
                d.after_failed_save(start)?;
                return Err(e);
            }
            inner.doc.end_op();
            let encrypt = opts.protect.as_ref().map(|p| if p.remove { None } else { Some((p.owner.as_str(), p.user.as_str(), p.perms)) });
            let compact = opts.compress.is_some();
            // A full write is never made from the open document itself: MuPDF tidies the document it writes (drops unused
            // objects, renumbers them when compacting, marks everything saved), which a failed save couldn't take back. A
            // plain copy goes to a temporary file first, and the full write is made from that, opened on its own.
            let pw = d.password.lock().clone();
            let write = |doc: &Document, path: &str| -> Result<(), String> {
                let stage = temp_path(id, "save");
                let res = doc.save_copy(&stage).and_then(|_| {
                    let s = Document::open(&stage)?.unlocked_with(pw.as_deref());
                    if opts.any() { s.save_pdf_with(path, encrypt, compact) } else { s.save_pdf(path, false) }
                });
                let _ = std::fs::remove_file(&stage);
                res
            };
            let res = if !in_place {
                write_beside(&out, |t| write(&inner.doc, t))
            } else if !opts.any() && backing.is_none() && !unused && !afresh && inner.doc.can_save_incrementally() {
                // Checked first (another program can hold the file): MuPDF's own attempt, once started, leaves the
                // document in a state the undo journal doesn't fully take back.
                writable(&cur).and_then(|_| inner.doc.save_pdf(&cur, true))
            } else {
                // Written beside the file, then copied over it. The file must be writable first (another program can hold
                // it), since the copy is made after the open document has moved over to the new file.
                let ready = writable(&cur)
                    .and_then(|_| write(&inner.doc, &tmp))
                    .and_then(|_| open_saved(&tmp));
                match ready {
                    Err(e) => {
                        let _ = std::fs::remove_file(&tmp);
                        Err(e)
                    }
                    Ok(saved) => {
                        // From here the open document is the saved copy (and the original is no longer held open).
                        inner.doc = saved;
                        if let Err(e) = copy_with_retry(&tmp, &cur) {
                            // Everything is in the copy beside the file: kept, and the document goes on from it.
                            drop(inner);
                            let old = d.backing.lock().replace(tmp.clone());
                            if let Some(old) = old.filter(|o| *o != tmp) {
                                let _ = std::fs::remove_file(old);
                            }
                            if opts.protect.is_some() {
                                *d.password.lock() = new_pw.clone();
                            }
                            d.content_edited.store(true, Ordering::Release);
                            d.prepare()?;
                            let mut info = d.info();
                            info.not_saved = Some(format!(
                                "{} couldn't be replaced ({e}). Your changes are kept, in a copy beside it ({}) until a save works: save again.",
                                file_name(&cur),
                                file_name(&tmp)
                            ));
                            return Ok(info);
                        }
                        Ok(())
                    }
                }
            };
            if let Err(e) = res {
                drop(inner);
                d.after_failed_save(start)?;
                return Err(e);
            }
        }
        // Saved: the document is opened again from the saved file, so what's shown is what was saved.
        if opts.protect.is_some() {
            *d.password.lock() = new_pw.clone();
        }
        inner.doc = open_saved(&out)?;
        drop(inner);
        // The temporary copy and a kept one are closed now and can go.
        let _ = std::fs::remove_file(&tmp);
        if let Some(old) = d.backing.lock().take() {
            let _ = std::fs::remove_file(old);
        }
        *d.path.write() = out;
        *d.converted.write() = None;
        d.content_edited.store(false, Ordering::Release);
        d.took_out.store(false, Ordering::Release);
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
        if in_place && d.converted.read().is_some() {
            return Err("This PDF was made from an Office file. Use Save as PDF to keep it.".into());
        }
        {
            let inner = d.inner.lock();
            if !inner.doc.is_pdf() {
                return Err("Only PDFs can be signed. Save this file as a PDF first (Save as PDF).".into());
            }
            if inner.doc.has_changes() || d.backing.lock().is_some() {
                return Err("Save your changes first: a signature covers the document as it is saved.".into());
            }
            if !inner.doc.can_save_incrementally() {
                return Err("This file was damaged and repaired when it was opened, so it can't be signed as it is. Save a copy, open the copy and sign that.".into());
            }
        }
        if req.rect != [0.0; 4] {
            let [ox, oy, _, _] = d.bounds(index)?;
            for (i, v) in req.rect.iter_mut().enumerate() {
                *v += if i % 2 == 0 { ox } else { oy };
            }
        }
        // Signing writes into the file as it goes (the signature is made at the end of the save). It therefore goes
        // into a copy beside the target first, in place or not, so a failure (a cancelled PIN, an unreachable
        // timestamp service) never leaves the original with an empty signature in it, nor takes away a file that
        // was at the new name before.
        let write_to = format!("{out}.signing.tmp");
        std::fs::copy(&cur, &write_to).map_err(|e| format!("Couldn't write {write_to}: {e}"))?;
        let mut signing = mupdf::Signing { sign: Box::new(sign), error: None };
        let mut inner = d.inner.lock();
        // A file that says it's PDF/A stays so: the signature's look is made as that part wants.
        let pdfa = inner.doc.pdfa_part();
        let mut res = (|| -> Result<(), String> {
            let page = d.page(&mut inner, index)?;
            page.sign(&req, &mut signing, pdfa)?;
            drop(page);
            inner.pages.clear();
            inner.doc.save_pdf(&write_to, true)
        })()
        .map_err(|e| signing.error.take().unwrap_or(e));
        inner.pages.clear();
        d.lists.lock().clear();
        d.texts.lock().clear();
        if res.is_ok() {
            res = (|| -> Result<(), String> {
                if in_place {
                    // Let go of the original (the open document holds it) before the signed copy takes its place.
                    let bytes = std::fs::read(&write_to).map_err(|e| e.to_string())?;
                    inner.doc = Document::open_buffer("pdf", &bytes, "")?;
                }
                // A search or Read mode may still be reading the file for a moment: try for a while, as saving does.
                std::fs::rename(&write_to, &out)
                    .or_else(|_| copy_with_retry(&write_to, &out))
                    .map_err(|e| format!("Couldn't replace {} with the signed document: {e}", file_name(&out)))
            })();
        }
        let _ = std::fs::remove_file(&write_to);
        // Reopen from disk: the signed file, or on failure the untouched original.
        let reopen = if res.is_ok() { out.clone() } else { cur.clone() };
        let fresh = Document::open(&reopen)?.unlocked_with(d.password.lock().as_deref());
        inner.doc = fresh;
        drop(inner);
        if let Err(e) = res {
            d.prepare()?;
            return Err(e);
        }
        *d.path.write() = out;
        // Signed into a file of its own (in place is refused above for a PDF made from an Office file): from now on
        // it's a PDF in its own right, as after Save as.
        *d.converted.write() = None;
        d.edited();
        d.prepare()?;
        Ok(d.info())
    }

    /// Adds the pages of other files (PDFs, pictures, any format MuPDF opens) at
    /// the end of the document, as one undo step. A file that needs a password
    /// fails with "PASSWORD:<path>" (or "WRONG_PASSWORD:<path>").
    pub fn insert_pages(&self, id: DocId, sources: Vec<InsertSource>, fit: Option<[f32; 2]>) -> Result<Inserted, String> {
        let d = self.doc(id)?;
        let docs = self.open_sources(&sources)?;
        d.insert("Insert pages", |doc| {
            for (src, pages) in &docs {
                doc.insert_pages(src, pages, fit)?;
            }
            Ok(())
        })
    }

    /// Combines files into a new PDF at `target`: their pages in order and (with
    /// `bookmarks`) a bookmark per file holding that file's own bookmarks. Errors
    /// as for insert_pages. Returns the page count.
    pub fn merge(&self, sources: Vec<InsertSource>, fit: Option<[f32; 2]>, bookmarks: bool, target: &str) -> Result<usize, String> {
        let docs = self.open_sources(&sources)?;
        let out = Document::new_pdf()?;
        let mut base = 0;
        for ((src, pages), s) in docs.iter().zip(&sources) {
            let n = out.insert_pages(src, pages, fit)?;
            if bookmarks && n > 0 {
                let name = file_name(&s.path);
                out.outline_group(src, name.rsplit_once('.').map_or(name, |(stem, _)| stem), base, pages)?;
            }
            base += n;
        }
        if base == 0 {
            return Err("There are no pages to combine.".into());
        }
        write_beside(target, |tmp| out.save_pdf(tmp, false)).map(|_| base)
    }

    /// Copies pages of another open document (as they are in memory, unsaved
    /// changes included) to the end of this one, as one undo step.
    pub fn insert_from_open(&self, id: DocId, from: DocId, pages: Vec<usize>) -> Result<Inserted, String> {
        let s = self.doc(from)?;
        let password = s.password.lock().clone();
        let copy = {
            let inner = s.inner.lock();
            if inner.doc.is_pdf() && inner.doc.has_changes() {
                let tmp = temp_path(from, "copy");
                inner.doc.save_copy(&tmp)?;
                Some(tmp)
            } else {
                None
            }
        };
        let path = copy.clone().unwrap_or_else(|| s.file());
        let res = self.insert_pages(id, vec![InsertSource { path, pages: Some(pages), password }], None);
        if let Some(tmp) = copy {
            let _ = std::fs::remove_file(tmp);
        }
        res
    }

    /// Writes pages to new PDFs (extract, split), each with the page rotations
    /// `rot` (per document page). The document is taken as it is in memory, so
    /// unsaved changes are included; it's left as it is. Returns each file's size.
    pub fn save_parts(&self, id: DocId, parts: Vec<Part>, rot: Vec<i32>) -> Result<Vec<u64>, String> {
        let d = self.doc(id)?;
        let write = |path: &str, f: &dyn Fn(&str) -> Result<(), String>| -> Result<u64, String> {
            write_beside(path, f)?;
            Ok(std::fs::metadata(path).map(|m| m.len()).unwrap_or(0))
        };
        if !d.inner.lock().doc.is_pdf() {
            // Other formats are drawn into new PDFs.
            return parts.iter().map(|p| write(&p.path, &|out| d.inner.lock().doc.convert_to_pdf(out, &p.pages, &rot))).collect();
        }
        // Unsaved changes are written to a temporary copy; otherwise the file itself is read.
        let temp = {
            let inner = d.inner.lock();
            if inner.doc.has_changes() {
                let t = temp_path(id, "parts");
                inner.doc.save_copy(&t)?;
                Some(t)
            } else {
                None
            }
        };
        let copy = temp.clone().unwrap_or_else(|| d.file());
        let file_pages = d.file_pages.load(Ordering::Relaxed);
        let res = parts
            .iter()
            .map(|p| {
                write(&p.path, &|out| {
                    let doc = Document::open(&copy)?.unlocked_with(d.password.lock().as_deref());
                    doc.apply_page_edits(&rot, &p.pages, file_pages)?;
                    doc.save_pdf(out, false)
                })
            })
            .collect();
        if let Some(t) = temp {
            let _ = std::fs::remove_file(t);
        }
        res
    }

    /// Groups pages (document indices, in order) into parts of about `max_bytes`.
    pub fn split_by_size(&self, id: DocId, order: Vec<usize>, max_bytes: f64) -> Result<Vec<Vec<usize>>, String> {
        let d = self.doc(id)?;
        let inner = d.inner.lock();
        if !inner.doc.is_pdf() {
            return Err("Only PDFs can be split by size.".into());
        }
        let starts = inner.doc.split_by_size(&order, max_bytes)?;
        Ok(starts.iter().enumerate().map(|(i, &s)| order[s..starts.get(i + 1).copied().unwrap_or(order.len())].to_vec()).collect())
    }

    /// Size (points) of a file's first page: for a picture, its size at its own resolution.
    pub fn first_page_size(&self, path: &str) -> Result<[f32; 2], String> {
        let (file, _) = self.source(path, None)?;
        let d = Document::open(&file).map_err(|e| format!("Couldn't open {}: {e}", file_name(path)))?;
        layout_reflowable(&d, &file)?;
        let b = d.load_page(0)?.bounds()?;
        Ok([b[2] - b[0], b[3] - b[1]])
    }

    /// Page count and kind of a file, without keeping it open.
    pub fn peek(&self, path: &str, password: Option<&str>) -> Result<Peek, String> {
        let (file, _) = self.source(path, None)?;
        let d = Document::open(&file).map_err(|e| format!("Couldn't open {}: {e}", file_name(path)))?;
        let is_pdf = d.is_pdf();
        if d.needs_password() && !password.is_some_and(|pw| d.authenticate(pw)) {
            return Ok(Peek { pages: 0, is_pdf, locked: true });
        }
        layout_reflowable(&d, &file)?;
        Ok(Peek { pages: d.page_count()?, is_pdf, locked: false })
    }

    /// Writes the pages in `order` (turned by `rot`) with one of MuPDF's writers ("docx", "txt"…).
    pub fn write_document(&self, id: DocId, order: &[usize], rot: &[i32], path: &str, format: &str, options: &str) -> Result<(), String> {
        let d = self.doc(id)?;
        let inner = d.inner.lock();
        inner.doc.write_document(path, format, options, order, rot)
    }

    /// Makes a PDF of a file that isn't one (a web page, e-book, picture, XPS; an Office file through its converter).
    pub fn convert_file(&self, path: &str, out: &str) -> Result<(), String> {
        let (file, converted) = self.source(path, None)?;
        if converted.is_some() {
            return std::fs::copy(&file, out).map(drop).map_err(|e| e.to_string());
        }
        let d = Document::open(&file).map_err(|e| format!("Couldn't open {}: {e}", file_name(path)))?;
        if d.needs_password() {
            return Err(format!("{} is protected with a password: open it first.", file_name(path)));
        }
        if d.is_pdf() {
            return Err(format!("{} is already a PDF.", file_name(path)));
        }
        if d.is_reflowable() {
            // Web pages and e-books become A4 pages.
            d.layout(595.0, 842.0, 11.0)?;
        }
        let n = d.page_count()?;
        d.convert_to_pdf(out, &(0..n).collect::<Vec<_>>(), &vec![0; n])
    }

    /// Opens every file pages will be taken from (Office files through PDFs made from them) and checks the
    /// pages asked for, before anything is changed.
    fn open_sources(&self, sources: &[InsertSource]) -> Result<Vec<(Document, Vec<usize>)>, String> {
        let mut docs = Vec::with_capacity(sources.len());
        for s in sources {
            let (file, _) = self.source(&s.path, None)?;
            let src = Document::open(&file).map_err(|e| format!("Couldn't open {}: {e}", file_name(&s.path)))?;
            if src.needs_password() {
                match s.password.as_deref() {
                    None => return Err(format!("PASSWORD:{}", s.path)),
                    Some(pw) if !src.authenticate(pw) => return Err(format!("WRONG_PASSWORD:{}", s.path)),
                    _ => {}
                }
            }
            layout_reflowable(&src, &file)?;
            let n = src.page_count()?;
            let pages = match &s.pages {
                Some(p) => {
                    if let Some(bad) = p.iter().find(|&&i| i >= n) {
                        return Err(format!("{} has no page {} (it has {n})", file_name(&s.path), bad + 1));
                    }
                    p.clone()
                }
                None => (0..n).collect(),
            };
            docs.push((src, pages));
        }
        Ok(docs)
    }

    /// Adds `count` empty pages of w×h points at the end, as one undo step.
    pub fn insert_blank(&self, id: DocId, w: f32, h: f32, count: usize) -> Result<Inserted, String> {
        if !((1.0..=14400.0).contains(&w) && (1.0..=14400.0).contains(&h)) || count == 0 || count > 1000 {
            return Err("bad page size or count".into());
        }
        self.doc(id)?.insert("Insert blank page", |doc| doc.insert_blank(w, h, count).map(|_| ()))
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
        // The document as it is in memory goes to a temporary copy, which is flattened into a second one beside the
        // target: a failure leaves neither half a file nor a damaged target.
        let copy_path = temp_path(id, "flatten");
        d.inner.lock().doc.save_copy(&copy_path)?;
        let res = write_beside(&target, |out| {
            let copy = Document::open(&copy_path)?.unlocked_with(d.password.lock().as_deref());
            copy.apply_page_edits(&rot, &order, d.file_pages.load(Ordering::Relaxed))?;
            copy.bake(annots)?;
            copy.save_pdf(out, false)
        });
        let _ = std::fs::remove_file(&copy_path);
        res
    }

    /// Writes every page as a new PDF, leaving the open document as it is (printing
    /// an e-book or XPS file on Android, where the print service takes PDFs).
    pub fn export_pdf(&self, id: DocId, out: &str) -> Result<(), String> {
        let d = self.doc(id)?;
        let n = d.page_count();
        let res = d.inner.lock().doc.convert_to_pdf(out, &(0..n).collect::<Vec<_>>(), &vec![0; n]);
        res
    }

    /// The document's own page labels ("iv", "A-3"…), or None when they are
    /// just 1, 2, 3… (most files).
    pub fn page_labels(&self, id: DocId) -> Result<Option<Vec<String>>, String> {
        let d = self.doc(id)?;
        if !d.inner.lock().doc.is_pdf() {
            return Ok(None);
        }
        // From the catalog's /PageLabels alone: loading every page to ask its label was slow on long files and
        // pushed the viewer's pages out of the cache.
        let ranges = d.page_label_ranges()?;
        if ranges.is_empty() {
            return Ok(None);
        }
        let n = d.page_count();
        let labels: Vec<String> = (0..n).map(|i| label_of(&ranges, i)).collect();
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
        // Guarded: a fault while drawing becomes this page's error. Unguarded, the worker caught it but the answer
        // was never sent: the page stayed blank with no message and its request counted as running for good.
        self.pool.submit(rid, prio, move |ctl| done(guarded(req.page, || render_now(&d, &req, ctl))));
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
                            match guarded(i, || work(i, src)) {
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
                } else if let Err(e) = d.with_private(|doc| {
                    run(&PageSrc::Private(doc));
                    Ok(())
                }) {
                    // The file couldn't be opened again (moved, or gone): said, not taken for a finished scan.
                    error.lock().get_or_insert(e);
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

    /// Every page's words (see fo_text_words_json) and size, read in parallel; waits for them. `progress` gets the
    /// number of pages read so far. Engine::cancel(sid) stops it ("stopped").
    pub fn page_words(self: &Arc<Self>, doc: DocId, sid: u64, progress: impl Fn(usize) + Send + Sync + 'static) -> Result<Vec<(String, [f32; 2])>, String> {
        let n = self.doc(doc)?.page_count();
        let out: Arc<Mutex<Vec<(String, [f32; 2])>>> = Arc::new(Mutex::new(vec![(String::new(), [0.0; 2]); n]));
        let done = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = std::sync::mpsc::channel();
        let pages = out.clone();
        self.scan_pages(
            doc,
            0,
            sid,
            move |i, src| {
                let (list, b) = src.list(i)?;
                let json = list.text()?.words(&b)?;
                pages.lock()[i] = (json, [b[2] - b[0], b[3] - b[1]]);
                progress(done.fetch_add(1, Ordering::AcqRel) + 1);
                Ok(true)
            },
            |_, _| {},
            move |complete, _, error| {
                let _ = tx.send((complete, error));
            },
        )?;
        let (complete, error) = rx.recv().map_err(|_| "stopped".to_string())?;
        if let Some(e) = error {
            return Err(e);
        }
        if !complete {
            return Err("stopped".into());
        }
        let v = std::mem::take(&mut *out.lock());
        Ok(v)
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


/// Runs one page's part of a background job: a fault (a panic) in it becomes that page's error, so the job still
/// ends, says so, and lets go of its document copies instead of never reporting back.
pub(crate) fn guarded<T>(page: usize, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| Err(format!("Page {} couldn't be read: Paginaut hit a fault in it.", page + 1)))
}

/// A file in the temp folder for a copy of a document.
fn temp_path(id: DocId, what: &str) -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    std::env::temp_dir().join(format!("paginaut-{what}-{id}-{nanos}.pdf")).to_string_lossy().into_owned()
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// The two paths name the same file. Windows' file names ignore case; on Linux, Report.pdf and report.pdf are two
/// files, unless they're one file (the same inode, as on a case-insensitive disk or through a link).
#[cfg(windows)]
fn same_file(a: &str, b: &str) -> bool {
    let norm = |p: &str| {
        std::fs::canonicalize(p).map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_else(|_| p.replace('/', "\\").to_lowercase())
    };
    norm(a) == norm(b)
}

#[cfg(not(windows))]
fn same_file(a: &str, b: &str) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        // One isn't there (a new file): the same only when both name it alike.
        _ => {
            let norm = |p: &str| std::fs::canonicalize(p).map(|x| x.to_string_lossy().into_owned()).unwrap_or_else(|_| p.to_string());
            norm(a) == norm(b)
        }
    }
}

/// The file can be written now (no other program holds it).
fn writable(path: &str) -> Result<(), String> {
    std::fs::OpenOptions::new().write(true).open(path).map(drop).map_err(|e| format!("Couldn't write {}: {e}", file_name(path)))
}

/// Writes a file with `write` into a temporary file beside `target`, then puts it in its place, so a failure never
/// leaves half a file (or a damaged original) at `target`.
fn write_beside(target: &str, write: impl FnOnce(&str) -> Result<(), String>) -> Result<(), String> {
    let tmp = format!("{target}.paginaut-tmp");
    let res = write(&tmp).and_then(|_| std::fs::rename(&tmp, target).or_else(|_| copy_with_retry(&tmp, target)).map_err(|e| e.to_string()));
    let _ = std::fs::remove_file(&tmp);
    res
}

/// Page `i`'s label from the document's label ranges (see fo_pdf_page_label_ranges_json): the range's prefix and
/// its number in the range's style (D 1 2 3, R I II, r i ii, A A..Z AA.., a a..z aa..).
fn label_of(ranges: &[mupdf::LabelRange], i: usize) -> String {
    let Some(r) = ranges.iter().rev().find(|r| r.s <= i) else { return String::new() };
    let n = (r.n.max(1) as usize) + (i - r.s);
    let roman = |mut n: usize| {
        let mut s = String::new();
        for (v, t) in [(1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")] {
            while n >= v {
                s.push_str(t);
                n -= v;
            }
        }
        s
    };
    let letters = |n: usize| {
        let c = (b'a' + ((n - 1) % 26) as u8) as char;
        c.to_string().repeat((n - 1) / 26 + 1)
    };
    let num = match r.st.as_str() {
        "D" => n.to_string(),
        "r" => roman(n),
        "R" => roman(n).to_uppercase(),
        "a" => letters(n),
        "A" => letters(n).to_uppercase(),
        _ => String::new(),
    };
    format!("{}{num}", r.p)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn range(s: usize, st: &str, p: &str, n: i32) -> mupdf::LabelRange {
        mupdf::LabelRange { s, st: st.into(), p: p.into(), n }
    }

    #[test]
    fn labels_from_ranges() {
        let r = [range(0, "r", "", 1), range(4, "D", "", 1), range(10, "A", "App-", 1), range(40, "", "Cover", 1)];
        let got: Vec<String> = [0, 3, 4, 9, 10, 35, 36, 40].iter().map(|&i| label_of(&r, i)).collect();
        assert_eq!(got, ["i", "iv", "1", "6", "App-A", "App-Z", "App-AA", "Cover"]);
        assert_eq!(label_of(&[range(0, "D", "", 5)], 2), "7");
        assert_eq!(label_of(&[range(2, "D", "", 1)], 0), "");
        assert_eq!(label_of(&[range(0, "R", "", 1)], 1993), "MCMXCIV");
    }
}
