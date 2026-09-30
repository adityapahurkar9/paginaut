//! Safe wrappers over the C layer.
//!
//! Two kinds of MuPDF context exist on every thread, both created lazily:
//! - **Shared**: a clone of the global base context. All clones share one
//!   resource store (decoded images, fonts) and one set of locks, so objects
//!   can move between threads. Used for interactive viewing.
//! - **Private**: a fully independent context with its own store, font engine
//!   and glyph cache and no locks. Background jobs use it so they scale across
//!   cores instead of queueing on MuPDF's global locks. Objects made with it
//!   must be used and dropped on the thread that made them.
//!
//! Every handle remembers its kind and always uses the matching context.

use super::ffi::*;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

pub type Result<T> = std::result::Result<T, String>;

struct Ctx(*mut fo_ctx);

impl Drop for Ctx {
    fn drop(&mut self) {
        unsafe { fo_ctx_drop(self.0) }
    }
}

/// Store size for each thread's private context. Background jobs read text and
/// don't need decoded images cached, so this stays small.
const PRIVATE_STORE: usize = 8 << 20;

thread_local! {
    static CTX: Ctx = {
        let p = unsafe { fo_ctx_new() };
        assert!(!p.is_null(), "MuPDF is not initialised");
        Ctx(p)
    };
    static PRIVATE_CTX: Ctx = {
        let p = unsafe { fo_ctx_new_private(PRIVATE_STORE) };
        assert!(!p.is_null(), "could not create a private MuPDF context");
        Ctx(p)
    };
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Shared,
    Private,
}

fn with_kind<R>(kind: Kind, f: impl FnOnce(*mut fo_ctx) -> R) -> R {
    match kind {
        Kind::Shared => CTX.with(|c| f(c.0)),
        Kind::Private => PRIVATE_CTX.with(|c| f(c.0)),
    }
}

fn with_ctx<R>(f: impl FnOnce(*mut fo_ctx) -> R) -> R {
    with_kind(Kind::Shared, f)
}

/// For destructors: if the thread's context is already gone (thread exit),
/// skip the free; the context's own teardown reclaims its memory.
fn drop_with(kind: Kind, f: impl FnOnce(*mut fo_ctx)) {
    let _ = match kind {
        Kind::Shared => CTX.try_with(|c| f(c.0)),
        Kind::Private => PRIVATE_CTX.try_with(|c| f(c.0)),
    };
}

fn last_error(c: *mut fo_ctx) -> String {
    unsafe { CStr::from_ptr(fo_error(c)).to_string_lossy().into_owned() }
}

/// Takes ownership of a MuPDF-allocated C string.
fn take_string(c: *mut fo_ctx, p: *mut c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let s = unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() };
    unsafe { fo_free(c, p.cast()) };
    Some(s)
}

pub fn init(store_bytes: usize) -> Result<()> {
    if unsafe { fo_global_init(store_bytes) } == 0 {
        Ok(())
    } else {
        Err("could not initialise MuPDF".into())
    }
}

/// Frees `percent`% of the shared resource store (100 = empty it).
pub fn shrink_store(percent: i32) {
    with_ctx(|c| unsafe { fo_store_shrink(c, percent) })
}

/// Empties this thread's private store, if the thread has created one.
pub fn empty_private_store() {
    let _ = PRIVATE_CTX.try_with(|c| unsafe { fo_store_shrink(c.0, 100) });
}

#[cfg(windows)]
extern "C" {
    fn _heapmin() -> i32;
}

/// Asks the C runtime to hand free heap memory back to Windows. MuPDF frees
/// through the CRT heap, which otherwise keeps memory reserved after big jobs.
pub fn release_heap() {
    #[cfg(windows)]
    unsafe {
        _heapmin();
    }
}

// ---------------------------------------------------------------- Tesseract

/// A text recogniser with its language models loaded. It must stay on the
/// thread that made it.
pub struct Tess(*mut po_tess);

impl Tess {
    /// `langs`: "eng" or "eng+hin"; `files`: (language, model file path) for each.
    pub fn new(langs: &str, files: &[(String, String)]) -> Result<Self> {
        static INIT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if !*INIT.get_or_init(|| unsafe { fo_ocr_init() } == 0) {
            return Err("text recognition couldn't start".into());
        }
        let list: String = files.iter().map(|(l, p)| format!("{l}\t{p}\n")).collect();
        let langs = CString::new(langs).map_err(|e| e.to_string())?;
        let list = CString::new(list).map_err(|e| e.to_string())?;
        let mut err = [0u8; 256];
        let t = unsafe { po_tess_new(langs.as_ptr(), list.as_ptr(), err.as_mut_ptr().cast(), err.len() as c_int) };
        if t.is_null() {
            Err(unsafe { CStr::from_ptr(err.as_ptr().cast()) }.to_string_lossy().into_owned())
        } else {
            Ok(Tess(t))
        }
    }
}

impl Drop for Tess {
    fn drop(&mut self) {
        unsafe { po_tess_drop(self.0) }
    }
}

// ---------------------------------------------------------------- Cookie

pub struct Cookie(*mut fo_cookie);
unsafe impl Send for Cookie {}
unsafe impl Sync for Cookie {}

impl Cookie {
    pub fn new() -> Self {
        Cookie(unsafe { fo_cookie_new() })
    }
    pub fn abort(&self) {
        unsafe { fo_cookie_abort(self.0) }
    }
}

impl Drop for Cookie {
    fn drop(&mut self) {
        unsafe { fo_cookie_drop(self.0) }
    }
}

// ---------------------------------------------------------------- Document

/// One piece of a stamp: text (plain, laid out with shaping) or a picture, in
/// a box placed on the page by `matrix` (box space → page coordinates).
#[derive(serde::Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StampPart {
    #[serde(default)]
    pub text: Option<String>,
    /// A picture file (PNG, JPEG…).
    #[serde(default)]
    pub image: Option<String>,
    /// CSS font family: "sans-serif", "serif" or "monospace".
    #[serde(default)]
    pub font: Option<String>,
    #[serde(default)]
    pub size: f32,
    #[serde(default)]
    pub color: [f32; 3],
    #[serde(default)]
    pub bold: bool,
    /// "left", "center" or "right" within the box.
    #[serde(default)]
    pub align: Option<String>,
    pub width: f32,
    #[serde(default)]
    pub height: f32,
    /// Text: the box's top (0), middle (1) or bottom (2) is at y = 0.
    #[serde(default)]
    pub valign: u8,
    pub matrix: [f32; 6],
    #[serde(default = "full")]
    pub opacity: f32,
}

fn full() -> f32 {
    1.0
}

impl StampPart {
    /// The text as HTML and CSS for MuPDF's layout engine.
    fn html(&self) -> (String, String) {
        let text = self.text.as_deref().unwrap_or("");
        let mut esc = String::with_capacity(text.len() + 16);
        for ch in text.chars() {
            match ch {
                '&' => esc.push_str("&amp;"),
                '<' => esc.push_str("&lt;"),
                '>' => esc.push_str("&gt;"),
                '\n' => esc.push_str("<br/>"),
                _ => esc.push(ch),
            }
        }
        let align = match self.align.as_deref() {
            Some("center") => "center",
            Some("right") => "right",
            _ => "left",
        };
        let font = match self.font.as_deref() {
            Some("serif") => "serif",
            Some("monospace") => "monospace",
            _ => "sans-serif",
        };
        let [r, g, b] = self.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        let html = format!("<p>{esc}</p>");
        let css = format!(
            "p {{ margin: 0; text-align: {align}; line-height: 1.2; font-family: {font}; font-size: {}pt; color: rgb({r},{g},{b}); font-weight: {}; }}",
            self.size.max(1.0),
            if self.bold { "bold" } else { "normal" }
        );
        (html, css)
    }
}

