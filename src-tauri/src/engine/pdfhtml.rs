//! EPUBs that are PDFs in disguise. pdf2htmlEX (behind many converted textbooks) pins every line of
//! a PDF page in place with CSS (`position: absolute`, a class per left and bottom coordinate) and cuts
//! words into pieces with empty spacer spans that copy the PDF's letter spacing. MuPDF lays HTML out as
//! flowing text, so those pages come out as lines piled on one another and words split in two.
//!
//! Such a book opens as the PDF it was made from (`as_pdf`): every page drawn again from what the
//! EPUB holds (its lines where the CSS puts them, in its own fonts, over its page pictures), the way a
//! web browser shows it. `repaired_epub` makes an ordinary, flowing EPUB of it instead, for e-book
//! readers: the lines read in order with the spacers dropped, joined into paragraphs (a new one at an
//! indent, a wider gap or a change of type size), larger type as headings, the running heads and page
//! numbers left out, and the pages' pictures kept. The original file is never touched.

use super::mupdf::{zip_list, zip_read};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Pictures smaller than this are page backgrounds with nothing on them.
const MIN_PICTURE: usize = 20_000;

/// The PDF the book was made from (drawn again in `temp`), when `path` is an EPUB made by pdf2htmlEX;
/// None for any other file. Made once per file and run (see `clear`); a second open of the same book
/// while it's being made waits for it.
pub fn as_pdf(path: &str, temp: &Path) -> Option<String> {
    let key = book_key(path)?;
    #[cfg(unix)]
    if let Some(s) = super::sandbox::scratch() {
        return as_pdf_given(path, key, s);
    }
    let out = temp.join(format!("paginaut-book-{key:016x}.pdf"));
    if out.exists() {
        return Some(out.to_string_lossy().into_owned());
    }
    let lock = {
        let mut building = BUILDING.lock();
        match building.iter().find(|(k, _)| *k == key) {
            Some((_, l)) => l.clone(),
            None => {
                let l = std::sync::Arc::new(parking_lot::Mutex::new(()));
                building.push((key, l.clone()));
                l
            }
        }
    };
    let _one = lock.lock();
    if out.exists() {
        return Some(out.to_string_lossy().into_owned());
    }
    let script = Pinned::script(&Source::read(path)?, usize::MAX)?;
    let part = part_file(temp, "book", key);
    let made = super::mupdf::pinned_pdf(path, &script, &part.to_string_lossy()).is_ok() && std::fs::rename(&part, &out).is_ok();
    if !made {
        let _ = std::fs::remove_file(&part);
    }
    BUILDING.lock().retain(|(k, _)| *k != key);
    made.then(|| out.to_string_lossy().into_owned())
}

/// `as_pdf` in the isolated engine: the book is drawn into a file the app keeps by name (made once, like
/// in `temp`), held open while this engine runs.
#[cfg(unix)]
fn as_pdf_given(path: &str, key: u64, s: &dyn super::sandbox::Scratch) -> Option<String> {
    use super::sandbox::{fd_path, hold};
    use std::os::fd::AsRawFd;
    static MADE: parking_lot::Mutex<Vec<(u64, String)>> = parking_lot::Mutex::new(Vec::new());
    let name = format!("paginaut-book-{key:016x}.pdf");
    let lock = {
        let mut building = BUILDING.lock();
        match building.iter().find(|(k, _)| *k == key) {
            Some((_, l)) => l.clone(),
            None => {
                let l = std::sync::Arc::new(parking_lot::Mutex::new(()));
                building.push((key, l.clone()));
                l
            }
        }
    };
    let _one = lock.lock();
    let found = MADE.lock().iter().find(|(k, _)| *k == key).map(|(_, p)| p.clone());
    let out = found.or_else(|| {
        let made = match s.open(&name) {
            Some(fd) => Some(hold(fd, &name)),
            None => {
                let script = Pinned::script(&Source::read(path)?, usize::MAX)?;
                let fd = s.create(&name).ok()?;
                let ok = super::mupdf::pinned_pdf(path, &script, &fd_path(fd.as_raw_fd(), &name)).is_ok() && s.commit(&name).is_ok();
                ok.then(|| hold(fd, &name))
            }
        };
        if let Some(p) = &made {
            MADE.lock().push((key, p.clone()));
        }
        made
    });
    BUILDING.lock().retain(|(k, _)| *k != key);
    out
}

/// A book being made, and the lock a second open of it waits on.
static BUILDING: parking_lot::Mutex<Vec<(u64, std::sync::Arc<parking_lot::Mutex<()>>)>> = parking_lot::Mutex::new(Vec::new());

/// What a rebuilt book is known by: its path, size and date (a changed file is made again).
fn book_key(path: &str) -> Option<u64> {
    if !path.to_ascii_lowercase().ends_with(".epub") {
        return None;
    }
    let (len, mtime) = super::sandbox::stat(path)?;
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    // A handle's number changes with each open: the file's name stands for it.
    (super::sandbox::file_name(path), len, mtime).hash(&mut h);
    Some(h.finish())
}

/// A file being written, named for no one else to write at the same time.
fn part_file(temp: &Path, what: &str, key: u64) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    temp.join(format!("paginaut-{what}-{key:016x}-{}-{n}.part", std::process::id()))
}

/// For the shelf: a pdf2htmlEX book's first page alone as a PDF (in `temp`, removed by the caller), its title,
/// author and number of pages. Drawing one page is quick; the whole book is made only when it's opened.
pub struct Preview {
    pub pdf: String,
    pub title: String,
    pub author: String,
    pub pages: usize,
}

pub fn preview(path: &str, temp: &Path) -> Option<Preview> {
    let key = book_key(path)?;
    let src = Source::read(path)?;
    let script = Pinned::script(&src, 1)?;
    #[cfg(unix)]
    if let Some(s) = super::sandbox::scratch() {
        use std::os::fd::AsRawFd;
        let fd = s.anon().ok()?;
        super::mupdf::pinned_pdf(path, &script, &super::sandbox::fd_path(fd.as_raw_fd(), "cover.pdf")).ok()?;
        let pdf = super::sandbox::hold(fd, "cover.pdf");
        return Some(Preview { pdf, title: element_text(&src.opf, "dc:title"), author: element_text(&src.opf, "dc:creator"), pages: src.page_count() });
    }
    let part = part_file(temp, "cover", key);
    if super::mupdf::pinned_pdf(path, &script, &part.to_string_lossy()).is_err() {
        let _ = std::fs::remove_file(&part);
        return None;
    }
    let pdf = part.with_extension("pdf");
    std::fs::rename(&part, &pdf).ok()?;
    Some(Preview { pdf: pdf.to_string_lossy().into_owned(), title: element_text(&src.opf, "dc:title"), author: element_text(&src.opf, "dc:creator"), pages: src.page_count() })
}

