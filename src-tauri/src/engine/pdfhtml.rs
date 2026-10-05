//! EPUBs that are PDFs in disguise. pdf2htmlEX (behind many converted textbooks) pins every line of
//! a PDF page in place with CSS (`position: absolute`, a class per left and bottom coordinate) and cuts
//! words into pieces with empty spacer spans that copy the PDF's letter spacing. MuPDF lays HTML out as
//! flowing text, so those pages come out as lines piled on one another and words split in two.
//!
//! Such a book is rebuilt here as an ordinary EPUB, unpacked in the temp folder: the lines read in
//! order with the spacers dropped, joined into paragraphs (a new one at an indent, a wider gap or a
//! change of type size), larger type as headings, the running heads and page numbers left out, and the
//! pages' pictures kept, packed as an EPUB of its own. The original file is never touched.

use super::mupdf::{zip_list, zip_read};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Pictures smaller than this are page backgrounds with nothing on them.
const MIN_PICTURE: usize = 20_000;

/// The rebuilt book (an EPUB in `temp`), when `path` is an EPUB made by pdf2htmlEX; None for any
/// other file. Made once per file and run (see `clear`).
pub fn rebuilt(path: &str, temp: &Path) -> Option<String> {
    if !path.to_ascii_lowercase().ends_with(".epub") {
        return None;
    }
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (path, meta.len(), mtime).hash(&mut h);
        h.finish()
    };
    let out = temp.join(format!("paginaut-book-{key:016x}.epub"));
    if out.exists() {
        return Some(out.to_string_lossy().into_owned());
    }
    let book = Book::read(path)?;
    let dir = temp.join(format!("paginaut-book-{key:016x}"));
    book.write(&dir).ok()?;
    let packed = pack(&dir, &out.to_string_lossy());
    let _ = std::fs::remove_dir_all(&dir);
    packed.ok()?;
    Some(out.to_string_lossy().into_owned())
}

/// Whether `path` (a document's file) is a book rebuilt here.
pub fn is_rebuilt(path: &str) -> bool {
    Path::new(path).file_name().is_some_and(|n| {
        let n = n.to_string_lossy();
        n.starts_with("paginaut-book-") && n.ends_with(".epub")
    })
}

/// Packs an unpacked EPUB (the folder `root`) into an EPUB file at `target`: the "mimetype" entry
/// first and every entry stored as it is (the pictures are compressed already).
fn pack(root: &Path, target: &str) -> Result<(), String> {
    let mut files = vec!["mimetype".to_string(), "META-INF/container.xml".to_string()];
    let mut rest: Vec<String> = std::fs::read_dir(root)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n != "mimetype")
        .collect();
    rest.sort();
    files.extend(rest);
    let mut zip = Vec::new();
    let mut central = Vec::new();
    for name in &files {
        let data = std::fs::read(root.join(name)).map_err(|e| e.to_string())?;
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
    // Written beside the target first: saving over the original e-book is allowed.
    if let Some(dir) = Path::new(target).parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let part = format!("{target}.part");
    std::fs::write(&part, &zip).map_err(|e| e.to_string())?;
    std::fs::rename(&part, target).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        e.to_string()
    })
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
            if e.file_name().to_string_lossy().starts_with("paginaut-book-") {
                let _ = if e.path().is_dir() { std::fs::remove_dir_all(e.path()) } else { std::fs::remove_file(e.path()) };
            }
        }
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