/// A bookmark: title, page (document index, -1 for none), top of the view on it, and the ones inside it.
#[derive(serde::Deserialize, Clone)]
pub struct OutlineNode {
    pub t: String,
    pub p: i32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub k: Vec<OutlineNode>,
}

/// A range of page labels: from page index `s`, style ("D", "r", "R", "a", "A" or "" for none), prefix, first number.
#[derive(serde::Deserialize, serde::Serialize, Clone)]
pub struct LabelRange {
    pub s: usize,
    pub st: String,
    #[serde(default)]
    pub p: String,
    #[serde(default = "one_i32")]
    pub n: i32,
}

fn one_i32() -> i32 {
    1
}

/// How pages are resized (see fo_pdf_page_size).
#[derive(serde::Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct PageSizeOptions {
    /// 0 fit, 1 fill, 2 actual size.
    pub fit: u8,
    /// Where the content sits on a new page size: 0, 0.5 or 1 across and down.
    pub anchor: [f32; 2],
    /// Delete what's outside the crop box (otherwise it's only hidden).
    pub remove: bool,
    /// New space: 0 nothing, 1 colour, 2 edges stretched out.
    pub fill: u8,
    pub color: [f32; 3],
}

pub struct Document(*mut fo_doc, Kind);
unsafe impl Send for Document {}

impl Document {
    pub fn open(path: &str) -> Result<Self> {
        Self::open_kind(path, Kind::Shared)
    }

    /// Opens with this thread's private context (see module docs).
    pub fn open_private(path: &str) -> Result<Self> {
        Self::open_kind(path, Kind::Private)
    }

    /// A document from bytes (`magic`: its kind, like "md"), with what it links to read from the folder `dir`.
    pub fn open_buffer(magic: &str, data: &[u8], dir: &str) -> Result<Self> {
        let (m, d) = (CString::new(magic).map_err(|e| e.to_string())?, CString::new(dir).map_err(|e| e.to_string())?);
        with_kind(Kind::Shared, |c| {
            let doc = unsafe { fo_open_buffer(c, m.as_ptr(), data.as_ptr(), data.len(), d.as_ptr()) };
            if doc.is_null() {
                Err(last_error(c))
            } else {
                Ok(Document(doc, Kind::Shared))
            }
        })
    }

    /// Replaces the bookmarks with `items` (inside the current operation).
    pub fn set_outline(&self, items: &[OutlineNode]) -> Result<()> {
        fn add(c: *mut fo_ctx, d: *mut fo_doc, parent: c_int, items: &[OutlineNode], depth: usize) -> Result<()> {
            if depth > 32 {
                return Ok(());
            }
            for it in items {
                let title = CString::new(it.t.replace('\0', "")).unwrap_or_default();
                let n = unsafe { fo_pdf_outline_add(c, d, parent, title.as_ptr(), it.p, it.y) };
                if n < 0 {
                    return Err(last_error(c));
                }
                add(c, d, n, &it.k, depth + 1)?;
            }
            Ok(())
        }
        with_kind(self.1, |c| {
            if unsafe { fo_pdf_outline_clear(c, self.0) } != 0 {
                return Err(last_error(c));
            }
            add(c, self.0, 0, items, 0)
        })
    }

    /// The page label ranges (see fo_pdf_page_label_ranges_json).
    pub fn page_label_ranges_json(&self) -> Result<String> {
        with_kind(self.1, |c| take_string(c, unsafe { fo_pdf_page_label_ranges_json(c, self.0) }).ok_or_else(|| last_error(c)))
    }

    /// Replaces the page labels (inside the current operation).
    pub fn set_page_labels(&self, ranges: &[LabelRange]) -> Result<()> {
        let starts: Vec<c_int> = ranges.iter().map(|r| r.s as c_int).collect();
        let firsts: Vec<c_int> = ranges.iter().map(|r| r.n.max(1)).collect();
        let styles: Vec<c_char> = ranges.iter().map(|r| r.st.bytes().next().filter(|b| b"DrRaA".contains(b)).unwrap_or(0) as c_char).collect();
        let prefixes = CString::new(ranges.iter().map(|r| r.p.replace(['\n', '\0'], " ")).collect::<Vec<_>>().join("\n")).unwrap_or_default();
        with_kind(self.1, |c| {
            let rc = unsafe { fo_pdf_set_page_labels(c, self.0, starts.as_ptr(), styles.as_ptr(), prefixes.as_ptr(), firsts.as_ptr(), ranges.len() as c_int) };
            if rc == 0 { Ok(()) } else { Err(last_error(c)) }
        })
    }

    /// Sets a metadata entry ("info:Title"…); empty removes it.
    pub fn set_metadata(&self, key: &str, value: &str) -> Result<()> {
        let (k, v) = (CString::new(key).map_err(|e| e.to_string())?, CString::new(value.replace('\0', "")).unwrap_or_default());
        with_kind(self.1, |c| if unsafe { fo_set_metadata(c, self.0, k.as_ptr(), v.as_ptr()) } == 0 { Ok(()) } else { Err(last_error(c)) })
    }

    /// A new, empty PDF in memory (shared context).
    pub fn new_pdf() -> Result<Self> {
        with_ctx(|c| {
            let d = unsafe { fo_pdf_new(c) };
            if d.is_null() {
                Err(last_error(c))
            } else {
                Ok(Document(d, Kind::Shared))
            }
        })
    }

    fn open_kind(path: &str, kind: Kind) -> Result<Self> {
        let cpath = CString::new(path).map_err(|e| e.to_string())?;
        with_kind(kind, |c| {
            let d = unsafe { fo_open(c, cpath.as_ptr()) };
            if d.is_null() {
                Err(last_error(c))
            } else {
                Ok(Document(d, kind))
            }
        })
    }

    pub fn needs_password(&self) -> bool {
        with_kind(self.1, |c| unsafe { fo_needs_password(c, self.0) } != 0)
    }

    pub fn authenticate(&self, password: &str) -> bool {
        let pw = CString::new(password).unwrap_or_default();
        with_kind(self.1, |c| unsafe { fo_authenticate(c, self.0, pw.as_ptr()) } > 0)
    }

    pub fn is_reflowable(&self) -> bool {
        with_kind(self.1, |c| unsafe { fo_is_reflowable(c, self.0) } != 0)
    }

    pub fn layout(&self, w: f32, h: f32, em: f32) -> Result<()> {
        with_kind(self.1, |c| {
            if unsafe { fo_layout(c, self.0, w, h, em) } == 0 {
                Ok(())
            } else {
                Err(last_error(c))
            }
        })
    }

    pub fn is_pdf(&self) -> bool {
        with_kind(self.1, |c| unsafe { fo_is_pdf(c, self.0) } != 0)
    }

    /// Pages carrying text Paginaut recognised (read from the page objects).
    pub fn ocr_layer_count(&self) -> Result<usize> {
        with_kind(self.1, |c| match unsafe { fo_ocr_layer_count(c, self.0) } {
            n if n < 0 => Err(last_error(c)),
            n => Ok(n as usize),
        })
    }