/// Whether `path` (a document's file) is a book drawn again here.
pub fn is_rebuilt(path: &str) -> bool {
    Path::new(super::sandbox::file_name(path)).file_name().is_some_and(|n| {
        let n = n.to_string_lossy();
        n.starts_with("paginaut-book-") && n.ends_with(".pdf")
    })
}

/// The title and author an EPUB gives (empty when it gives none).
pub fn title_author(path: &str) -> (String, String) {
    let text = |entry: &str| zip_read(path, entry).ok().flatten().map(|b| String::from_utf8_lossy(&b).into_owned());
    let opf = text("META-INF/container.xml").and_then(|c| text(tags(&c, "rootfile").into_iter().find_map(|t| attr(t, "full-path"))?));
    opf.map(|o| (element_text(&o, "dc:title"), element_text(&o, "dc:creator"))).unwrap_or_default()
}

/// An ordinary, flowing EPUB at `target` made from the pdf2htmlEX EPUB at `path`.
pub fn repaired_epub(path: &str, target: &str) -> Result<(), String> {
    let book = Source::read(path).map(|s| Book::read(path, &s)).ok_or("This e-book isn't one made from a PDF.")?;
    super::sandbox::write_target(target, &pack(book.files()))
}

/// An EPUB file of these entries: "mimetype" and the container first, the rest by name, every entry
/// stored as it is (the pictures are compressed already).
fn pack(mut files: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let rank = |n: &str| match n {
        "mimetype" => 0,
        "META-INF/container.xml" => 1,
        _ => 2,
    };
    files.sort_by(|a, b| rank(&a.0).cmp(&rank(&b.0)).then_with(|| a.0.cmp(&b.0)));
    let mut zip = Vec::new();
    let mut central = Vec::new();
    for (name, data) in &files {
        let crc = crc32(&data);
        let at = zip.len() as u32;
        let (len, name_len) = (data.len() as u32, name.len() as u16);
        // Local header: signature, version 1.0, no flags, stored, no date, CRC, sizes, name.
        zip.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        zip.extend_from_slice(&[10, 0, 0, 0, 0, 0, 0, 0, 0x21, 0]);
        for v in [crc, len, len] {
            zip.extend_from_slice(&v.to_le_bytes());
        }
        zip.extend_from_slice(&name_len.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(name.as_bytes());
        zip.extend_from_slice(&data);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&[20, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0x21, 0]);
        for v in [crc, len, len] {
            central.extend_from_slice(&v.to_le_bytes());
        }
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&[0; 12]);
        central.extend_from_slice(&at.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let (start, size, n) = (zip.len() as u32, central.len() as u32, files.len() as u16);
    zip.extend_from_slice(&central);
    zip.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    zip.extend_from_slice(&[0, 0, 0, 0]);
    zip.extend_from_slice(&n.to_le_bytes());
    zip.extend_from_slice(&n.to_le_bytes());
    zip.extend_from_slice(&size.to_le_bytes());
    zip.extend_from_slice(&start.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    !data.iter().fold(!0u32, |c, &b| table[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8))
}

/// Removes books rebuilt by earlier runs.
pub fn clear(temp: &Path) {
    if let Ok(list) = std::fs::read_dir(temp) {
        for e in list.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("paginaut-book-") || name.starts_with("paginaut-cover-") {
                let _ = if e.path().is_dir() { std::fs::remove_dir_all(e.path()) } else { std::fs::remove_file(e.path()) };
            }
        }
    }
}

/// pdf2htmlEX's CSS: each class's declarations (screen rules only, not the print ones), and the
/// font files its `@font-face` rules name.
#[derive(Default)]
struct Css {
    rules: HashMap<String, String>,
    fonts: HashMap<String, String>,
}

impl Css {
    /// Reads a stylesheet (or a `<style>` element's text): rules for single classes, @font-face, and what's
    /// inside @media blocks for the screen; @media print blocks are skipped whole, wherever they are
    /// (pdf2htmlEX's base CSS has one in the middle).
    fn read(&mut self, css: &str, dir: &str) {
        // (Old pages wrap a <style>'s text in an HTML comment.)
        let css = strip_comments(css).replace("<!--", "").replace("-->", "");
        self.read_rules(&css, dir);
    }

    fn read_rules(&mut self, css: &str, dir: &str) {
        let mut rest = css;
        while let Some(open) = rest.find('{') {
            // What comes before a rule may end an earlier statement (`@charset "utf-8";`).
            let head = rest[..open].rsplit(';').next().unwrap_or("").trim();
            let Some(len) = block_len(&rest[open..]) else { break };
            let body = &rest[open + 1..open + len];
            rest = &rest[open + len + 1..];
            if let Some(media) = head.strip_prefix("@media") {
                let media = media.to_ascii_lowercase();
                if !media.contains("print") || media.contains("screen") || media.contains("all") {
                    self.read_rules(body, dir);
                }
                continue;
            }
            if head == "@font-face" {
                let family = decl(body, "font-family").map(|f| f.trim_matches(['"', '\''])).unwrap_or("");
                let url = body.find("url(").and_then(|i| body[i + 4..].split(')').next()).map(|u| u.trim().trim_matches(['"', '\'']));
                if let (false, Some(url)) = (family.is_empty(), url) {
                    self.fonts.insert(family.to_string(), resolve(dir, url));
                }
                continue;
            }
            if head.starts_with('@') {
                continue;
            }
            // Only single classes matter here (".x1d", or each of ".a,.b").
            for sel in head.split(',') {
                if let Some(name) = sel.trim().strip_prefix('.').filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')) {
                    let r = self.rules.entry(name.to_string()).or_default();
                    r.push_str(body);
                    r.push(';');
                }
            }
        }
    }

    fn px(&self, class: &str, prop: &str) -> Option<f32> {
        decl(self.rules.get(class)?, prop)?.strip_suffix("px")?.trim().parse().ok()
    }

    fn matrix(&self, class: &str) -> Option<[f32; 4]> {
        let t = decl(self.rules.get(class)?, "transform")?;
        let v: Vec<f32> = t.strip_prefix("matrix(")?.split([',', ')']).take(4).filter_map(|n| n.trim().parse().ok()).collect();
        (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
    }

    /// rgb(), rgba(), #rrggbb or transparent, as 0–1 values and an alpha.
    fn color(&self, class: &str) -> Option<[f32; 4]> {
        let c = decl(self.rules.get(class)?, "color")?;
        if c == "transparent" {
            return Some([0.0, 0.0, 0.0, 0.0]);
        }
        if let Some(hex) = c.strip_prefix('#').filter(|h| h.len() == 6) {
            let n = u32::from_str_radix(hex, 16).ok()?;
            return Some([((n >> 16) & 255) as f32 / 255.0, ((n >> 8) & 255) as f32 / 255.0, (n & 255) as f32 / 255.0, 1.0]);
        }
        let inner = c.strip_prefix("rgba(").or_else(|| c.strip_prefix("rgb("))?.trim_end_matches(')');
        let v: Vec<f32> = inner.split(',').filter_map(|n| n.trim().parse().ok()).collect();
        (v.len() >= 3).then(|| [v[0] / 255.0, v[1] / 255.0, v[2] / 255.0, v.get(3).copied().unwrap_or(1.0)])
    }

    fn hidden(&self, class: &str) -> bool {
        self.rules.get(class).and_then(|r| decl(r, "visibility")) == Some("hidden")
    }
}

/// The length of the block opening at `s[0]` (a `{`), up to its matching `}`.
fn block_len(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, b) in s.bytes().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn strip_comments(css: &str) -> std::borrow::Cow<'_, str> {
    if !css.contains("/*") {
        return css.into();
    }
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        rest = rest[i + 2..].find("*/").map_or("", |e| &rest[i + 2 + e + 2..]);
    }
    out.push_str(rest);
    out.into()
}