/// The numbers pdf2htmlEX's classes stand for: `.x1d{left:72px}`, `.y69{bottom:540px}`,
/// `.fsb{font-size:40px}`, `.m2{transform:matrix(0.25,…)}` (screen rules only, not the print ones).
fn read_css(css: &str, into: &mut HashMap<String, f32>) {
    let css = css.split("@media print").next().unwrap_or(css);
    let mut rest = css;
    while let Some(i) = rest.find('.') {
        rest = &rest[i + 1..];
        let name_end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
        let name = &rest[..name_end];
        let after = &rest[name_end..];
        if name.is_empty() || !after.starts_with('{') {
            continue;
        }
        let Some(close) = after.find('}') else { break };
        let body = &after[1..close];
        let px = |prop: &str| {
            body.split(';').find_map(|d| d.trim().strip_prefix(prop)?.trim_start().strip_prefix(':')?.trim().strip_suffix("px")?.parse::<f32>().ok())
        };
        let value = match name.as_bytes()[0] {
            b'x' => px("left"),
            b'y' => px("bottom"),
            b'f' if name.starts_with("fs") => px("font-size"),
            b'm' => body.find("matrix(").and_then(|i| {
                let v: Vec<f32> = body[i + 7..].split([',', ')']).take(2).filter_map(|n| n.trim().parse().ok()).collect();
                (v.len() == 2).then(|| (v[0] * v[0] + v[1] * v[1]).sqrt())
            }),
            _ => None,
        };
        if let Some(v) = value {
            into.insert(name.to_string(), v);
        }
        rest = &after[close..];
    }
}

/// A pdf2htmlEX page: pinned pages (`class="pf …"`) of pinned lines (`class="t …"`).
fn is_pinned(page: &str) -> bool {
    tags(page, "div").iter().any(|t| attr(t, "class").is_some_and(|c| c == "pf" || c.starts_with("pf ")))
        && tags(page, "div").iter().any(|t| attr(t, "class").is_some_and(|c| c.starts_with("t ")))
}

impl Book {
    fn read(path: &str) -> Option<Book> {
        let names: HashSet<String> = zip_list(path).ok()?.into_iter().collect();
        let text = |entry: &str| zip_read(path, entry).ok().flatten().map(|b| String::from_utf8_lossy(&b).into_owned());
        let container = text("META-INF/container.xml")?;
        let opf_entry = tags(&container, "rootfile").into_iter().find_map(|t| attr(t, "full-path"))?.to_string();
        let opf = text(&opf_entry)?;
        let base = folder(&opf_entry).to_string();
        let manifest: HashMap<&str, &str> = tags(&opf, "item").into_iter().filter_map(|t| Some((attr(t, "id")?, attr(t, "href")?))).collect();
        let spine: Vec<String> = tags(&opf, "itemref").into_iter().filter_map(|t| manifest.get(attr(t, "idref")?)).map(|h| resolve(&base, h)).collect();
        // Only books whose first pages are pinned.
        if !spine.iter().take(3).filter_map(|e| text(e)).any(|p| is_pinned(&p)) {
            return None;
        }
        let mut css = HashMap::new();
        let mut css_read = HashSet::new();
        let mut pages = Vec::new();
        for entry in &spine {
            let Some(page) = text(entry) else { continue };
            let dir = folder(entry);
            for link in tags(&page, "link") {
                if let Some(href) = attr(link, "href").filter(|h| h.ends_with(".css")) {
                    let e = resolve(dir, href);
                    if css_read.insert(e.clone()) {
                        if let Some(s) = text(&e) {
                            read_css(&s, &mut css);
                        }
                    }
                }
            }
            if let Some(i) = page.find("<style") {
                if let Some(end) = page[i..].find("</style>") {
                    read_css(&page[i..i + end], &mut css);
                }
            }
            pages.push(read_page(&page, dir, &css, &names));
        }
        let mut pictures = Vec::new();
        let blocks = assemble(pages, &mut |entry: &str| {
            let bytes = zip_read(path, entry).ok().flatten()?;
            (bytes.len() >= MIN_PICTURE).then(|| {
                pictures.push((entry.to_string(), bytes));
                pictures.len() - 1
            })
        });
        Some(Book { title: element_text(&opf, "dc:title"), creator: element_text(&opf, "dc:creator"), language: element_text(&opf, "dc:language"), blocks, pictures })
    }