    pub fn page_count(&self) -> Result<usize> {
        with_kind(self.1, |c| {
            let n = unsafe { fo_count_pages(c, self.0) };
            if n < 0 {
                Err(last_error(c))
            } else {
                Ok(n as usize)
            }
        })
    }

    pub fn metadata(&self, key: &str) -> Option<String> {
        let k = CString::new(key).ok()?;
        let mut buf = vec![0u8; 1024];
        with_kind(self.1, |c| {
            let n = unsafe { fo_metadata(c, self.0, k.as_ptr(), buf.as_mut_ptr().cast(), buf.len() as c_int) };
            if n <= 0 {
                return None;
            }
            let s = unsafe { CStr::from_ptr(buf.as_ptr().cast()) }.to_string_lossy().trim().to_string();
            (!s.is_empty()).then_some(s)
        })
    }

    pub fn outline_json(&self) -> Result<String> {
        with_kind(self.1, |c| take_string(c, unsafe { fo_outline_json(c, self.0) }).ok_or_else(|| last_error(c)))
    }

    pub fn load_page(&self, index: usize) -> Result<Page> {
        with_kind(self.1, |c| {
            let p = unsafe { fo_page_load(c, self.0, index as c_int) };
            if p.is_null() {
                Err(last_error(c))
            } else {
                Ok(Page(p, self.1))
            }
        })
    }

    /// Applies rotations (per original page) and a new page order to a PDF in
    /// memory. Pages from `file_pages` on were added since the file was opened.
    pub fn apply_page_edits(&self, rot: &[i32], order: &[usize], file_pages: usize) -> Result<()> {
        let order: Vec<c_int> = order.iter().map(|&x| x as c_int).collect();
        with_kind(self.1, |c| {
            let rc = unsafe {
                fo_pdf_apply_page_edits(c, self.0, rot.as_ptr(), rot.len() as c_int, order.as_ptr(), order.len() as c_int, file_pages as c_int)
            };
            if rc == 0 { Ok(()) } else { Err(last_error(c)) }
        })
    }

    pub fn can_save_incrementally(&self) -> bool {
        with_kind(self.1, |c| unsafe { fo_pdf_can_save_incrementally(c, self.0) } != 0)
    }

    /// Saves a PDF. `incremental` appends changes to this document's own file.
    pub fn save_pdf(&self, path: &str, incremental: bool) -> Result<()> {
        let p = CString::new(path).map_err(|e| e.to_string())?;
        with_kind(self.1, |c| {
            if unsafe { fo_pdf_save(c, self.0, p.as_ptr(), incremental as c_int) } == 0 { Ok(()) } else { Err(last_error(c)) }
        })
    }

    /// Protection as JSON (see fo_pdf_security_json).
    pub fn security_json(&self, password: &str) -> Result<String> {
        let pw = CString::new(password).unwrap_or_default();
        with_kind(self.1, |c| take_string(c, unsafe { fo_pdf_security_json(c, self.0, pw.as_ptr()) }).ok_or_else(|| last_error(c)))
    }

    /// Whether `candidate` is the owner (permissions) password; `current` is the one it was opened with.
    pub fn check_owner(&self, candidate: &str, current: &str) -> bool {
        let (a, b) = (CString::new(candidate).unwrap_or_default(), CString::new(current).unwrap_or_default());
        with_kind(self.1, |c| unsafe { fo_pdf_check_owner(c, self.0, a.as_ptr(), b.as_ptr()) } > 0)
    }

    /// For a file repaired on opening: rebuilds the page tree from the readable pages; returns how many were left out.
    pub fn fix_page_tree(&self) -> Result<usize> {
        with_kind(self.1, |c| {
            let n = unsafe { fo_pdf_fix_page_tree(c, self.0) };
            if n >= 0 { Ok(n as usize) } else { Err(last_error(c)) }
        })
    }

    /// Brings pictures down to `dpi`, photos saved as JPEG at `quality` (and lossless ones if `lossless_too`).
    pub fn rewrite_images(&self, dpi: i32, quality: i32, lossless_too: bool) -> Result<()> {
        with_kind(self.1, |c| if unsafe { fo_pdf_rewrite_images(c, self.0, dpi, quality, lossless_too as c_int) } == 0 { Ok(()) } else { Err(last_error(c)) })
    }

    /// Full rewrite: `encrypt` None keeps the protection, Some(None) removes it, Some(Some((owner, user, perms))) sets AES-256.
    pub fn save_pdf_with(&self, path: &str, encrypt: Option<Option<(&str, &str, i32)>>, compact: bool) -> Result<()> {
        let p = CString::new(path).map_err(|e| e.to_string())?;
        let (mode, owner, user, perms) = match encrypt {
            None => (-1, "", "", -1),
            Some(None) => (0, "", "", -1),
            Some(Some((o, u, perms))) => (1, o, u, perms),
        };
        let (o, u) = (CString::new(owner).map_err(|e| e.to_string())?, CString::new(user).map_err(|e| e.to_string())?);
        with_kind(self.1, |c| {
            let rc = unsafe { fo_pdf_save_with(c, self.0, p.as_ptr(), mode, perms, o.as_ptr(), u.as_ptr(), compact as c_int) };
            if rc == 0 { Ok(()) } else { Err(last_error(c)) }
        })
    }

    /// Writes the pages in `order` (turned by `rot`) with one of MuPDF's writers ("docx", "txt"…).
    pub fn write_document(&self, path: &str, format: &str, options: &str, order: &[usize], rot: &[i32]) -> Result<()> {
        let p = CString::new(path).map_err(|e| e.to_string())?;
        let (f, o) = (CString::new(format).map_err(|e| e.to_string())?, CString::new(options).map_err(|e| e.to_string())?);
        let order: Vec<c_int> = order.iter().map(|&x| x as c_int).collect();
        with_kind(self.1, |c| {
            let rc = unsafe { fo_write_document(c, self.0, p.as_ptr(), f.as_ptr(), o.as_ptr(), order.as_ptr(), order.len() as c_int, rot.as_ptr()) };
            if rc == 0 { Ok(()) } else { Err(last_error(c)) }
        })
    }

    /// Writes any document (EPUB, images, XPS…) as a PDF with the given order and rotations.
    pub fn convert_to_pdf(&self, path: &str, order: &[usize], rot: &[i32]) -> Result<()> {
        let p = CString::new(path).map_err(|e| e.to_string())?;
        let order: Vec<c_int> = order.iter().map(|&x| x as c_int).collect();
        with_kind(self.1, |c| {
            let rc = unsafe { fo_convert_to_pdf(c, self.0, p.as_ptr(), order.as_ptr(), order.len() as c_int, rot.as_ptr()) };
            if rc == 0 { Ok(()) } else { Err(last_error(c)) }
        })
    }

    /// Turns on undo (and form scripts if there are fields). Returns (fields, has XFA).
    pub fn forms_init(&self) -> Result<(usize, bool)> {
        let mut xfa: c_int = 0;
        with_kind(self.1, |c| {
            let n = unsafe { fo_pdf_forms_init(c, self.0, &mut xfa) };
            if n < 0 { Err(last_error(c)) } else { Ok((n as usize, xfa != 0)) }
        })
    }

    /// Undoes (or redoes) one edit: None when there was nothing to undo, else
    /// whether that step added, removed or resized pages.
    pub fn undo(&self, redo: bool) -> Result<Option<bool>> {
        with_kind(self.1, |c| match unsafe { fo_pdf_undo(c, self.0, redo as c_int) } {
            n if n < 0 => Err(last_error(c)),
            0 => Ok(None),
            n => Ok(Some(n == 2)),
        })
    }