/// The text of every `<style>` element in a page.
fn style_elements(page: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = page;
    while let Some(i) = rest.find("<style") {
        let Some(open) = rest[i..].find('>').map(|e| i + e + 1) else { break };
        let close = rest[open..].find("</style>").map_or(rest.len(), |e| open + e);
        out.push(&rest[open..close]);
        rest = &rest[close..];
    }
    out
}

/// A pdf2htmlEX EPUB as read for drawing again or for flowing: its zip entries, package file, pages (each
/// spine entry and its text) and CSS (linked and in `<style>` elements). None for any other EPUB.
struct Source {
    names: HashSet<String>,
    opf: String,
    pages: Vec<(String, String)>,
    css: Css,
}

impl Source {
    fn read(path: &str) -> Option<Source> {
        let names: HashSet<String> = zip_list(path).ok()?.into_iter().collect();
        let text = |entry: &str| zip_read(path, entry).ok().flatten().map(|b| String::from_utf8_lossy(&b).into_owned());
        let container = text("META-INF/container.xml")?;
        let opf_entry = tags(&container, "rootfile").into_iter().find_map(|t| attr(t, "full-path"))?.to_string();
        let opf = text(&opf_entry)?;
        let base = folder(&opf_entry).to_string();
        let manifest: HashMap<&str, &str> = tags(&opf, "item").into_iter().filter_map(|t| Some((attr(t, "id")?, attr(t, "href")?))).collect();
        let spine: Vec<String> = tags(&opf, "itemref").into_iter().filter_map(|t| manifest.get(attr(t, "idref")?)).map(|h| resolve(&base, h)).collect();
        // Only books whose first pages are pinned (checked before reading the rest).
        if !spine.iter().take(3).filter_map(|e| text(e)).any(|p| is_pinned(&p)) {
            return None;
        }
        let mut css = Css::default();
        let mut css_read = HashSet::new();
        let pages: Vec<(String, String)> = spine.iter().filter_map(|e| Some((e.clone(), text(e)?))).collect();
        for (entry, page) in &pages {
            for link in tags(page, "link") {
                if let Some(href) = attr(link, "href").filter(|h| h.ends_with(".css")) {
                    let e = resolve(folder(entry), href);
                    if css_read.insert(e.clone()) {
                        if let Some(s) = text(&e) {
                            css.read(&s, folder(&e));
                        }
                    }
                }
            }
            for style in style_elements(page) {
                css.read(style, folder(entry));
            }
        }
        Some(Source { names, opf, pages, css })
    }

    /// The book's pages (pdf2htmlEX's `pf` boxes).
    fn page_count(&self) -> usize {
        self.pages.iter().map(|(_, p)| tags(p, "div").iter().filter(|t| attr(t, "class").is_some_and(|c| c.split_whitespace().next() == Some("pf"))).count()).sum()
    }
}

/// A standard font in place of one the book names but doesn't carry: its kind (serif, sans serif or fixed
/// width), weight and slant.
fn standard_font(rule: Option<&str>) -> &'static str {
    let get = |prop: &str| rule.and_then(|r| decl(r, prop)).unwrap_or("").to_ascii_lowercase();
    let family = get("font-family");
    let weight = get("font-weight");
    let bold = weight.contains("bold") || weight.parse::<u32>().is_ok_and(|w| w >= 600);
    let slant = matches!(get("font-style").as_str(), "italic" | "oblique");
    let fixed = ["mono", "courier", "consol"].iter().any(|k| family.contains(k));
    let sans = !fixed && ["sans", "arial", "helvetica", "verdana", "calibri"].iter().any(|k| family.contains(k));
    match (fixed, sans, bold, slant) {
        (true, _, false, false) => "Courier",
        (true, _, true, false) => "Courier-Bold",
        (true, _, false, true) => "Courier-Oblique",
        (true, _, true, true) => "Courier-BoldOblique",
        (_, true, false, false) => "Helvetica",
        (_, true, true, false) => "Helvetica-Bold",
        (_, true, false, true) => "Helvetica-Oblique",
        (_, true, true, true) => "Helvetica-BoldOblique",
        (_, _, false, false) => "Times-Roman",
        (_, _, true, false) => "Times-Bold",
        (_, _, false, true) => "Times-Italic",
        (_, _, true, true) => "Times-BoldItalic",
    }
}

/// The value of `prop: value` among a rule's declarations (the last one given).
fn decl<'a>(body: &'a str, prop: &str) -> Option<&'a str> {
    body.split(';').filter_map(|d| d.split_once(':')).filter(|(k, _)| k.trim() == prop).map(|(_, v)| v.trim()).last()
}

/// The class in `classes` made of `prefix` and a hex number ("x1d" for "x").
fn class_of<'a>(classes: &'a str, prefix: &str) -> Option<&'a str> {
    classes.split_whitespace().find(|c| c.len() > prefix.len() && c.starts_with(prefix) && c[prefix.len()..].bytes().all(|b| b.is_ascii_hexdigit()))
}

/// What a run of text is drawn with: from the line's classes, changed by spans inside it.
#[derive(Clone, PartialEq)]
struct Style {
    font: String,
    size: f32,
    color: [f32; 4],
    letter: f32,
    word: f32,
}