    fn write(&self, dir: &Path) -> std::io::Result<()> {
        let tmp: PathBuf = dir.with_extension("part");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("META-INF"))?;
        std::fs::write(tmp.join("mimetype"), "application/epub+zip")?;
        std::fs::write(
            tmp.join("META-INF").join("container.xml"),
            r#"<?xml version="1.0" encoding="UTF-8"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#,
        )?;
        let mut items = String::new();
        for (i, (entry, bytes)) in self.pictures.iter().enumerate() {
            let ext = entry.rsplit('.').next().unwrap_or("png").to_ascii_lowercase();
            let mime = match ext.as_str() {
                "jpg" | "jpeg" => "image/jpeg",
                "svg" => "image/svg+xml",
                "gif" => "image/gif",
                _ => "image/png",
            };
            std::fs::write(tmp.join(format!("p{i}.{ext}")), bytes)?;
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
        std::fs::write(
            tmp.join("book.xhtml"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>{}</title><style>p{{margin:0 0 0.6em}} h2{{margin:1.2em 0 0.5em;font-size:1.25em}} .pic{{margin:0.8em 0;text-align:center}} img{{max-width:100%}}</style></head><body>\n{body}</body></html>",
                escape(&self.title)
            ),
        )?;
        std::fs::write(
            tmp.join("book.opf"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><package xmlns="http://www.idpf.org/2007/opf" xmlns:dc="http://purl.org/dc/elements/1.1/" version="2.0" unique-identifier="id"><metadata><dc:identifier id="id">paginaut-rebuilt</dc:identifier><dc:title>{}</dc:title><dc:creator>{}</dc:creator><dc:language>{}</dc:language></metadata><manifest><item id="book" href="book.xhtml" media-type="application/xhtml+xml"/>{items}</manifest><spine><itemref idref="book"/></spine></package>"#,
                escape(&self.title),
                escape(&self.creator),
                escape(if self.language.is_empty() { "en" } else { &self.language })
            ),
        )?;
        let _ = std::fs::remove_dir_all(dir);
        std::fs::rename(&tmp, dir)
    }
}

fn read_page(page: &str, dir: &str, css: &HashMap<String, f32>, names: &HashSet<String>) -> Page {
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
        let get = |prefix: &str| classes.split_whitespace().filter(|c| c.len() > prefix.len() && c.starts_with(prefix) && c[prefix.len()..].bytes().all(|b| b.is_ascii_hexdigit())).find_map(|c| css.get(c).copied());
        let size = get("fs").unwrap_or(0.0) * get("m").unwrap_or(1.0);
        lines.push(Line { text, x: get("x").unwrap_or(0.0), y: get("y").unwrap_or(0.0), size: (size * 100.0).round() / 100.0 });
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
        let mut m = HashMap::new();
        read_css(".x1d{left:72.5px;}.y69{bottom:540px;}.fsb{font-size:40px;}.m2{transform:matrix(0.25,0,0,0.25,0,0);}@media print{.x1d{left:9pt;}}", &mut m);
        assert_eq!(m["x1d"], 72.5);
        assert_eq!(m["y69"], 540.0);
        assert_eq!(m["fsb"], 40.0);
        assert!((m["m2"] - 0.25).abs() < 1e-6);
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
        let entry = rebuilt(&path, &temp).expect("a pdf2htmlEX book");
        assert!(is_rebuilt(&entry));
        let names = zip_list(&entry).expect("a zip");
        assert_eq!(names[0], "mimetype");
        assert!(names.iter().any(|n| n == "book.xhtml"));
        let doc = super::super::mupdf::Document::open(&entry).expect("opens");
        assert_eq!(doc.metadata("format").as_deref(), Some("EPUB"));
        doc.layout(450.0, 640.0, 11.0).expect("lays out");
        println!("{entry}: {:?} pages", doc.page_count());
        assert!(doc.page_count().unwrap() > 100);
    }
}