    pub fn has_changes(&self) -> bool {
        with_kind(self.1, |c| unsafe { fo_pdf_has_changes(c, self.0) } != 0)
    }

    pub fn reset_form(&self) -> Result<()> {
        with_kind(self.1, |c| if unsafe { fo_pdf_reset_form(c, self.0) } < 0 { Err(last_error(c)) } else { Ok(()) })
    }

    /// Makes fields (and annotations, if asked) plain page content.
    pub fn bake(&self, annots: bool) -> Result<()> {
        with_kind(self.1, |c| if unsafe { fo_pdf_bake(c, self.0, annots as c_int) } < 0 { Err(last_error(c)) } else { Ok(()) })
    }

    /// Writes a full copy without cleaning up objects, leaving this document untouched.
    pub fn save_copy(&self, path: &str) -> Result<()> {
        let p = CString::new(path).map_err(|e| e.to_string())?;
        with_kind(self.1, |c| if unsafe { fo_pdf_save(c, self.0, p.as_ptr(), 2) } == 0 { Ok(()) } else { Err(last_error(c)) })
    }

    /// Everything between begin_op and end_op is one undo step.
    pub fn begin_op(&self, name: &str) {
        let n = CString::new(name).unwrap_or_default();
        with_kind(self.1, |c| unsafe { fo_pdf_begin_op(c, self.0, n.as_ptr()) })
    }

    pub fn end_op(&self) {
        with_kind(self.1, |c| unsafe { fo_pdf_end_op(c, self.0) })
    }

    /// Undoes everything since begin_op, leaving no undo step.
    pub fn abandon_op(&self) {
        with_kind(self.1, |c| unsafe { fo_pdf_abandon_op(c, self.0) })
    }

    /// Appends pages of `src` (any format; changed in memory, so open it just
    /// for this) to the end of this PDF, inside the current operation. `fit`
    /// scales pages of other formats (pictures…) onto pages of that size.
    pub fn insert_pages(&self, src: &Document, pages: &[usize], fit: Option<[f32; 2]>) -> Result<usize> {
        if self.1 != src.1 {
            return Err("documents from different contexts".into());
        }
        let pages: Vec<c_int> = pages.iter().map(|&p| p as c_int).collect();
        let fit = fit.unwrap_or([0.0, 0.0]);
        with_kind(self.1, |c| {
            let n = unsafe { fo_pdf_insert_pages(c, self.0, src.0, pages.as_ptr(), pages.len() as c_int, fit.as_ptr()) };
            if n < 0 { Err(last_error(c)) } else { Ok(n as usize) }
        })
    }

    /// Where parts of about `max_bytes` start, for pages in `order` (indices into it).
    pub fn split_by_size(&self, order: &[usize], max_bytes: f64) -> Result<Vec<usize>> {
        let order: Vec<c_int> = order.iter().map(|&p| p as c_int).collect();
        let mut starts = vec![0 as c_int; order.len().max(1)];
        with_kind(self.1, |c| {
            let n = unsafe { fo_pdf_split_by_size(c, self.0, order.as_ptr(), order.len() as c_int, max_bytes, starts.as_mut_ptr()) };
            if n < 0 {
                Err(last_error(c))
            } else {
                Ok(starts[..n as usize].iter().map(|&s| s as usize).collect())
            }
        })
    }

    /// Adds a bookmark `title` pointing at page `base`, with `src`'s own bookmarks
    /// under it for the copied pages (`pages[k]` of src is now page base + k).
    pub fn outline_group(&self, src: &Document, title: &str, base: usize, pages: &[usize]) -> Result<()> {
        let title = CString::new(title.replace('\0', "")).map_err(|e| e.to_string())?;
        let pages: Vec<c_int> = pages.iter().map(|&p| p as c_int).collect();
        with_kind(self.1, |c| {
            let rc = unsafe { fo_pdf_outline_group(c, self.0, src.0, title.as_ptr(), base as c_int, pages.as_ptr(), pages.len() as c_int) };
            if rc == 0 { Ok(()) } else { Err(last_error(c)) }
        })
    }

    /// Appends `count` empty pages of w×h points, inside the current operation.
    pub fn insert_blank(&self, w: f32, h: f32, count: usize) -> Result<usize> {
        with_kind(self.1, |c| {
            let n = unsafe { fo_pdf_insert_blank(c, self.0, w, h, count as c_int) };
            if n < 0 { Err(last_error(c)) } else { Ok(n as usize) }
        })
    }

    pub fn doc_annots_json(&self) -> Result<String> {
        with_kind(self.1, |c| take_string(c, unsafe { fo_doc_annots_json(c, self.0) }).ok_or_else(|| last_error(c)))
    }

    pub fn links_json(&self, page: &Page) -> Result<String> {
        with_kind(self.1, |c| take_string(c, unsafe { fo_links_json(c, self.0, page.0) }).ok_or_else(|| last_error(c)))
    }
}

/// What a new digital signature looks like and says.
pub struct SignRequest {
    /// An empty signature field of the form to sign in (its object number), or -1 for a new one at `rect`.
    pub field: i32,
    /// Page space; all zero for an invisible signature.
    pub rect: [f32; 4],
    /// CN, O, OU, email, country.
    pub dn: [String; 5],
    pub reason: String,
    pub location: String,
    /// A picture for the left half of the box (PNG/JPEG), e.g. your handwritten signature.
    pub image: Option<Vec<u8>>,
    /// PDF_SIGNATURE_SHOW_* flags.
    pub flags: i32,
    /// Lock every form field.
    pub lock: bool,
    /// Room reserved for the signature, in bytes.
    pub max: usize,
}

/// The signing step MuPDF calls back into during the save.
pub struct Signing<'a> {
    pub sign: Box<dyn FnMut(&[u8]) -> std::result::Result<Vec<u8>, String> + 'a>,
    /// Why it failed, when it did (MuPDF only reports a generic error).
    pub error: Option<String>,
}

extern "C" fn sign_trampoline(user: *mut std::ffi::c_void, data: *const u8, len: usize, out: *mut u8, cap: usize) -> isize {
    let st = unsafe { &mut *(user as *mut Signing) };
    let data = unsafe { std::slice::from_raw_parts(data, len) };
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (st.sign)(data)));
    match res {
        Ok(Ok(v)) if v.len() <= cap => {
            unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), out, v.len()) };
            v.len() as isize
        }
        Ok(Ok(v)) => {
            st.error = Some(format!("The signature ({} bytes) is larger than the room reserved for it ({cap} bytes).", v.len()));
            -1
        }
        Ok(Err(e)) => {
            st.error = Some(e);
            -1
        }
        Err(_) => {
            st.error = Some("Signing failed unexpectedly.".into());
            -1
        }
    }
}

impl Document {
    /// Signature fields as JSON (see fo_sigs_json).
    pub fn sigs_json(&self) -> Result<String> {
        with_kind(self.1, |c| take_string(c, unsafe { fo_sigs_json(c, self.0) }).ok_or_else(|| last_error(c)))
    }

    /// Pages with redaction marks: [(page, marks)].
    pub fn redaction_pages(&self) -> Result<Vec<(usize, usize)>> {
        let json = with_kind(self.1, |c| take_string(c, unsafe { fo_pdf_redaction_pages_json(c, self.0) }).ok_or_else(|| last_error(c)))?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    }