/// pdf2htmlEX's pages read for drawing again (fo_pinned_pdf's script).
struct Pinned<'a> {
    css: &'a Css,
    out: String,
    fonts: HashMap<String, usize>,
    /// Pages still to draw (the shelf's cover needs one).
    pages_left: usize,
}

impl Pinned<'_> {
    /// The instructions for the book's first `max_pages` pages; None when none could be drawn.
    fn script(src: &Source, max_pages: usize) -> Option<String> {
        let mut p = Pinned { css: &src.css, out: String::new(), fonts: HashMap::new(), pages_left: max_pages };
        let mut drawn = 0;
        for (entry, page) in &src.pages {
            if p.pages_left == 0 {
                break;
            }
            drawn += p.page(page, folder(entry), &src.names) as usize;
        }
        (drawn > 0).then_some(p.out)
    }

    /// The id of the font a line's ff class names: its file in the EPUB (as the class's font-family, or the
    /// class's own name, names it), and the standard font to use when there's none or it can't be read.
    fn font_id(&mut self, class: &str) -> usize {
        if let Some(&id) = self.fonts.get(class) {
            return id;
        }
        let rule = self.css.rules.get(class).map(String::as_str);
        let family = rule.and_then(|r| decl(r, "font-family")).map(|f| f.split(',').next().unwrap_or("").trim().trim_matches(['"', '\'']));
        let file = family.and_then(|f| self.css.fonts.get(f)).or_else(|| self.css.fonts.get(class)).map_or("", String::as_str);
        let id = self.fonts.len();
        let _ = writeln!(self.out, "F\t{id}\t{file}\t{}", standard_font(rule));
        self.fonts.insert(class.to_string(), id);
        id
    }

    fn style(&self, classes: &str, base: &Style) -> Style {
        let mut s = base.clone();
        if let Some(c) = class_of(classes, "ff") {
            s.font = c.to_string();
        }
        if let Some(v) = class_of(classes, "fs").and_then(|c| self.css.px(c, "font-size")) {
            s.size = v;
        }
        if let Some(v) = class_of(classes, "fc").and_then(|c| self.css.color(c)) {
            s.color = v;
        }
        if let Some(v) = class_of(classes, "ls").and_then(|c| self.css.px(c, "letter-spacing")) {
            s.letter = v;
        }
        if let Some(v) = class_of(classes, "ws").and_then(|c| self.css.px(c, "word-spacing")) {
            s.word = v;
        }
        s
    }

    /// Writes one page's instructions; false when it has no page box.
    fn page(&mut self, page: &str, dir: &str, names: &HashSet<String>) -> bool {
        let at = |classes: &str, prefix: &str, prop: &str| class_of(classes, prefix).and_then(|c| self.css.px(c, prop)).unwrap_or(0.0);
        let mut page_h = 0.0f32;
        let mut started = false;
        // Open divs: their offset from the page's left and bottom, and whether one is a line.
        let mut divs: Vec<(f32, f32, bool)> = Vec::new();
        // The line being read: its style stack, origin and matrix, the run so far, and spans to skip.
        let mut spans: Vec<Style> = Vec::new();
        let mut line: Option<([f32; 4], f32, f32)> = None;
        let mut run = String::new();
        let mut cont = false;
        let mut skip = 0usize;
        let mut rest = page;
        while !rest.is_empty() {
            let (token, is_tag) = match rest.find('<') {
                Some(0) => match rest.find('>') {
                    Some(e) => (&rest[..=e], true),
                    None => break,
                },
                Some(i) => (&rest[..i], false),
                None => (rest, false),
            };
            rest = &rest[token.len()..];
            if !is_tag {
                if line.is_some() && skip == 0 {
                    run.push_str(&decode(token).replace(['\t', '\n', '\r', '\u{1}'], " "));
                }
                continue;
            }
            let inner = token.trim_start_matches('<').trim_end_matches('>');
            let closing = inner.starts_with('/');
            let selfclose = inner.ends_with('/');
            let name: String = inner.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            let classes = attr(inner, "class").unwrap_or("");
            let (px, py) = divs.last().map_or((0.0, 0.0), |d| (d.0, d.1));
            match (name.as_str(), closing) {
                ("div", false) => {
                    let first = classes.split_whitespace().next().unwrap_or("");
                    if first == "pf" {
                        let (w, h) = (at(classes, "w", "width"), at(classes, "h", "height"));
                        if w <= 0.0 || h <= 0.0 || self.pages_left == 0 {
                            return started;
                        }
                        self.pages_left -= 1;
                        page_h = h;
                        started = true;
                        let _ = writeln!(self.out, "P\t{w} {h}");
                    }
                    if selfclose {
                        continue;
                    }
                    match first {
                        "c" => divs.push((px + at(classes, "x", "left"), py + at(classes, "y", "bottom"), false)),
                        "t" if started => {
                            let m = class_of(classes, "m").and_then(|c| self.css.matrix(c)).unwrap_or([1.0, 0.0, 0.0, 1.0]);
                            let base = Style { font: String::new(), size: 0.0, color: [0.0, 0.0, 0.0, 1.0], letter: 0.0, word: 0.0 };
                            spans = vec![self.style(classes, &base)];
                            let (x, y) = (px + at(classes, "x", "left"), py + at(classes, "y", "bottom"));
                            line = Some((m, x, page_h - y));
                            run.clear();
                            cont = false;
                            skip = 0;
                            divs.push((px, py, true));
                        }
                        _ => divs.push((px, py, false)),
                    }
                }
                ("div", true) => {
                    if divs.pop().is_some_and(|d| d.2) {
                        self.flush(&spans, line, &mut run, &mut cont);
                        line = None;
                    }
                }
                ("span", false) if line.is_some() => {
                    if skip > 0 {
                        skip += !selfclose as usize;
                        continue;
                    }
                    // A spacer: the gap pdf2htmlEX kept between letters or words (its text isn't drawn).
                    if classes.split_whitespace().next() == Some("_") {
                        let gap = class_of(classes, "_").and_then(|c| self.css.px(c, "width").or_else(|| self.css.px(c, "margin-left"))).unwrap_or(0.0);
                        if gap != 0.0 {
                            let _ = write!(run, "\u{1}{gap}\u{1}");
                        }
                        if !selfclose {
                            skip = 1;
                        }
                        continue;
                    }
                    if selfclose {
                        continue;
                    }
                    let top = spans.last().cloned().unwrap_or_else(|| spans[0].clone());
                    let next = self.style(classes, &top);
                    if next != top {
                        self.flush(&spans, line, &mut run, &mut cont);
                    }
                    spans.push(next);
                }
                ("span", true) if line.is_some() => {
                    if skip > 0 {
                        skip -= 1;
                        continue;
                    }
                    if spans.len() > 1 {
                        let top = spans.pop();
                        if spans.last() != top.as_ref() {
                            self.flush(&[top.unwrap_or_else(|| spans[0].clone())], line, &mut run, &mut cont);
                        }
                    }
                }
                ("img", false) if started && line.is_none() => {
                    let Some(src) = attr(inner, "src").map(|s| resolve(dir, s)).filter(|e| names.contains(e)) else { continue };
                    let (w, h) = (at(classes, "w", "width"), at(classes, "h", "height"));
                    let (x, y) = (px + at(classes, "x", "left"), py + at(classes, "y", "bottom"));
                    if w > 0.0 && h > 0.0 {
                        let _ = writeln!(self.out, "I\t{src}\t{x} {} {w} {h}", page_h - y - h);
                    }
                }
                _ => {}
            }
        }
        started
    }

    /// Writes the run read so far in the style on top of `spans`.
    fn flush(&mut self, spans: &[Style], line: Option<([f32; 4], f32, f32)>, run: &mut String, cont: &mut bool) {
        let (Some(st), Some((m, x, y))) = (spans.last(), line) else { return };
        if run.is_empty() {
            return;
        }
        let text = std::mem::take(run);
        let id = self.font_id(&st.font);
        // Hidden text keeps its place for searching and copying, unseen.
        let alpha = if self.css.hidden(&st.font) { 0.0 } else { st.color[3] };
        let [r, g, b, _] = st.color;
        let _ = writeln!(
            self.out,
            "G\t{id} {r} {g} {b} {alpha} {} {} {} {} {x} {y} {} {} {} {}\t{text}",
            m[0], m[1], m[2], m[3], st.size, st.letter, st.word, *cont as u8
        );
        *cont = true;
    }
}