    /// Takes metadata, scripts, attachments, comments or hidden text out (see fo_pdf_sanitize).
    pub fn sanitize(&self, what: i32) -> Result<String> {
        with_kind(self.1, |c| take_string(c, unsafe { fo_pdf_sanitize(c, self.0, what) }).ok_or_else(|| last_error(c)))
    }

    /// Embedded files as JSON (see fo_attachments_json).
    pub fn attachments_json(&self) -> Result<String> {
        with_kind(self.1, |c| take_string(c, unsafe { fo_attachments_json(c, self.0) }).ok_or_else(|| last_error(c)))
    }

    /// Attaches a file to the document (inside the caller's undo step: the native side makes its own).
    pub fn attachment_add(&self, name: &str, mime: &str, data: &[u8], modified: i64) -> Result<()> {
        let (n, m) = (CString::new(name).map_err(|e| e.to_string())?, CString::new(mime).map_err(|e| e.to_string())?);
        with_kind(self.1, |c| {
            if unsafe { fo_pdf_attachment_add(c, self.0, n.as_ptr(), m.as_ptr(), data.as_ptr(), data.len(), modified) } == 0 { Ok(()) } else { Err(last_error(c)) }
        })
    }

    /// Removes attachment `i` (from the document's list, or its pin from its page).
    pub fn attachment_remove(&self, i: usize) -> Result<()> {
        with_kind(self.1, |c| if unsafe { fo_pdf_attachment_remove(c, self.0, i as c_int) } == 0 { Ok(()) } else { Err(last_error(c)) })
    }

    /// The contents of embedded file `i`.
    pub fn attachment_data(&self, i: usize) -> Result<Vec<u8>> {
        with_kind(self.1, |c| {
            let (mut d, mut len) = (std::ptr::null_mut(), 0usize);
            if unsafe { fo_attachment_data(c, self.0, i as c_int, &mut d, &mut len) } != 0 {
                return Err(last_error(c));
            }
            Ok(take_bytes(c, d, len))
        })
    }

    /// The bytes signature `i` covers, and its CMS blob.
    pub fn sig_data(&self, i: usize) -> Result<(Vec<u8>, Vec<u8>)> {
        with_kind(self.1, |c| {
            let (mut d, mut dl, mut s, mut sl) = (std::ptr::null_mut(), 0usize, std::ptr::null_mut(), 0usize);
            if unsafe { fo_sig_data(c, self.0, i as c_int, &mut d, &mut dl, &mut s, &mut sl) } != 0 {
                return Err(last_error(c));
            }
            let out = unsafe { (std::slice::from_raw_parts(d, dl).to_vec(), std::slice::from_raw_parts(s, sl).to_vec()) };
            unsafe {
                fo_free(c, d.cast());
                fo_free(c, s.cast());
            }
            Ok(out)
        })
    }
}

/// Copies `len` bytes the native side allocated, and frees them.
fn take_bytes(c: *mut fo_ctx, p: *mut u8, len: usize) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    let v = unsafe { std::slice::from_raw_parts(p, len).to_vec() };
    unsafe { fo_free(c, p.cast()) };
    v
}

impl Page {
    /// Replaces a paragraph's text (see fo_edit_text); coordinates relative to the page origin.
    #[allow(clippy::too_many_arguments)]
    pub fn edit_text(&self, lines: &[[f32; 4]], probe: [f32; 2], html: &str, css: &str, x: f32, baseline: f32, width: f32) -> Result<()> {
        let flat: Vec<f32> = lines.iter().flatten().copied().collect();
        let (h, s) = (CString::new(html).map_err(|e| e.to_string())?, CString::new(css).map_err(|e| e.to_string())?);
        with_kind(self.1, |c| {
            let rc = unsafe { fo_edit_text(c, self.0, flat.as_ptr(), lines.len() as c_int, probe.as_ptr(), h.as_ptr(), s.as_ptr(), x, baseline, width) };
            if rc == 0 { Ok(()) } else { Err(last_error(c)) }
        })
    }

    /// Applies this page's redaction marks (see fo_pdf_redact_apply): (marks, letters left under them).
    pub fn redact_apply(&self, cover: bool, rgb: [f32; 3], label: &str) -> Result<(usize, usize)> {
        let l = CString::new(label).unwrap_or_default();
        with_kind(self.1, |c| {
            let mut left: c_int = 0;
            match unsafe { fo_pdf_redact_apply(c, self.0, cover as c_int, rgb.as_ptr(), l.as_ptr(), &mut left) } {
                n if n < 0 => Err(last_error(c)),
                n => Ok((n as usize, left.max(0) as usize)),
            }
        })
    }

    /// Deletes (0), moves (1), replaces (2) or adds (3) a picture in the page's own content (see fo_pdf_picture_edit).
    pub fn picture_edit(&self, target: &[f32; 4], action: i32, dest: &[f32; 4], data: &[u8]) -> Result<()> {
        with_kind(self.1, |c| {
            let rc = unsafe { fo_pdf_picture_edit(c, self.0, target.as_ptr(), action, dest.as_ptr(), data.as_ptr(), data.len()) };
            if rc == 0 { Ok(()) } else { Err(last_error(c)) }
        })
    }

    /// This page as a one-page PDF showing only `area` (page coordinates), turned by `rot`.
    pub fn area_pdf(&self, area: &[f32; 4], rot: i32) -> Result<Vec<u8>> {
        with_kind(self.1, |c| {
            let (mut d, mut len) = (std::ptr::null_mut(), 0usize);
            if unsafe { fo_pdf_area_pdf(c, self.0, area.as_ptr(), rot, &mut d, &mut len) } != 0 {
                return Err(last_error(c));
            }
            Ok(take_bytes(c, d, len))
        })
    }

    /// Adds a signed signature field. The signature itself is made by
    /// `signing` during the next incremental save, which must happen while
    /// `signing` is alive.
    pub fn sign(&self, req: &SignRequest, signing: &mut Signing) -> Result<()> {
        let dn: Vec<CString> = req.dn.iter().map(|s| CString::new(s.as_str()).unwrap_or_default()).collect();
        let dn_ptrs: Vec<*const c_char> = dn.iter().map(|s| s.as_ptr()).collect();
        let reason = CString::new(req.reason.as_str()).unwrap_or_default();
        let location = CString::new(req.location.as_str()).unwrap_or_default();
        let (img, img_len) = req.image.as_deref().map_or((std::ptr::null(), 0), |b| (b.as_ptr(), b.len()));
        with_kind(self.1, |c| {
            let rc = unsafe {
                fo_sign_page(
                    c,
                    self.0,
                    req.field as c_int,
                    req.rect.as_ptr(),
                    sign_trampoline,
                    signing as *mut Signing as *mut std::ffi::c_void,
                    req.max,
                    dn_ptrs.as_ptr(),
                    reason.as_ptr(),
                    location.as_ptr(),
                    img,
                    img_len,
                    req.flags as c_int,
                    req.lock as c_int,
                )
            };
            if rc == 0 { Ok(()) } else { Err(last_error(c)) }
        })
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        drop_with(self.1, |c| unsafe { fo_doc_drop(c, self.0) })
    }
}

// ---------------------------------------------------------------- Page