#[derive(Clone)]
struct Line {
    text: String,
    x: f32,
    y: f32,
    size: f32,
}

struct Page {
    lines: Vec<Line>,
    pictures: Vec<String>,
}

enum Block {
    Heading(String),
    Para(String),
    Picture(usize),
}

struct Book {
    title: String,
    creator: String,
    language: String,
    blocks: Vec<Block>,
    /// The pictures' entries in the EPUB, with their bytes.
    pictures: Vec<(String, Vec<u8>)>,
}

/// `dir` + `rel` as a zip entry name ("OEBPS/pages/" + "../css/a.css" = "OEBPS/css/a.css").
fn resolve(dir: &str, rel: &str) -> String {
    let rel = rel.split(['#', '?']).next().unwrap_or("");
    let mut out: Vec<&str> = Vec::new();
    for part in dir.split('/').chain(rel.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            p => out.push(p),
        }
    }
    out.join("/")
}

fn folder(entry: &str) -> &str {
    entry.rfind('/').map_or("", |i| &entry[..=i])
}

/// The value of `attr="…"` in one tag.
fn attr<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(i) = tag[from..].find(attr) {
        let at = from + i;
        let before = tag[..at].chars().next_back();
        let rest = &tag[at + attr.len()..];
        if matches!(before, Some(c) if c.is_whitespace()) {
            if let Some(rest) = rest.trim_start().strip_prefix('=') {
                let rest = rest.trim_start();
                let q = rest.chars().next()?;
                if q == '"' || q == '\'' {
                    return rest[1..].find(q).map(|e| &rest[1..1 + e]);
                }
            }
        }
        from = at + attr.len();
    }
    None
}

/// Every tag named `name` (`<name …>`), as the text between `<` and `>`.
fn tags<'a>(s: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name}");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = s[from..].find(&open) {
        let at = from + i;
        let after = s[at + open.len()..].chars().next();
        let Some(end) = s[at..].find('>') else { break };
        if matches!(after, Some(c) if c.is_whitespace() || c == '/' || c == '>') {
            out.push(&s[at + 1..at + end]);
        }
        from = at + end;
    }
    out
}

/// The text of the first `<name …>…</name>`.
fn element_text(s: &str, name: &str) -> String {
    let Some(tag) = tags(s, name).into_iter().next() else { return String::new() };
    let start = s.find(tag).map(|i| i + tag.len() + 1).unwrap_or(0);
    let end = s[start..].find(&format!("</{name}")).map_or(start, |e| start + e);
    clean(&s[start..end])
}

fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.char_indices().take(12).find(|&(_, c)| c == ';').map(|(i, _)| i) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let c = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ if name.starts_with("#x") || name.starts_with("#X") => u32::from_str_radix(&name[2..], 16).ok().and_then(char::from_u32),
            _ if name.starts_with('#') => name[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Text without its tags: entities decoded, runs of spaces made one, ligatures spelled out (a
/// spacer after a ligature had become a space: "beneﬁ t").
fn clean(html: &str) -> String {
    let mut bare = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => bare.push(c),
            _ => {}
        }
    }
    let text = decode(&bare);
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    let mut after_ligature = false;
    for c in text.chars() {
        let blank = c.is_whitespace() || ('\u{2000}'..='\u{200b}').contains(&c) || c == '\u{a0}';
        if blank {
            space = !out.is_empty();
            continue;
        }
        if space && !(after_ligature && c.is_lowercase()) {
            out.push(' ');
        }
        space = false;
        after_ligature = true;
        match c {
            '\u{fb00}' => out.push_str("ff"),
            '\u{fb01}' => out.push_str("fi"),
            '\u{fb02}' => out.push_str("fl"),
            '\u{fb03}' => out.push_str("ffi"),
            '\u{fb04}' => out.push_str("ffl"),
            '\u{fb05}' | '\u{fb06}' => out.push_str("st"),
            _ => {
                after_ligature = false;
                out.push(c);
            }
        }
    }
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}


/// A pdf2htmlEX page: pinned pages (`class="pf …"`) of pinned lines (`class="t …"`).
fn is_pinned(page: &str) -> bool {
    tags(page, "div").iter().any(|t| attr(t, "class").is_some_and(|c| c == "pf" || c.starts_with("pf ")))
        && tags(page, "div").iter().any(|t| attr(t, "class").is_some_and(|c| c.starts_with("t ")))
}

impl Book {
    fn read(path: &str, src: &Source) -> Book {
        let pages: Vec<Page> = src.pages.iter().map(|(entry, page)| read_page(page, folder(entry), &src.css, &src.names)).collect();
        let mut pictures = Vec::new();
        let blocks = assemble(pages, &mut |entry: &str| {
            let bytes = zip_read(path, entry).ok().flatten()?;
            (bytes.len() >= MIN_PICTURE).then(|| {
                pictures.push((entry.to_string(), bytes));
                pictures.len() - 1
            })
        });
        let opf = &src.opf;
        Book { title: element_text(opf, "dc:title"), creator: element_text(opf, "dc:creator"), language: element_text(opf, "dc:language"), blocks, pictures }
    }

    /// The entries of an ordinary EPUB of this book (packed by `pack`).
    fn files(&self) -> Vec<(String, Vec<u8>)> {
        let mut files = vec![
            ("mimetype".to_string(), b"application/epub+zip".to_vec()),
            (
                "META-INF/container.xml".to_string(),
                br#"<?xml version="1.0" encoding="UTF-8"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#.to_vec(),
            ),
        ];
        let mut items = String::new();
        for (i, (entry, bytes)) in self.pictures.iter().enumerate() {
            let ext = entry.rsplit('.').next().unwrap_or("png").to_ascii_lowercase();
            let mime = match ext.as_str() {
                "jpg" | "jpeg" => "image/jpeg",
                "svg" => "image/svg+xml",
                "gif" => "image/gif",
                _ => "image/png",
            };
            files.push((format!("p{i}.{ext}"), bytes.clone()));
            let _ = write!(items, r#"<item id="p{i}" href="p{i}.{ext}" media-type="{mime}"/>"#);
        }
        let mut body = String::new();
        for b in &self.blocks {
            match b {
                Block::Heading(t) => {
                    let _ = write!(body, "<h2>{}</h2>\n", escape(t));
                }
                Block::Para(t) => {
                    let _ = write!(body, "<p>{}</p>\n", escape(t));
                }
                Block::Picture(i) => {
                    let ext = self.pictures[*i].0.rsplit('.').next().unwrap_or("png").to_ascii_lowercase();
                    let _ = write!(body, "<div class=\"pic\"><img src=\"p{i}.{ext}\" alt=\"\"/></div>\n");
                }
            }
        }
        files.push((
            "book.xhtml".to_string(),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>{}</title><style>p{{margin:0 0 0.6em}} h2{{margin:1.2em 0 0.5em;font-size:1.25em}} .pic{{margin:0.8em 0;text-align:center}} img{{max-width:100%}}</style></head><body>\n{body}</body></html>",
                escape(&self.title)
            )
            .into_bytes(),
        ));
        files.push((
            "book.opf".to_string(),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="2.0" unique-identifier="id"><metadata><dc:identifier id="id">paginaut-rebuilt</dc:identifier><dc:title>{}</dc:title><dc:creator>{}</dc:creator><dc:language>{}</dc:language></metadata><manifest><item id="book" href="book.xhtml" media-type="application/xhtml+xml"/>{items}</manifest><spine><itemref idref="book"/></spine></package>"#,
                escape(&self.title),
                escape(&self.creator),
                escape(if self.language.is_empty() { "en" } else { &self.language })
            )
            .into_bytes(),
        ));
        files
    }
}

fn read_page(page: &str, dir: &str, css: &Css, names: &HashSet<String>) -> Page {
    let pictures = tags(page, "img").into_iter().filter_map(|t| attr(t, "src")).map(|s| resolve(dir, s)).filter(|e| names.contains(e)).collect();
    let mut lines = Vec::new();
    let mut from = 0;
    while let Some(i) = page[from..].find("<div class=\"t ") {
        let at = from + i;
        let Some(open_end) = page[at..].find('>').map(|e| at + e) else { break };
        let close = page[open_end..].find("</div>").map_or(page.len(), |e| open_end + e);
        from = close;
        let classes = attr(&page[at + 1..open_end], "class").unwrap_or("");
        let text = clean(&page[open_end + 1..close]);
        if text.is_empty() {
            continue;
        }
        let px = |prefix: &str, prop: &str| class_of(classes, prefix).and_then(|c| css.px(c, prop));
        // The size the line is seen at: its type size times its matrix's scale.
        let scale = class_of(classes, "m").and_then(|c| css.matrix(c)).map_or(1.0, |m| (m[0] * m[0] + m[1] * m[1]).sqrt());
        let size = px("fs", "font-size").unwrap_or(0.0) * scale;
        lines.push(Line { text, x: px("x", "left").unwrap_or(0.0), y: px("y", "bottom").unwrap_or(0.0), size: (size * 100.0).round() / 100.0 });
    }
    Page { lines, pictures }
}