/// Properties to set on an annotation (all optional; coordinates in page space).
#[derive(Default)]
pub struct AnnotEdit {
    pub rect: Option<[f32; 4]>,
    pub quads: Option<Vec<f32>>,
    pub ink: Option<Vec<Vec<f32>>>,
    pub line: Option<[f32; 4]>,
    pub line_endings: Option<[i32; 2]>,
    pub color: Option<Vec<f32>>,
    pub interior: Option<Vec<f32>>,
    pub opacity: Option<f32>,
    pub border: Option<f32>,
    pub icon: Option<String>,
    pub font: Option<(String, f32, Vec<f32>)>,
    /// PNG or JPEG bytes (signatures, image stamps).
    pub image: Option<Vec<u8>>,
    pub contents: Option<String>,
}

pub struct Page(*mut fo_page, Kind);
unsafe impl Send for Page {}
// Pages are only ever used while holding their document's lock (see
// `engine::Doc`), which serialises all access.
unsafe impl Sync for Page {}

impl Page {
    pub fn bounds(&self) -> Result<[f32; 4]> {
        let mut out = [0f32; 4];
        with_kind(self.1, |c| {
            if unsafe { fo_page_bounds(c, self.0, out.as_mut_ptr()) } == 0 {
                Ok(out)
            } else {
                Err(last_error(c))
            }
        })
    }

    /// Annotations on this page as JSON (see fo_annots_json).
    pub fn annots_json(&self, bounds: &[f32; 4]) -> Result<String> {
        with_kind(self.1, |c| take_string(c, unsafe { fo_annots_json(c, self.0, bounds.as_ptr()) }).ok_or_else(|| last_error(c)))
    }

    pub fn annot_create(&self, kind: &str, author: &str) -> Result<i32> {
        let k = CString::new(kind).map_err(|e| e.to_string())?;
        let a = CString::new(author).unwrap_or_default();
        with_kind(self.1, |c| match unsafe { fo_annot_create(c, self.0, k.as_ptr(), a.as_ptr()) } {
            n if n < 0 => Err(last_error(c)),
            n => Ok(n),
        })
    }

    /// Applies properties to annotation `num` (coordinates in page space), then redraws it.
    pub fn annot_apply(&self, num: i32, e: &AnnotEdit) -> Result<()> {
        with_kind(self.1, |c| {
            let ok = |rc: c_int| if rc < 0 { Err(last_error(c)) } else { Ok(()) };
            unsafe {
                if let Some(r) = &e.rect {
                    ok(fo_annot_set_rect(c, self.0, num, r.as_ptr()))?;
                }
                if let Some(q) = &e.quads {
                    ok(fo_annot_set_quads(c, self.0, num, q.as_ptr(), (q.len() / 8) as c_int))?;
                }
                if let Some(ink) = &e.ink {
                    let counts: Vec<c_int> = ink.iter().map(|s| (s.len() / 2) as c_int).collect();
                    let pts: Vec<f32> = ink.iter().flatten().copied().collect();
                    ok(fo_annot_set_ink(c, self.0, num, counts.as_ptr(), counts.len() as c_int, pts.as_ptr()))?;
                }
                if let Some(l) = &e.line {
                    let [s, t] = e.line_endings.unwrap_or([0, 0]);
                    ok(fo_annot_set_line(c, self.0, num, l.as_ptr(), s, t))?;
                }
                if let Some(col) = &e.color {
                    ok(fo_annot_set_color(c, self.0, num, col.len() as c_int, col.as_ptr()))?;
                }
                if let Some(col) = &e.interior {
                    ok(fo_annot_set_interior(c, self.0, num, col.len() as c_int, col.as_ptr()))?;
                }
                if let Some(o) = e.opacity {
                    ok(fo_annot_set_opacity(c, self.0, num, o))?;
                }
                if let Some(w) = e.border {
                    ok(fo_annot_set_border(c, self.0, num, w))?;
                }
                if let Some(icon) = &e.icon {
                    let s = CString::new(icon.as_str()).unwrap_or_default();
                    ok(fo_annot_set_icon(c, self.0, num, s.as_ptr()))?;
                }
                if let Some((font, size, rgb)) = &e.font {
                    let f = CString::new(font.as_str()).unwrap_or_default();
                    let rgb = [rgb.first().copied().unwrap_or(0.0), rgb.get(1).copied().unwrap_or(0.0), rgb.get(2).copied().unwrap_or(0.0)];
                    ok(fo_annot_set_font(c, self.0, num, f.as_ptr(), *size, rgb.as_ptr()))?;
                }
                if let Some(img) = &e.image {
                    ok(fo_annot_set_image(c, self.0, num, img.as_ptr(), img.len()))?;
                }
                if let Some(t) = &e.contents {
                    let s = CString::new(t.replace('\0', "")).unwrap_or_default();
                    ok(fo_annot_set_contents(c, self.0, num, s.as_ptr()))?;
                }
                ok(fo_annot_update(c, self.0, num))
            }
        })
    }

    pub fn annot_delete(&self, num: i32) -> Result<()> {
        with_kind(self.1, |c| if unsafe { fo_annot_delete(c, self.0, num) } < 0 { Err(last_error(c)) } else { Ok(()) })
    }

    /// Form fields on this page as JSON (see fo_widgets_json).
    pub fn widgets_json(&self, bounds: &[f32; 4]) -> Result<String> {
        with_kind(self.1, |c| take_string(c, unsafe { fo_widgets_json(c, self.0, bounds.as_ptr()) }).ok_or_else(|| last_error(c)))
    }

    /// Sets a text/choice field. Ok(false) if the form's own rules rejected the value.
    pub fn widget_set(&self, num: i32, value: &str) -> Result<bool> {
        let v = CString::new(value).map_err(|e| e.to_string())?;
        with_kind(self.1, |c| match unsafe { fo_widget_set(c, self.0, num, v.as_ptr()) } {
            n if n < 0 => Err(last_error(c)),
            n => Ok(n == 1),
        })
    }

    pub fn widget_toggle(&self, num: i32) -> Result<()> {
        with_kind(self.1, |c| if unsafe { fo_widget_toggle(c, self.0, num) } < 0 { Err(last_error(c)) } else { Ok(()) })
    }

    pub fn label(&self) -> String {
        let mut buf = vec![0u8; 64];
        with_kind(self.1, |c| unsafe { fo_page_label(c, self.0, buf.as_mut_ptr().cast(), buf.len() as c_int) });
        unsafe { CStr::from_ptr(buf.as_ptr().cast()) }.to_string_lossy().into_owned()
    }

    /// `extras`: 0 contents only, 1 with annotations and fields, 2 without fillable fields.
    pub fn display_list(&self, extras: u8) -> Result<List> {
        with_kind(self.1, |c| {
            let l = unsafe { fo_list_new(c, self.0, extras as c_int) };
            if l.is_null() {
                Err(last_error(c))
            } else {
                Ok(List(l, self.1))
            }
        })
    }
}

impl Page {
    /// Adds a recognised-text layer of `content` (empty: removes all such layers).
    /// `font`: the invisible font's object number to share, 0 to make one.
    pub fn ocr_apply(&self, content: &str, font: &mut i32) -> Result<()> {
        with_kind(self.1, |c| {
            if unsafe { fo_ocr_apply(c, self.0, content.as_ptr().cast(), content.len(), font) } == 0 {
                Ok(())
            } else {
                Err(last_error(c))
            }
        })
    }

    /// The box around everything drawn (relative to the page's top-left), None if blank.
    pub fn content_box(&self) -> Result<Option<[f32; 4]>> {
        let mut out = [0f32; 4];
        with_kind(self.1, |c| match unsafe { fo_page_content_box(c, self.0, out.as_mut_ptr()) } {
            n if n < 0 => Err(last_error(c)),
            0 => Ok(None),
            _ => Ok(Some(out)),
        })
    }