/// Text without its digits and separators: the same running head on every page ("4 • Business
/// Plans that Work", "Business Plans that Work • 5").
fn running_key(t: &str) -> String {
    t.chars().filter(|c| !c.is_ascii_digit() && !"•·|".contains(*c)).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn is_folio(t: &str) -> bool {
    !t.is_empty() && t.chars().all(|c| c.is_ascii_digit() || "ivxlcIVXLC •·|".contains(c))
}

fn same(a: f32, b: f32) -> bool {
    (a - b).abs() <= 0.05 * a.max(b).max(1.0)
}

fn median(mut v: Vec<f32>) -> Option<f32> {
    v.sort_by(|a, b| a.total_cmp(b));
    v.get(v.len() / 2).copied()
}

fn ends_sentence(t: &str) -> bool {
    t.trim_end().ends_with(['.', '!', '?', ':', '"', '”', '’'])
}

fn starts_item(t: &str) -> bool {
    let mut cs = t.chars();
    match cs.next() {
        Some('•' | '▪' | '■' | '●' | '◦' | '–') => true,
        Some(c) if c.is_ascii_digit() => {
            let rest: String = t.chars().skip_while(|c| c.is_ascii_digit()).take(2).collect();
            t.chars().take_while(|c| c.is_ascii_digit()).count() <= 2 && rest.starts_with(". ")
        }
        _ => false,
    }
}

fn assemble(mut pages: Vec<Page>, picture: &mut dyn FnMut(&str) -> Option<usize>) -> Vec<Block> {
    // Running heads and page numbers: at a page's top or bottom, on many pages.
    let mut edge: HashMap<String, usize> = HashMap::new();
    for p in &pages {
        let n = p.lines.len();
        for (i, l) in p.lines.iter().enumerate() {
            if i < 2 || i + 2 >= n {
                let k = running_key(&l.text);
                if !k.is_empty() {
                    *edge.entry(k).or_default() += 1;
                }
            }
        }
    }
    let least = 3.max(pages.len() / 20);
    for p in &mut pages {
        let n = p.lines.len();
        let lines = std::mem::take(&mut p.lines);
        p.lines = lines
            .into_iter()
            .enumerate()
            .filter(|(i, l)| !((*i < 2 || i + 2 >= n) && (is_folio(&l.text) || edge.get(&running_key(&l.text)).is_some_and(|&c| c >= least))))
            .map(|(_, l)| l)
            .collect();
    }
    // The body text's size: the one most characters are set in.
    let mut chars: HashMap<u32, usize> = HashMap::new();
    for l in pages.iter().flat_map(|p| &p.lines) {
        *chars.entry(l.size.to_bits()).or_default() += l.text.chars().count();
    }
    let body = chars.into_iter().max_by_key(|(_, n)| *n).map_or(0.0, |(s, _)| f32::from_bits(s));

    let mut out = Vec::new();
    let mut open: Option<(bool, String)> = None; // (heading, text)
    let flush = |open: &mut Option<(bool, String)>, out: &mut Vec<Block>| {
        if let Some((h, t)) = open.take() {
            out.push(if h { Block::Heading(t) } else { Block::Para(t) });
        }
    };
    for p in &pages {
        // Per type size on this page: the usual left edge and line spacing.
        let mut left: HashMap<u32, f32> = HashMap::new();
        let mut lead: HashMap<u32, f32> = HashMap::new();
        for l in &p.lines {
            let k = l.size.to_bits();
            if left.contains_key(&k) {
                continue;
            }
            let mut xs: HashMap<i32, usize> = HashMap::new();
            for m in p.lines.iter().filter(|m| m.size == l.size) {
                *xs.entry(m.x.round() as i32).or_default() += 1;
            }
            let most = xs.values().copied().max().unwrap_or(0);
            left.insert(k, xs.iter().filter(|(_, &n)| n == most).map(|(&x, _)| x).min().unwrap_or(0) as f32);
            let gaps: Vec<f32> = p.lines.windows(2).filter(|w| w[0].size == l.size && w[1].size == l.size).map(|w| w[0].y - w[1].y).filter(|g| *g > 0.0 && *g < l.size * 3.0).collect();
            lead.insert(k, median(gaps).unwrap_or(l.size * 1.3));
        }
        for pic in &p.pictures {
            if let Some(i) = picture(pic) {
                flush(&mut open, &mut out);
                out.push(Block::Picture(i));
            }
        }
        let mut prev: Option<&Line> = None;
        for l in &p.lines {
            // Footnote marks set apart: small numbers alone.
            if body > 0.0 && l.size < body * 0.9 && l.text.len() <= 3 && l.text.chars().all(|c| c.is_ascii_digit() || c == '*') {
                continue;
            }
            let k = l.size.to_bits();
            let heading = body > 0.0 && l.size >= body * 1.2 && l.text.chars().count() < 150;
            let indented = l.x > left[&k] + l.size * 0.8;
            let new = match (&open, prev) {
                (None, _) => true,
                (Some((h, _)), _) if *h != heading => true,
                (Some((_, t)), None) => heading || indented || ends_sentence(t),
                (Some(_), Some(pv)) => {
                    !same(l.size, pv.size) || (!heading && indented) || pv.y - l.y > if heading { l.size * 2.2 } else { lead[&k] * 1.5 }
                }
            } || starts_item(&l.text);
            if new {
                flush(&mut open, &mut out);
                open = Some((heading, l.text.clone()));
            } else if let Some((_, t)) = &mut open {
                // A word broken at the end of a line is joined again.
                let hyphen = t.ends_with('-') && t.chars().rev().nth(1).is_some_and(char::is_alphabetic) && l.text.starts_with(char::is_lowercase);
                if hyphen {
                    t.pop();
                } else {
                    t.push(' ');
                }
                t.push_str(&l.text);
            }
            prev = Some(l);
        }
        // A heading never runs on to the next page.
        if open.as_ref().is_some_and(|(h, _)| *h) {
            flush(&mut open, &mut out);
        }
    }
    flush(&mut open, &mut out);
    // A chapter's large first letter (a drop cap) belongs to the paragraph after it.
    let mut joined = Vec::with_capacity(out.len());
    let mut cap: Option<String> = None;
    for b in out {
        match b {
            Block::Heading(t) if t.chars().count() == 1 && t.chars().all(char::is_alphabetic) => cap = Some(t),
            Block::Para(t) if cap.is_some() => joined.push(Block::Para(cap.take().unwrap_or_default() + &t)),
            b => {
                if let Some(c) = cap.take() {
                    joined.push(Block::Heading(c));
                }
                joined.push(b);
            }
        }
    }
    joined.extend(cap.map(Block::Heading));
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacers_and_entities() {
        assert_eq!(clean(r#"to the <span class="_ _1d"/>ru<span class="_ _1d"/>le &amp; beneﬁ <span class="_ _2"> </span>t"#), "to the rule & benefit");
        assert_eq!(clean("4 <span class=\"_ _4\"> </span> <span class=\"_ _8\"/>• Business"), "4 • Business");
    }

    #[test]
    fn css_numbers() {
        let mut c = Css::default();
        // A print block in the middle (as in pdf2htmlEX's base CSS), a comment, a screen block and a group.
        c.read("/*! pdf2htmlEX {x} */.x1d{left:72.5px;}@media print{.x1d{left:9pt;}.pf{margin:0}}.y69{bottom:540px;}@media screen{.fsb{font-size:40px;}}.m2,.m3{transform:matrix(0.25,0,0,0.25,0,0);}@font-face{font-family:ff1;src:url(f1.woff)format(\"woff\");}", "OEBPS/");
        assert_eq!(c.px("x1d", "left"), Some(72.5));
        assert_eq!(c.px("y69", "bottom"), Some(540.0));
        assert_eq!(c.px("fsb", "font-size"), Some(40.0));
        assert_eq!(c.matrix("m3"), Some([0.25, 0.0, 0.0, 0.25]));
        assert_eq!(c.fonts.get("ff1").map(String::as_str), Some("OEBPS/f1.woff"));
    }

    #[test]
    fn every_style_element() {
        let page = "<head><style type=\"text/css\">.a{left:1px}</style><style>@media print{.b{left:9pt}}.b{left:2px}</style></head>";
        let mut c = Css::default();
        for s in style_elements(page) {
            c.read(s, "");
        }
        assert_eq!(c.px("a", "left"), Some(1.0));
        assert_eq!(c.px("b", "left"), Some(2.0));
    }

    #[test]
    fn standard_fonts() {
        assert_eq!(standard_font(None), "Times-Roman");
        assert_eq!(standard_font(Some("font-family:sans-serif;font-weight:bold")), "Helvetica-Bold");
        assert_eq!(standard_font(Some("font-family:\"Courier New\";font-style:italic")), "Courier-Oblique");
        assert_eq!(standard_font(Some("font-family:Georgia,serif;font-weight:700")), "Times-Bold");
    }

    #[test]
    fn crc() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn paths() {
        assert_eq!(resolve("OEBPS/pages/", "../css/pdf.css"), "OEBPS/css/pdf.css");
        assert_eq!(resolve("", "OEBPS/content.opf"), "OEBPS/content.opf");
    }

    /// A real converted book, when one is at hand (PAGINAUT_PDFHTML=<file.epub>).
    #[test]
    fn real_book() {
        let Ok(path) = std::env::var("PAGINAUT_PDFHTML") else { return };
        let _ = super::super::mupdf::init(64 << 20);
        let temp = std::env::temp_dir().join("paginaut-pdfhtml-test");
        let _ = std::fs::remove_dir_all(&temp);
        let _ = std::fs::create_dir_all(&temp);
        let t0 = std::time::Instant::now();
        let pdf = as_pdf(&path, &temp).expect("a pdf2htmlEX book");
        println!("{pdf}: drawn in {:?}", t0.elapsed());
        assert!(is_rebuilt(&pdf));
        let doc = super::super::mupdf::Document::open(&pdf).expect("opens");
        assert!(doc.is_pdf());
        println!("{:?} pages", doc.page_count());
        let epub = temp.join("repaired.epub");
        repaired_epub(&path, &epub.to_string_lossy()).expect("repairs");
        let names = zip_list(&epub.to_string_lossy()).expect("a zip");
        assert_eq!(names[0], "mimetype");
        assert!(names.iter().any(|n| n == "book.xhtml"));
        let e = super::super::mupdf::Document::open(&epub.to_string_lossy()).expect("opens");
        assert_eq!(e.metadata("format").as_deref(), Some("EPUB"));
        // The shelf's preview: the first page alone, the whole book's count.
        let t0 = std::time::Instant::now();
        let p = preview(&path, &temp).expect("a preview");
        println!("preview in {:?}: {:?} by {:?}, {} pages", t0.elapsed(), p.title, p.author, p.pages);
        let one = super::super::mupdf::Document::open(&p.pdf).expect("opens");
        assert_eq!(one.page_count().ok(), Some(1));
        assert_eq!(Some(p.pages), doc.page_count().ok());
        // Pages to look at (PAGINAUT_PDFHTML_PAGES=0,5,…), through the engine as the app opens the book.
        if let Ok(list) = std::env::var("PAGINAUT_PDFHTML_PAGES") {
            let engine = super::super::Engine::new(1, 64 << 20).expect("engine");
            let info = engine.open(&path).expect("opens through the engine");
            assert!(info.repaired);
            for i in list.split(',').filter_map(|n| n.trim().parse::<usize>().ok()) {
                let png = engine.doc(info.doc_id).unwrap().page_image(i, 0, "png", 110.0).expect("draws");
                std::fs::write(temp.join(format!("page-{i}.png")), png).unwrap();
            }
            println!("pages drawn in {}", temp.display());
        }
    }

    /// A book whose CSS is only in `<style>` elements (pdf2htmlEX's default), with a print block in the middle
    /// of the first one, and a line in a font it doesn't carry: the line is drawn, in a standard font, and its
    /// text can be found.
    #[test]
    fn styles_inside_and_a_font_not_carried() {
        let _ = super::super::mupdf::init(64 << 20);
        let temp = std::env::temp_dir().join(format!("paginaut-pdfhtml-styles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp);
        let dir = temp.join("book");
        std::fs::create_dir_all(dir.join("META-INF")).unwrap();
        std::fs::write(dir.join("mimetype"), "application/epub+zip").unwrap();
        std::fs::write(
            dir.join("META-INF/container.xml"),
            r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("content.opf"),
            r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>T</dc:title></metadata><manifest><item id="p1" href="page.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="p1"/></spine></package>"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("page.xhtml"),
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style type="text/css">.pf{position:relative}@media print{.pf{margin:0}}.t{position:absolute}</style><style type="text/css">.ff1{font-family:sans-serif;font-weight:bold;visibility:visible;}.fs0{font-size:24px;}.fc0{color:rgb(0,0,0);}.m0{transform:matrix(1,0,0,1,0,0);}.x0{left:50px;}.y0{bottom:300px;}.w0{width:400px;}.h0{height:400px;}</style></head><body><div class="pf w0 h0"><div class="t m0 x0 y0 ff1 fs0 fc0">Hello <span class="_ _0"> </span>pinned world</div></div></body></html>"#,
        )
        .unwrap();
        let epub = temp.join("styles.epub");
        let files = ["mimetype", "META-INF/container.xml", "content.opf", "page.xhtml"].map(|n| (n.to_string(), std::fs::read(dir.join(n)).unwrap())).to_vec();
        std::fs::write(&epub, pack(files)).unwrap();
        let pdf = as_pdf(&epub.to_string_lossy(), &temp).expect("drawn as a PDF");
        let doc = super::super::mupdf::Document::open(&pdf).unwrap();
        let page = doc.load_page(0).unwrap();
        let b = page.bounds().unwrap();
        assert_eq!([b[2] - b[0], b[3] - b[1]], [400.0, 400.0]);
        let text = page.display_list(0).unwrap().text().unwrap();
        assert!(!text.search(&b, "Hello").unwrap().is_empty());
        assert!(!text.search(&b, "pinned world").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&temp);
    }

    /// Two opens of one book at once: one build, both get the same whole PDF.
    #[test]
    fn real_book_twice() {
        let Ok(path) = std::env::var("PAGINAUT_PDFHTML") else { return };
        let _ = super::super::mupdf::init(64 << 20);
        let temp = std::env::temp_dir().join("paginaut-pdfhtml-twice");
        let _ = std::fs::remove_dir_all(&temp);
        let _ = std::fs::create_dir_all(&temp);
        let both: Vec<Option<String>> = std::thread::scope(|s| {
            let a = s.spawn(|| as_pdf(&path, &temp));
            let b = s.spawn(|| as_pdf(&path, &temp));
            vec![a.join().unwrap(), b.join().unwrap()]
        });
        assert!(both[0].is_some() && both[0] == both[1]);
        let doc = super::super::mupdf::Document::open(both[0].as_ref().unwrap()).expect("a whole PDF");
        assert!(doc.page_count().unwrap() > 1);
        // No half-made files left.
        assert!(std::fs::read_dir(&temp).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".part")));
    }
}