    /// Crops and/or resizes the page (see fo_pdf_page_size), inside the current operation.
    pub fn page_size(&self, crop: [f32; 4], size: Option<[f32; 2]>, o: &PageSizeOptions) -> Result<()> {
        let size = size.unwrap_or([0.0, 0.0]);
        with_kind(self.1, |c| {
            let rc = unsafe {
                fo_pdf_page_size(c, self.0, crop.as_ptr(), size.as_ptr(), o.fit as c_int, o.anchor[0], o.anchor[1], o.remove as c_int, o.fill as c_int, o.color.as_ptr())
            };
            if rc == 0 { Ok(()) } else { Err(last_error(c)) }
        })
    }

    /// Draws `parts` on the page as one stamp of `kind` ("Header", "Watermark"),
    /// behind or in front of its content, inside the current operation.
    /// `images` holds the bytes of picture files by path.
    pub fn stamp(&self, parts: &[StampPart], images: &std::collections::HashMap<String, Vec<u8>>, kind: &str, behind: bool) -> Result<()> {
        let kind = CString::new(kind).map_err(|e| e.to_string())?;
        with_kind(self.1, |c| {
            let st = unsafe { fo_stamp_begin(c, self.0) };
            if st.is_null() {
                return Err(last_error(c));
            }
            for part in parts {
                let rc = if let Some(path) = &part.image {
                    let data = images.get(path).ok_or("picture missing")?;
                    unsafe { fo_stamp_image(c, st, data.as_ptr(), data.len(), part.width, part.height, part.matrix.as_ptr(), part.opacity) }
                } else {
                    let (html, css) = part.html();
                    let (html, css) = (CString::new(html).unwrap_or_default(), CString::new(css).unwrap_or_default());
                    unsafe { fo_stamp_text(c, st, html.as_ptr(), css.as_ptr(), part.width, part.valign as c_int, part.matrix.as_ptr(), part.opacity) }
                };
                if rc != 0 {
                    let e = last_error(c);
                    unsafe { fo_stamp_drop(c, st) };
                    return Err(e);
                }
            }
            if unsafe { fo_stamp_end(c, st, kind.as_ptr(), behind as c_int) } == 0 { Ok(()) } else { Err(last_error(c)) }
        })
    }

    /// Removes the page's stamps of one kind; returns whether there were any.
    pub fn remove_stamps(&self, kind: &str) -> Result<bool> {
        let kind = CString::new(kind).map_err(|e| e.to_string())?;
        with_kind(self.1, |c| match unsafe { fo_stamp_remove(c, self.0, kind.as_ptr()) } {
            n if n < 0 => Err(last_error(c)),
            n => Ok(n > 0),
        })
    }

    pub fn has_ocr_layer(&self) -> bool {
        with_kind(self.1, |c| unsafe { fo_ocr_has_layer(c, self.0) }) == 1
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        drop_with(self.1, |c| unsafe { fo_page_drop(c, self.0) })
    }
}

// ---------------------------------------------------------------- Display list

/// Immutable recording of a page; safe to render from many threads at once.
pub struct List(*mut fo_list, Kind);
unsafe impl Send for List {}
unsafe impl Sync for List {}

pub enum RenderOutcome {
    Done,
    Aborted,
}

/// One file out of a zip archive (an Office file's parts); None when it has no such entry.
pub fn zip_read(path: &str, entry: &str) -> Result<Option<Vec<u8>>> {
    let (p, e) = (CString::new(path).map_err(|e| e.to_string())?, CString::new(entry).map_err(|e| e.to_string())?);
    with_ctx(|c| {
        let (mut d, mut len) = (std::ptr::null_mut(), 0usize);
        match unsafe { fo_zip_read(c, p.as_ptr(), e.as_ptr(), &mut d, &mut len) } {
            0 => Ok(Some(take_bytes(c, d, len))),
            1 => Ok(None),
            _ => Err(last_error(c)),
        }
    })
}

pub fn render_size(bounds: &[f32; 4], scale: f32, rotate: i32) -> (u32, u32) {
    let (mut w, mut h) = (0, 0);
    unsafe { fo_render_size(bounds.as_ptr(), scale, rotate, &mut w, &mut h) };
    (w.max(0) as u32, h.max(0) as u32)
}

impl List {
    /// Renders `clip` (device px, relative to the page's top-left) into `out` as RGBA.
    pub fn render(
        &self,
        bounds: &[f32; 4],
        scale: f32,
        rotate: i32,
        clip: [i32; 4],
        out: &mut [u8],
        cookie: Option<&Cookie>,
    ) -> Result<RenderOutcome> {
        let need = ((clip[2] - clip[0]).max(0) as usize) * ((clip[3] - clip[1]).max(0) as usize) * 4;
        if out.len() < need || need == 0 {
            return Err("render buffer too small".into());
        }
        with_kind(self.1, |c| {
            let rc = unsafe {
                fo_render(
                    c,
                    self.0,
                    bounds.as_ptr(),
                    scale,
                    rotate,
                    clip.as_ptr(),
                    out.as_mut_ptr(),
                    cookie.map_or(std::ptr::null_mut(), |k| k.0),
                )
            };
            match rc {
                0 => Ok(RenderOutcome::Done),
                1 => Ok(RenderOutcome::Aborted),
                _ => Err(last_error(c)),
            }
        })
    }

    /// Paragraphs on the page as JSON (see fo_paragraphs_json).
    pub fn paragraphs_json(&self, bounds: &[f32; 4]) -> Result<String> {
        with_kind(self.1, |c| take_string(c, unsafe { fo_paragraphs_json(c, self.0, bounds.as_ptr()) }).ok_or_else(|| last_error(c)))
    }

    /// Tables on the page as JSON (see fo_tables_json).
    pub fn tables_json(&self, bounds: &[f32; 4]) -> Result<String> {
        with_kind(self.1, |c| take_string(c, unsafe { fo_tables_json(c, self.0, bounds.as_ptr()) }).ok_or_else(|| last_error(c)))
    }

    /// The table in `area`, with these column lines if given (see fo_table_in_json).
    pub fn table_in_json(&self, bounds: &[f32; 4], area: &[f32; 4], cols: &[f32]) -> Result<String> {
        with_kind(self.1, |c| {
            take_string(c, unsafe { fo_table_in_json(c, self.0, bounds.as_ptr(), area.as_ptr(), cols.as_ptr(), cols.len() as c_int) }).ok_or_else(|| last_error(c))
        })
    }

    /// Pictures on the page as JSON (see fo_pictures_json).
    pub fn pictures_json(&self, bounds: &[f32; 4]) -> Result<String> {
        with_kind(self.1, |c| take_string(c, unsafe { fo_pictures_json(c, self.0, bounds.as_ptr()) }).ok_or_else(|| last_error(c)))
    }

    /// Picture `idx` as a file (fmt 0 as stored, 1 PNG, 2 PDF) and its extension.
    pub fn picture_data(&self, idx: usize, fmt: i32) -> Result<(Vec<u8>, String)> {
        with_kind(self.1, |c| {
            let (mut d, mut len, mut ext) = (std::ptr::null_mut(), 0usize, [0 as c_char; 8]);
            if unsafe { fo_picture_data(c, self.0, idx as c_int, fmt, &mut d, &mut len, ext.as_mut_ptr()) } != 0 {
                return Err(last_error(c));
            }
            let ext = unsafe { std::ffi::CStr::from_ptr(ext.as_ptr()) }.to_string_lossy().into_owned();
            Ok((take_bytes(c, d, len), ext))
        })
    }

    /// An area (relative to the page origin), turned by `rot`: PNG at `dpi`, or SVG.
    /// `kind`: 0 PNG, 1 SVG, 2 JPEG, 3 PNM (plain RGB).
    pub fn area_image(&self, bounds: &[f32; 4], area: &[f32; 4], rot: i32, dpi: f32, kind: i32) -> Result<Vec<u8>> {
        with_kind(self.1, |c| {
            let (mut d, mut len) = (std::ptr::null_mut(), 0usize);
            if unsafe { fo_area_image(c, self.0, bounds.as_ptr(), area.as_ptr(), rot, dpi, kind, &mut d, &mut len) } != 0 {
                return Err(last_error(c));
            }
            Ok(take_bytes(c, d, len))
        })
    }

    /// The text in an area (relative to the page origin).
    pub fn area_text(&self, bounds: &[f32; 4], area: &[f32; 4]) -> Result<String> {
        with_kind(self.1, |c| take_string(c, unsafe { fo_area_text(c, self.0, bounds.as_ptr(), area.as_ptr()) }).ok_or_else(|| last_error(c)))
    }

    /// Where images are drawn on the page: [x0, y0, x1, y1] relative to its origin.
    pub fn image_rects(&self, bounds: &[f32; 4]) -> Result<Vec<[f32; 4]>> {
        let json = with_kind(self.1, |c| {
            take_string(c, unsafe { fo_image_rects_json(c, self.0, bounds.as_ptr()) }).ok_or_else(|| last_error(c))
        })?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    }

    /// Page content for reflowing, as JSON (see fo_reflow_json).
    pub fn reflow(&self, bounds: &[f32; 4]) -> Result<String> {
        with_kind(self.1, |c| take_string(c, unsafe { fo_reflow_json(c, self.0, bounds.as_ptr()) }).ok_or_else(|| last_error(c)))
    }

    /// Share of the page (per 10,000) drawn but not text: pictures, text turned into outlines.
    pub fn untexted_ink(&self, bounds: &[f32; 4]) -> Result<u32> {
        with_kind(self.1, |c| {
            let n = unsafe { fo_ocr_ink(c, self.0, bounds.as_ptr()) };
            if n < 0 {
                Err(last_error(c))
            } else {
                Ok(n as u32)
            }
        })
    }

    /// Recognises what's drawn on the page but isn't text yet (see fo_ocr_page).
    /// `None` if the cookie aborted it.
    /// With `redo`, an earlier OCR layer's words are read again rather than left as they are.
    pub fn ocr(&self, bounds: &[f32; 4], dpi: f32, tess: &Tess, cookie: Option<&Cookie>, redo: bool) -> Result<Option<String>> {
        with_kind(self.1, |c| {
            let mut aborted = 0;
            let k = cookie.map_or(std::ptr::null_mut(), |k| k.0);
            let s = take_string(c, unsafe { fo_ocr_page(c, self.0, bounds.as_ptr(), dpi, tess.0, k, redo as c_int, &mut aborted) });
            if aborted != 0 {
                Ok(None)
            } else {
                s.map(Some).ok_or_else(|| last_error(c))
            }
        })
    }

    pub fn text(&self) -> Result<Text> {
        with_kind(self.1, |c| {
            let t = unsafe { fo_text_new(c, self.0) };
            if t.is_null() {
                Err(last_error(c))
            } else {
                Ok(Text(t, self.1))
            }
        })
    }
}

impl Drop for List {
    fn drop(&mut self) {
        drop_with(self.1, |c| unsafe { fo_list_drop(c, self.0) })
    }
}

// ---------------------------------------------------------------- Structured text

/// Structured text of one page. Read-only after creation; callers still
/// serialise access through a mutex to stay on the safe side of MuPDF.
pub struct Text(*mut fo_text, Kind);
unsafe impl Send for Text {}

pub type QuadF = [f32; 8];

impl Text {
    pub fn highlight(&self, bounds: &[f32; 4], a: [f32; 2], b: [f32; 2]) -> Vec<QuadF> {
        const MAX: usize = 4096;
        let mut out = vec![0f32; MAX * 8];
        let n = with_kind(self.1, |c| unsafe {
            fo_text_highlight(c, self.0, bounds.as_ptr(), a[0], a[1], b[0], b[1], out.as_mut_ptr(), MAX as c_int)
        });
        (0..n.max(0) as usize).map(|i| out[i * 8..i * 8 + 8].try_into().unwrap()).collect()
    }

    pub fn snap(&self, bounds: &[f32; 4], a: [f32; 2], b: [f32; 2], mode: i32) -> Option<QuadF> {
        let mut q = [0f32; 8];
        let n = with_kind(self.1, |c| unsafe { fo_text_snap(c, self.0, bounds.as_ptr(), a[0], a[1], b[0], b[1], mode, q.as_mut_ptr()) });
        (n > 0 && q.iter().any(|v| *v != 0.0)).then_some(q)
    }

    pub fn copy(&self, bounds: &[f32; 4], a: [f32; 2], b: [f32; 2]) -> String {
        with_kind(self.1, |c| take_string(c, unsafe { fo_text_copy(c, self.0, bounds.as_ptr(), a[0], a[1], b[0], b[1]) }))
            .unwrap_or_default()
    }

    pub fn all(&self) -> String {
        with_kind(self.1, |c| take_string(c, unsafe { fo_text_all(c, self.0) })).unwrap_or_default()
    }

    pub fn char_count(&self) -> usize {
        with_kind(self.1, |c| unsafe { fo_text_char_count(c, self.0) }).max(0) as usize
    }

    /// Search with options (see `SearchOptions`). Returns JSON: [{"q":[...], "s":"line"}].
    pub fn find(&self, bounds: &[f32; 4], needle: &str, flags: i32, snippets: bool) -> Result<String> {
        let n = CString::new(needle).map_err(|e| e.to_string())?;
        with_kind(self.1, |c| {
            take_string(c, unsafe { fo_text_match_json(c, self.0, bounds.as_ptr(), n.as_ptr(), flags, snippets as c_int) })
                .ok_or_else(|| last_error(c))
        })
    }

    /// The page's words in MuPDF's order, JSON (see fo_text_words_json).
    pub fn words(&self, bounds: &[f32; 4]) -> Result<String> {
        with_kind(self.1, |c| take_string(c, unsafe { fo_text_words_json(c, self.0, bounds.as_ptr()) }).ok_or_else(|| last_error(c)))
    }

    /// One `Vec` of quad coordinates (8 floats per quad) per hit.
    pub fn search(&self, bounds: &[f32; 4], needle: &str) -> Result<Vec<Vec<f32>>> {
        let n = CString::new(needle).map_err(|e| e.to_string())?;
        let json = with_kind(self.1, |c| {
            take_string(c, unsafe { fo_text_search_json(c, self.0, bounds.as_ptr(), n.as_ptr()) }).ok_or_else(|| last_error(c))
        })?;
        serde_json::from_str(&json).map_err(|e| e.to_string())
    }
}

impl Drop for Text {
    fn drop(&mut self) {
        drop_with(self.1, |c| unsafe { fo_text_drop(c, self.0) })
    }
}
