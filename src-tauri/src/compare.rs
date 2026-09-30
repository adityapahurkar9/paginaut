//! Compare: two documents' words, lined up word by word (a PDF with a PDF, or with the Word, PowerPoint, Excel or
//! Markdown file it was made from), and for two PDFs, the drawings and scans that changed where no text is.

use crate::engine::mupdf::Document;
use crate::engine::{DocId, Engine};
use crate::officetext::{self, Block};
use serde::{Deserialize, Serialize};
use similar::algorithms::{myers, patience, Capture, Replace};
use similar::DiffOp;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// One side: an open document, or a file whose text is read directly (Word, PowerPoint, Excel, OpenDocument).
#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SideReq {
    pub path: String,
    pub doc: Option<DocId>,
    /// Pages you can see (not reflowable, not converted): drawings can be compared too.
    #[serde(default)]
    pub fixed: bool,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub match_case: bool,
    /// Leave out running headers, footers and page numbers.
    pub skip_edges: bool,
    /// Compare drawings and scans too (two PDFs whose pages have the same size).
    pub pictures: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub changes: Vec<Change>,
    /// Places that match, for scrolling both sides together: [a page or block, a y, b page or block, b y].
    pub anchors: Vec<[f32; 4]>,
    pub a_blocks: Option<Vec<Block>>,
    pub b_blocks: Option<Vec<Block>>,
    /// Words compared on each side.
    pub words: [usize; 2],
    /// The documents differ so much that the comparison was cut short: changes are coarse.
    pub rough: bool,
    /// Page pairs whose drawings were compared (None: not compared).
    pub picture_pages: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    /// "changed", "added" (only on the right), "removed" (only on the left) or "picture".
    pub kind: &'static str,
    pub a: Marks,
    pub b: Marks,
    pub old: String,
    pub new: String,
    /// Where in an Office file: "Slide 3", "Sheet1 · B7".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
}

#[derive(Serialize, Default, Clone)]
pub struct Marks {
    /// Documents: [page, x0, y0, x1, y1] (zero width: where something was added on the other side).
    pub boxes: Vec<[f32; 5]>,
    /// Text read from a file: [block, cell, from, to] in UTF-16 units (from = to: an insertion point).
    pub spans: Vec<[u32; 4]>,
    /// Where it is: [page or block, y].
    pub at: [f32; 2],
}

#[derive(Clone, Copy)]
enum At {
    /// Page, box, and the box of the rest of a word broken over two lines.
    Doc(u32, [f32; 4], Option<[f32; 4]>),
    /// Block, cell, from, to (UTF-16).
    Text(u32, u32, u32, u32),
}

struct Tok {
    key: String,
    text: String,
    /// A number: its value as shown and its decimals; exact values from a spreadsheet have decimals -1.
    num: Option<(f64, i32)>,
    at: At,
    /// First word of a line or paragraph (a place for scrolling together).
    line: bool,
}

pub enum Progress<'a> {
    Reading(&'a str, usize, usize),
    Comparing,
    Pictures(usize, usize),
}

enum Side {
    Doc { doc: DocId, pages: Vec<Vec<Word>>, sizes: Vec<[f32; 2]> },
    Text(Vec<Block>),
}

#[derive(Deserialize, Clone)]
struct Word(String, f32, f32, f32, f32, u8);

impl Word {
    fn r(&self) -> [f32; 4] {
        [self.1, self.2, self.3, self.4]
    }
}

pub fn run(engine: &Arc<Engine>, a: &SideReq, b: &SideReq, opts: Options, sid: u64, cancel: &AtomicBool, progress: &(dyn Fn(Progress) + Sync)) -> Result<Outcome, String> {
    let side = |s: &SideReq, which: &'static str| -> Result<Side, String> {
        match s.doc {
            Some(doc) => {
                let total = engine.doc(doc)?.page_count();
                let p = |done| progress(Progress::Reading(which, done, total));
                // The engine's progress runs on its threads: counts only.
                let (tx, rx) = std::sync::mpsc::channel::<usize>();
                let words = std::thread::scope(|scope| {
                    let h = scope.spawn(move || engine.page_words(doc, sid, move |n| {
                        let _ = tx.send(n);
                    }));
                    for n in rx {
                        p(n);
                    }
                    h.join().map_err(|_| "stopped".to_string())?
                })?;
                let mut pages = Vec::with_capacity(words.len());
                let mut sizes = Vec::with_capacity(words.len());
                for (json, size) in words {
                    pages.push(serde_json::from_str::<Vec<Word>>(&json).map_err(|e| e.to_string())?);
                    sizes.push(size);
                }
                Ok(Side::Doc { doc, pages, sizes })
            }
            None => {
                progress(Progress::Reading(which, 0, 1));
                Ok(Side::Text(officetext::read(&s.path)?))
            }
        }
    };
    let sa = side(a, "a")?;
    if cancel.load(Ordering::Acquire) {
        return Err("stopped".into());
    }
    let sb = side(b, "b")?;
    if cancel.load(Ordering::Acquire) {
        return Err("stopped".into());
    }
    progress(Progress::Comparing);
    let text_side = matches!(sa, Side::Text(_)) || matches!(sb, Side::Text(_));
    let sheets = [&sa, &sb].iter().any(|s| matches!(s, Side::Text(b) if b.iter().any(|x| x.k == "sheet")));
    let norm = Norm { case: opts.match_case, numbers: sheets, markers: text_side };
    let ta = tokens(&sa, &norm, opts.skip_edges);
    let tb = tokens(&sb, &norm, opts.skip_edges);

    let deadline = Instant::now() + Duration::from_secs(10);
    // Words compared loosely (numbers rounding to each other, letters a PDF lost) need the slower way of lining up.
    let fuzzy = sheets || ta.iter().chain(&tb).any(|t| t.key.contains('\u{fffd}'));
    let ka: Vec<K> = ta.iter().map(|t| K(t, fuzzy)).collect();
    let kb: Vec<K> = tb.iter().map(|t| K(t, fuzzy)).collect();
    let mut d = Replace::new(Capture::new());
    if fuzzy {
        let _ = myers::diff_deadline(&mut d, &ka, 0..ka.len(), &kb, 0..kb.len(), Some(deadline));
    } else {
        let _ = patience::diff_deadline(&mut d, &ka, 0..ka.len(), &kb, 0..kb.len(), Some(deadline));
    }
    let rough = Instant::now() >= deadline;
    let ops = d.into_inner().into_ops();

    let (blocks_a, blocks_b) = (blocks_of(&sa), blocks_of(&sb));
    let mut changes = changes_of(&ops, &ta, &tb, blocks_a, blocks_b, sheets);
    let anchors = anchors_of(&ops, &ta, &tb);

    let mut picture_pages = None;
    if let (Side::Doc { doc: da, pages: pa, sizes: za }, Side::Doc { doc: db, pages: pb, sizes: zb }) = (&sa, &sb) {
        if opts.pictures && a.fixed && b.fixed {
            let pairs = page_pairs(&ops, &ta, &tb, za, zb);
            picture_pages = Some(pairs.len());
            let found = pictures(engine, *da, *db, &pairs, pa, pb, cancel, progress)?;
            // Each page's picture changes after its text changes.
            for (page_a, page_b, rects) in found {
                let pos = changes.iter().rposition(|c| c.a.at[0] <= page_a as f32).map_or(0, |i| i + 1);
                let mut add = Vec::new();
                for r in rects {
                    add.push(Change {
                        kind: "picture",
                        a: Marks { boxes: vec![[page_a as f32, r[0], r[1], r[2], r[3]]], spans: Vec::new(), at: [page_a as f32, r[1]] },
                        b: Marks { boxes: vec![[page_b as f32, r[0], r[1], r[2], r[3]]], spans: Vec::new(), at: [page_b as f32, r[1]] },
                        old: String::new(),
                        new: String::new(),
                        place: None,
                    });
                }
                changes.splice(pos..pos, add);
            }
        }
    }
    Ok(Outcome {
        changes,
        anchors,
        words: [ta.len(), tb.len()],
        rough,
        picture_pages,
        a_blocks: match sa {
            Side::Text(b) => Some(b),
            _ => None,
        },
        b_blocks: match sb {
            Side::Text(b) => Some(b),
            _ => None,
        },
    })
}

fn blocks_of(s: &Side) -> Option<&[Block]> {
    match s {
        Side::Text(b) => Some(b),
        _ => None,
    }
}

// ---------------------------------------------------------------- words

struct Norm {
    case: bool,
    /// Numbers compared by value ("1,234.50" = "1234.50"), currency signs left out: a spreadsheet's cells.
    numbers: bool,
    /// List numbers and letters at the start of a line left out: Word keeps them out of the text.
    markers: bool,
}

const BULLETS: &str = "•◦▪▫■□●○‣⁃∙·➢➤►▶▸▹✓✔❖◆◇◼◻⦿⦾–—*";

fn is_bullet(t: &str) -> bool {
    t.chars().all(|c| BULLETS.contains(c) || ('\u{e000}'..='\u{f8ff}').contains(&c)) && !t.is_empty()
}

/// "1.", "2)", "(a)", "iv.", "1.2.3", "a." — a list's number or letter.
fn is_marker(t: &str) -> bool {
    let s = t.strip_prefix('(').unwrap_or(t);
    let levels = |b: &str| b.split('.').all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()));
    let Some(body) = s.strip_suffix(['.', ')']) else {
        // Outline numbers print without a closing dot: "2.3.1".
        return s.matches('.').count() >= 2 && s.len() <= 11 && levels(s);
    };
    if body.is_empty() || body.len() > 8 {
        return false;
    }
    let digits = levels(body);
    let letter = body.chars().count() == 1 && body.chars().all(|c| c.is_alphabetic());
    let roman = body.len() <= 6 && body.chars().all(|c| "ivxlcdmIVXLCDM".contains(c));
    digits || letter || roman
}

fn key(t: &str, n: &Norm) -> (String, Option<(f64, i32)>) {
    let mut s = String::with_capacity(t.len());
    for c in t.chars() {
        match c {
            '\u{fb00}' => s.push_str("ff"),
            '\u{fb01}' => s.push_str("fi"),
            '\u{fb02}' => s.push_str("fl"),
            '\u{fb03}' => s.push_str("ffi"),
            '\u{fb04}' => s.push_str("ffl"),
            '\u{fb05}' | '\u{fb06}' => s.push_str("st"),
            '‘' | '’' | '‚' | '‛' | '′' | '`' | '´' => s.push('\''),
            '“' | '”' | '„' | '‟' | '″' => s.push('"'),
            '‐' | '‑' | '‒' | '–' | '—' | '―' | '−' => s.push('-'),
            '…' => s.push_str("..."),
            '\u{ad}' | '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' => {}
            c if !n.case => s.extend(c.to_lowercase()),
            c => s.push(c),
        }
    }
    // Hyphens inside words: "self-contained" and "self-" "contained" joined over two lines read the same.
    if s.contains('-') {
        let ch: Vec<char> = s.chars().collect();
        s = ch.iter().enumerate().filter(|&(i, &c)| !(c == '-' && i > 0 && i + 1 < ch.len() && ch[i - 1].is_alphabetic() && ch[i + 1].is_alphabetic())).map(|(_, c)| *c).collect();
    }
    if n.numbers {
        if let Some((v, dec, pct)) = number(&s) {
            return (format!("#{}{}", format_args!("{:.*}", dec.max(0) as usize, v), if pct { "%" } else { "" }), Some((v, dec)));
        }
    }
    (s, None)
}

/// A number as printed: "1,234.50", "(12)", "$5", "25%" → (value, decimals, percent).
fn number(s: &str) -> Option<(f64, i32, bool)> {
    let t = s.trim_matches(|c: char| "$€£¥₹".contains(c));
    let (t, neg) = match t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        Some(x) => (x, true),
        None => (t, false),
    };
    let (t, pct) = match t.strip_suffix('%') {
        Some(x) => (x, true),
        None => (t, false),
    };
    let t = t.trim_matches(|c: char| "$€£¥₹".contains(c));
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit() || b == b',' || b == b'.' || b == b'-') || !t.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    let plain: String = t.chars().filter(|&c| c != ',').collect();
    let v: f64 = plain.parse().ok()?;
    let dec = plain.split_once('.').map_or(0, |(_, d)| d.len() as i32);
    Some((if neg { -v } else { v }, dec, pct))
}

fn is_currency(t: &str) -> bool {
    t.chars().all(|c| "$€£¥₹".contains(c))
}

fn tokens(s: &Side, n: &Norm, skip_edges: bool) -> Vec<Tok> {
    let mut out = Vec::new();
    match s {
        Side::Doc { pages, sizes, .. } => {
            let keep = if skip_edges { edges(pages, sizes) } else { pages.iter().map(|p| vec![true; p.len()]).collect() };
            for (p, words) in pages.iter().enumerate() {
                let mut i = 0;
                while i < words.len() {
                    if !keep[p][i] {
                        i += 1;
                        continue;
                    }
                    let w = &words[i];
                    let first = w.5 & 3 != 0;
                    // A word broken over two lines with a hyphen: one word.
                    let (text, rest) = if w.5 & 4 != 0 && i + 1 < words.len() && keep[p][i + 1] {
                        let next = &words[i + 1];
                        let lower = next.0.chars().next().is_some_and(|c| c.is_lowercase());
                        i += 1;
                        (if lower { format!("{}{}", w.0.trim_end_matches(|c| "-\u{2010}\u{2011}\u{ad}".contains(c)), next.0) } else { format!("{}{}", w.0, next.0) }, Some(next.r()))
                    } else {
                        (w.0.clone(), None)
                    };
                    i += 1;
                    // Footnotes' numbers: Word keeps them out of the text.
                    let footnote = w.5 & 8 != 0 && text.chars().count() <= 3;
                    if is_bullet(&text) || (n.markers && ((first && is_marker(&text)) || footnote)) || (n.numbers && is_currency(&text)) {
                        continue;
                    }
                    let (key, num) = key(&text, n);
                    if key.is_empty() {
                        continue;
                    }
                    out.push(Tok { key, text, num, at: At::Doc(p as u32, w.r(), rest), line: first });
                }
            }
        }
        Side::Text(blocks) => {
            for (bi, b) in blocks.iter().enumerate() {
                // A slide's number or a sheet's name isn't printed on the page.
                if b.k == "slide" || b.k == "sheet" {
                    continue;
                }
                for (ci, cell) in b.t.iter().enumerate() {
                    let raw = b.nums.get(ci).copied().flatten();
                    let words: Vec<(usize, &str)> = split_words(cell);
                    let numeric = words.iter().filter(|(_, w)| number(w).is_some()).count();
                    let mut first = true;
                    for (off, w) in words {
                        let from = cell[..off].encode_utf16().count() as u32;
                        let to = from + w.encode_utf16().count() as u32;
                        let line = first;
                        first = false;
                        // Typed list numbers ("1. Summary"): the PDF's are left out too.
                        if is_bullet(w) || (n.numbers && is_currency(w)) || (n.markers && line && ci == 0 && is_marker(w)) {
                            continue;
                        }
                        let (key, mut num) = key(w, n);
                        if key.is_empty() {
                            continue;
                        }
                        // The cell's exact value, when its text is one number.
                        if let (Some(v), Some(_), 1) = (raw, num, numeric) {
                            num = Some((v, -1));
                        }
                        out.push(Tok { key, text: w.to_string(), num, at: At::Text(bi as u32, ci as u32, from, to), line });
                    }
                }
            }
        }
    }
    out
}

fn split_words(s: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in s.char_indices() {
        if c.is_whitespace() {
            if let Some(st) = start.take() {
                out.push((st, &s[st..i]));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(st) = start {
        out.push((st, &s[st..]));
    }
    out
}

/// Which words to keep: running headers and footers (the same text near the top or bottom of most pages, numbers
/// aside) and page numbers are left out.
fn edges(pages: &[Vec<Word>], sizes: &[[f32; 2]]) -> Vec<Vec<bool>> {
    // Lines: runs of words from one that starts a line.
    let lines_of = |words: &[Word]| -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut start = 0;
        for i in 1..=words.len() {
            if i == words.len() || words[i].5 & 3 != 0 {
                out.push((start, i));
                start = i;
            }
        }
        out.into_iter().filter(|(a, b)| a < b).collect()
    };
    let near_edge = |words: &[Word], (a, b): (usize, usize), h: f32| {
        let y0 = words[a..b].iter().map(|w| w.2).fold(f32::MAX, f32::min);
        let y1 = words[a..b].iter().map(|w| w.4).fold(f32::MIN, f32::max);
        y1 < h * 0.12 || y0 > h * 0.88
    };
    let shape = |words: &[Word], (a, b): (usize, usize)| -> String {
        words[a..b].iter().map(|w| w.0.to_lowercase()).collect::<Vec<_>>().join(" ").chars().filter(|c| !c.is_ascii_digit()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let page_number = |words: &[Word], (a, b): (usize, usize)| {
        let t = words[a..b].iter().map(|w| w.0.to_lowercase()).collect::<Vec<_>>().join(" ");
        let t = t.trim_matches(|c: char| c == '-' || c == '–' || c == '|' || c.is_whitespace());
        let t = t.strip_prefix("page").unwrap_or(t).trim();
        !t.is_empty() && t.split(['/', ' ']).filter(|w| *w != "of" && !w.is_empty()).all(|w| w.chars().all(|c| c.is_ascii_digit()) || w.chars().all(|c| "ivxlcdm".contains(c)))
    };
    let mut count: HashMap<String, usize> = HashMap::new();
    for (p, words) in pages.iter().enumerate() {
        let mut seen = std::collections::HashSet::new();
        for l in lines_of(words) {
            if near_edge(words, l, sizes[p][1]) {
                let s = shape(words, l);
                if s.chars().any(char::is_alphabetic) && seen.insert(s.clone()) {
                    *count.entry(s).or_default() += 1;
                }
            }
        }
    }
    let need = (pages.len() / 2).max(2);
    pages
        .iter()
        .enumerate()
        .map(|(p, words)| {
            let mut keep = vec![true; words.len()];
            for l in lines_of(words) {
                if near_edge(words, l, sizes[p][1]) {
                    let s = shape(words, l);
                    let repeated = pages.len() >= 2 && s.chars().any(char::is_alphabetic) && count.get(&s).copied().unwrap_or(0) >= need;
                    if repeated || page_number(words, l) {
                        keep[l.0..l.1].iter_mut().for_each(|k| *k = false);
                    }
                }
            }
            keep
        })
        .collect()
}

// ---------------------------------------------------------------- lining up

/// A word as compared: its key, and for spreadsheets numbers that round to each other.
struct K<'a>(&'a Tok, bool);

impl PartialEq for K<'_> {
    fn eq(&self, o: &Self) -> bool {
        if self.0.key == o.0.key {
            return true;
        }
        if !self.1 {
            return false;
        }
        let (x, y) = (&self.0.key, &o.0.key);
        match (x.contains('\u{fffd}'), y.contains('\u{fffd}')) {
            (true, false) => return wildcard(x, y),
            (false, true) => return wildcard(y, x),
            (true, true) => return false,
            _ => {}
        }
        match (self.0.num, o.0.num) {
            (Some((x, dx)), Some((y, dy))) if dx < 0 || dy < 0 => {
                // One side's exact value against a number as printed, rounded to its decimals.
                let dec = if dx < 0 { dy } else { dx };
                let dec = if dx < 0 && dy < 0 { 9 } else { dec };
                (x - y).abs() <= 0.5 * 10f64.powi(-dec) + 1e-9 * x.abs().max(1.0)
            }
            _ => false,
        }
    }
}
impl Eq for K<'_> {}
impl Hash for K<'_> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.0.key.hash(h)
    }
}

/// `pattern` with each U+FFFD (letters a PDF lost) standing for one to three characters of `s`.
fn wildcard(pattern: &str, s: &str) -> bool {
    fn go(p: &[char], s: &[char]) -> bool {
        match p.split_first() {
            None => s.is_empty(),
            Some(('\u{fffd}', rest)) => (1..=3.min(s.len())).any(|k| go(rest, &s[k..])),
            Some((c, rest)) => s.first() == Some(c) && go(rest, &s[1..]),
        }
    }
    let (p, s): (Vec<char>, Vec<char>) = (pattern.chars().collect(), s.chars().collect());
    s.len() <= p.len() * 3 && go(&p, &s)
}

fn pos(t: &Tok) -> [f32; 2] {
    match t.at {
        At::Doc(p, r, _) => [p as f32, r[1]],
        At::Text(b, ..) => [b as f32, 0.0],
    }
}

/// Changed stretches, with one matching word between two changes taken into them.
fn regions(ops: &[DiffOp]) -> Vec<(usize, usize, usize, usize)> {
    let mut out: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut gap: Option<(usize, usize)> = None; // the last equal run's length, if short
    for op in ops {
        let (oi, ol, ni, nl) = match *op {
            DiffOp::Equal { len, .. } => {
                gap = Some((len, 0));
                continue;
            }
            DiffOp::Delete { old_index, old_len, new_index } => (old_index, old_len, new_index, 0),
            DiffOp::Insert { old_index, new_index, new_len } => (old_index, 0, new_index, new_len),
            DiffOp::Replace { old_index, old_len, new_index, new_len } => (old_index, old_len, new_index, new_len),
        };
        match (out.last_mut(), gap) {
            (Some(last), Some((len, _))) if len <= 1 => {
                last.1 = oi + ol;
                last.3 = ni + nl;
            }
            (Some(last), None) => {
                last.1 = oi + ol;
                last.3 = ni + nl;
            }
            _ => out.push((oi, oi + ol, ni, ni + nl)),
        }
        gap = None;
    }
    out
}

fn text_of(t: &[Tok], idx: &[usize]) -> String {
    let mut s = String::new();
    for &i in idx {
        if s.len() > 300 {
            s.push('…');
            break;
        }
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&t[i].text);
    }
    s
}

/// The marks of words `idx`; with none, where the other side's words would go (before word `at`).
fn marks(t: &[Tok], idx: &[usize], at: usize) -> Marks {
    let mut m = Marks::default();
    let Some(&first) = idx.first() else {
        // After the word before, else before the word after.
        let (tok, after) = match (at.checked_sub(1).and_then(|i| t.get(i)), t.get(at)) {
            (Some(prev), _) => (prev, true),
            (None, Some(next)) => (next, false),
            (None, None) => return m,
        };
        match tok.at {
            At::Doc(p, r, rest) => {
                let r = if after { rest.unwrap_or(r) } else { r };
                let x = if after { r[2] } else { r[0] };
                m.boxes.push([p as f32, x, r[1], x, r[3]]);
            }
            At::Text(b, c, f, e) => {
                let x = if after { e } else { f };
                m.spans.push([b, c, x, x]);
            }
        }
        m.at = pos(tok);
        return m;
    };
    m.at = pos(&t[first]);
    for &i in idx {
        match t[i].at {
            At::Doc(p, r, rest) => {
                for r in std::iter::once(r).chain(rest) {
                    let pf = p as f32;
                    // Joined to the box before it when on the same line.
                    match m.boxes.last_mut() {
                        Some(last) if last[0] == pf && (last[2] - r[1]).abs() < 2.0 && (last[4] - r[3]).abs() < 2.0 && r[0] >= last[1] - 1.0 && r[0] - last[3] < (r[3] - r[1]) * 3.0 => {
                            last[3] = last[3].max(r[2]);
                            last[2] = last[2].min(r[1]);
                            last[4] = last[4].max(r[3]);
                        }
                        _ => m.boxes.push([pf, r[0], r[1], r[2], r[3]]),
                    }
                }
            }
            At::Text(b, c, f, e) => match m.spans.last_mut() {
                Some(last) if last[0] == b && last[1] == c && last[3] <= f => last[3] = e,
                _ => m.spans.push([b, c, f, e]),
            },
        }
    }
    m
}

/// Where words read from a file are: their slide, or their sheet and cells.
fn place(blocks: Option<&[Block]>, t: &[Tok], idx: &[usize], at: usize) -> Option<String> {
    let blocks = blocks?;
    let first = idx.first().copied().or_else(|| at.checked_sub(1)).or(Some(at))?;
    let At::Text(b, c, ..) = t.get(first)?.at else { return None };
    let head = blocks[..b as usize].iter().rev().find(|x| x.k == "slide" || x.k == "sheet")?;
    if head.k != "sheet" {
        return Some(head.t[0].clone());
    }
    let cell = |b: u32, c: u32| blocks[b as usize].refs.get(c as usize).cloned();
    let first_ref = cell(b, c)?;
    let last_ref = idx.last().and_then(|&i| match t[i].at {
        At::Text(lb, lc, ..) if (lb, lc) != (b, c) => cell(lb, lc),
        _ => None,
    });
    Some(match last_ref {
        Some(l) => format!("{} · {first_ref}–{l}", head.t[0]),
        None => format!("{} · {first_ref}", head.t[0]),
    })
}

fn changes_of(ops: &[DiffOp], a: &[Tok], b: &[Tok], blocks_a: Option<&[Block]>, blocks_b: Option<&[Block]>, sheets: bool) -> Vec<Change> {
    // Per changed stretch: the words left unmatched on each side, and where it starts on each.
    let mut left: Vec<(Vec<usize>, Vec<usize>, usize, usize)> = regions(ops).into_iter().map(|(a0, a1, b0, b1)| ((a0..a1).collect(), (b0..b1).collect(), a0, b0)).collect();
    let mut moved: Vec<(usize, usize)> = Vec::new();
    if sheets {
        // A printout splits a wide sheet into pages column by column, so cells turn up elsewhere: a word left over on
        // one side that matches one left over on the other is the same cell.
        let mut by_key: HashMap<&str, VecDeque<(usize, usize)>> = HashMap::new();
        for (r, l) in left.iter().enumerate() {
            for (k, &j) in l.1.iter().enumerate() {
                by_key.entry(b[j].key.as_str()).or_default().push_back((r, k));
            }
        }
        let mut a_used: Vec<Vec<bool>> = left.iter().map(|l| vec![false; l.0.len()]).collect();
        let mut b_used: Vec<Vec<bool>> = left.iter().map(|l| vec![false; l.1.len()]).collect();
        for (r, l) in left.iter().enumerate() {
            for (k, &i) in l.0.iter().enumerate() {
                if let Some(q) = by_key.get_mut(a[i].key.as_str()) {
                    if let Some((rb, kb)) = q.pop_front() {
                        b_used[rb][kb] = true;
                        a_used[r][k] = true;
                    }
                }
            }
        }
        // Numbers shown rounded.
        let mut spare: Vec<(usize, usize)> = Vec::new();
        for (r, l) in left.iter().enumerate() {
            for (k, &j) in l.1.iter().enumerate() {
                if !b_used[r][k] && b[j].num.is_some() {
                    spare.push((r, k));
                }
            }
        }
        for (r, l) in left.iter().enumerate() {
            for (k, &i) in l.0.iter().enumerate() {
                if a_used[r][k] || a[i].num.is_none() {
                    continue;
                }
                if let Some(pos) = spare.iter().position(|&(rb, kb)| !b_used[rb][kb] && K(&a[i], true) == K(&b[left[rb].1[kb]], true)) {
                    let (rb, kb) = spare.swap_remove(pos);
                    b_used[rb][kb] = true;
                    a_used[r][k] = true;
                }
            }
        }
        for (r, l) in left.iter_mut().enumerate() {
            let (au, bu) = (&a_used[r], &b_used[r]);
            l.0 = l.0.iter().enumerate().filter(|(k, _)| !au[*k]).map(|(_, &i)| i).collect();
            l.1 = l.1.iter().enumerate().filter(|(k, _)| !bu[*k]).map(|(_, &j)| j).collect();
        }
    } else {
        // Words taken out in one place and put back in another: moved.
        let keys = |t: &[Tok], idx: &[usize]| idx.iter().map(|&i| t[i].key.as_str()).collect::<Vec<_>>().join(" ");
        let mut added: HashMap<String, Vec<usize>> = HashMap::new();
        for (r, l) in left.iter().enumerate() {
            if l.0.is_empty() && l.1.len() >= 3 {
                added.entry(keys(b, &l.1)).or_default().push(r);
            }
        }
        for (r, l) in left.iter().enumerate() {
            if l.1.is_empty() && l.0.len() >= 3 {
                if let Some(s) = added.get_mut(&keys(a, &l.0)).and_then(|v| v.pop()) {
                    moved.push((r, s));
                }
            }
        }
    }
    let moved_to: HashMap<usize, usize> = moved.iter().copied().collect();
    let moved_from: HashSet<usize> = moved.iter().map(|m| m.1).collect();
    let mut out = Vec::new();
    for (r, (ai, bi, a0, b0)) in left.iter().enumerate() {
        if moved_from.contains(&r) || (ai.is_empty() && bi.is_empty()) {
            continue;
        }
        let (kind, bi, b0) = match moved_to.get(&r) {
            Some(&s) => ("moved", &left[s].1, left[s].3),
            None if ai.is_empty() => ("added", bi, *b0),
            None if bi.is_empty() => ("removed", bi, *b0),
            None => ("changed", bi, *b0),
        };
        out.push(Change {
            kind,
            a: marks(a, ai, *a0),
            b: marks(b, bi, b0),
            old: text_of(a, ai),
            new: text_of(b, bi),
            place: place(blocks_b, b, bi, b0).or_else(|| place(blocks_a, a, ai, *a0)),
        });
    }
    out
}

fn anchors_of(ops: &[DiffOp], a: &[Tok], b: &[Tok]) -> Vec<[f32; 4]> {
    let mut out: Vec<[f32; 4]> = Vec::new();
    for op in ops {
        if let DiffOp::Equal { old_index, new_index, len } = *op {
            for k in 0..len {
                let (x, y) = (&a[old_index + k], &b[new_index + k]);
                if !(x.line || y.line) {
                    continue;
                }
                let (pa, pb) = (pos(x), pos(y));
                if out.last().is_some_and(|l| (l[0] == pa[0] && (l[1] - pa[1]).abs() < 1.0) || (l[2] == pb[0] && (l[3] - pb[1]).abs() < 1.0)) {
                    continue;
                }
                out.push([pa[0], pa[1], pb[0], pb[1]]);
            }
        }
    }
    // Plenty for scrolling; keep the list small for very long documents.
    if out.len() > 40_000 {
        let step = out.len().div_ceil(40_000);
        out = out.into_iter().step_by(step).collect();
    }
    out
}

// ---------------------------------------------------------------- drawings and scans

/// Each left page and the right page sharing the most of its words (pages without words follow their
/// neighbours); only pages of the same size, since drawings are compared where they lie.
fn page_pairs(ops: &[DiffOp], a: &[Tok], b: &[Tok], za: &[[f32; 2]], zb: &[[f32; 2]]) -> Vec<(usize, usize)> {
    let mut count: HashMap<(u32, u32), usize> = HashMap::new();
    for op in ops {
        if let DiffOp::Equal { old_index, new_index, len } = *op {
            for k in 0..len {
                if let (At::Doc(pa, ..), At::Doc(pb, ..)) = (a[old_index + k].at, b[new_index + k].at) {
                    *count.entry((pa, pb)).or_default() += 1;
                }
            }
        }
    }
    let mut best: Vec<Option<(usize, usize)>> = vec![None; za.len()];
    for (&(pa, pb), &n) in &count {
        let slot = &mut best[pa as usize];
        if slot.map_or(true, |(_, m)| n > m) {
            *slot = Some((pb as usize, n));
        }
    }
    let mut out = Vec::new();
    let mut last: Option<(usize, usize)> = None;
    for pa in 0..za.len() {
        let pb = match best[pa] {
            Some((pb, _)) => Some(pb),
            None => match last {
                Some((la, lb)) => Some(lb + (pa - la)),
                None => Some(pa),
            },
        };
        if let Some(pb) = pb.filter(|&pb| pb < zb.len()) {
            last = Some((pa, pb));
            if (za[pa][0] - zb[pb][0]).abs() <= 2.0 && (za[pa][1] - zb[pb][1]).abs() <= 2.0 {
                out.push((pa, pb));
            }
        }
    }
    out
}

const DPI: f32 = 50.0;
const CELL: usize = 6;

fn gray(pnm: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    // "P5"/"P6", width, height, maxval, one whitespace, then the samples.
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 && i < pnm.len() {
        while i < pnm.len() && pnm[i].is_ascii_whitespace() {
            i += 1;
        }
        let s = i;
        while i < pnm.len() && !pnm[i].is_ascii_whitespace() {
            i += 1;
        }
        fields.push(std::str::from_utf8(&pnm[s..i]).ok()?);
    }
    let (w, h): (usize, usize) = (fields.get(1)?.parse().ok()?, fields.get(2)?.parse().ok()?);
    let data = pnm.get(i + 1..)?;
    match *fields.first()? {
        "P5" => Some((w, h, data.get(..w * h)?.to_vec())),
        "P6" => {
            let d = data.get(..w * h * 3)?;
            Some((w, h, d.chunks_exact(3).map(|p| ((p[0] as u32 * 30 + p[1] as u32 * 59 + p[2] as u32 * 11) / 100) as u8).collect()))
        }
        _ => None,
    }
}

type Found = Vec<(usize, usize, Vec<[f32; 4]>)>;

#[allow(clippy::too_many_arguments)]
fn pictures(engine: &Arc<Engine>, da: DocId, db: DocId, pairs: &[(usize, usize)], wa: &[Vec<Word>], wb: &[Vec<Word>], cancel: &AtomicBool, progress: &(dyn Fn(Progress) + Sync)) -> Result<Found, String> {
    let (a, b) = (engine.doc(da)?, engine.doc(db)?);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done = std::sync::atomic::AtomicUsize::new(0);
    let found = parking_lot::Mutex::new(Vec::new());
    let error = parking_lot::Mutex::new(None::<String>);
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get() / 2).clamp(1, 8);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                // Each thread draws from copies of its own: drawing through the shared documents would wait on
                // their locks.
                let copies = a.private_copy().and_then(|x| Ok((x, b.private_copy()?)));
                let (ca, cb) = match copies {
                    Ok(c) => c,
                    Err(e) => return *error.lock() = Some(e),
                };
                loop {
                    let k = next.fetch_add(1, Ordering::AcqRel);
                    if k >= pairs.len() || cancel.load(Ordering::Acquire) || error.lock().is_some() {
                        break;
                    }
                    let (pa, pb) = pairs[k];
                    match page_differences((&a, ca.as_ref()), (&b, cb.as_ref()), pa, pb, &wa[pa], &wb[pb]) {
                        Ok(rects) if !rects.is_empty() => found.lock().push((pa, pb, rects)),
                        Ok(_) => {}
                        Err(e) => *error.lock() = Some(e),
                    }
                    progress(Progress::Pictures(done.fetch_add(1, Ordering::AcqRel) + 1, pairs.len()));
                }
            });
        }
    });
    if cancel.load(Ordering::Acquire) {
        return Err("stopped".into());
    }
    if let Some(e) = error.into_inner() {
        return Err(e);
    }
    let mut found = found.into_inner();
    found.sort_by_key(|f| f.0);
    Ok(found)
}

type Source<'a> = (&'a crate::engine::Doc, Option<&'a Document>);

/// Page `i` drawn at DPI (with its comments), from the thread's own copy when there is one.
fn draw(src: Source, i: usize) -> Result<Vec<u8>, String> {
    match src.1 {
        Some(doc) => {
            let page = doc.load_page(i)?;
            let b = page.bounds()?;
            page.display_list(1)?.area_image(&b, &[0.0, 0.0, b[2] - b[0], b[3] - b[1]], 0, DPI, 3)
        }
        None => src.0.page_image(i, 0, "pnm", DPI),
    }
}

/// Areas away from any text where the two pages' pixels differ, in page points.
fn page_differences(a: Source, b: Source, pa: usize, pb: usize, wa: &[Word], wb: &[Word]) -> Result<Vec<[f32; 4]>, String> {
    let (Some((w1, h1, g1)), Some((w2, h2, g2))) = (gray(&draw(a, pa)?), gray(&draw(b, pb)?)) else {
        return Ok(Vec::new());
    };
    let (w, h) = (w1.min(w2), h1.min(h2));
    let s = DPI / 72.0;
    // Text is compared as words; its pixels (on either page) are left out.
    let mut mask = vec![false; w * h];
    for word in wa.iter().chain(wb) {
        let x0 = ((word.1 * s) as isize - 2).clamp(0, w as isize) as usize;
        let y0 = ((word.2 * s) as isize - 2).clamp(0, h as isize) as usize;
        let x1 = ((word.3 * s).ceil() as isize + 2).clamp(0, w as isize) as usize;
        let y1 = ((word.4 * s).ceil() as isize + 2).clamp(0, h as isize) as usize;
        for y in y0..y1 {
            mask[y * w + x0..y * w + x1].iter_mut().for_each(|m| *m = true);
        }
    }
    // A pixel differs when its shade isn't found next to it on the other page either (a line drawn a pixel
    // off by another program isn't a change).
    let near = |g: &[u8], gw: usize, x: usize, y: usize, v: u8| {
        (y.saturating_sub(1)..(y + 2).min(h)).any(|ny| (x.saturating_sub(1)..(x + 2).min(w)).any(|nx| g[ny * gw + nx].abs_diff(v) <= 48))
    };
    let (cw, ch) = (w.div_ceil(CELL), h.div_ceil(CELL));
    let mut cells = vec![false; cw * ch];
    let (mut any, mut open) = (0, 0);
    for cy in 0..ch {
        for cx in 0..cw {
            let (mut n, mut free) = (0, 0);
            for y in cy * CELL..((cy + 1) * CELL).min(h) {
                for x in cx * CELL..((cx + 1) * CELL).min(w) {
                    if mask[y * w + x] {
                        continue;
                    }
                    free += 1;
                    let (v1, v2) = (g1[y * w1 + x], g2[y * w2 + x]);
                    if v1.abs_diff(v2) > 48 && !(near(&g2, w2, x, y, v1) && near(&g1, w1, x, y, v2)) {
                        n += 1;
                    }
                }
            }
            open += (free * 2 >= CELL * CELL) as usize;
            if n >= 4 {
                cells[cy * cw + cx] = true;
                any += 1;
            }
        }
    }
    if any == 0 {
        return Ok(Vec::new());
    }
    let to_pt = |c: usize| (c * CELL) as f32 / s;
    // Much of the page differs (another scan, say): one box for the page.
    if any * 4 > open.max(1) {
        return Ok(vec![[0.0, 0.0, to_pt(cw).min(w as f32 / s), to_pt(ch).min(h as f32 / s)]]);
    }
    // Groups of changed cells (two cells apart still one group); specks are left out.
    let mut seen = vec![false; cw * ch];
    let mut out = Vec::new();
    for start in 0..cells.len() {
        if !cells[start] || seen[start] {
            continue;
        }
        let (mut x0, mut y0, mut x1, mut y1, mut n) = (usize::MAX, usize::MAX, 0, 0, 0);
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let (cx, cy) = (i % cw, i / cw);
            (x0, y0, x1, y1, n) = (x0.min(cx), y0.min(cy), x1.max(cx), y1.max(cy), n + 1);
            for dy in -2isize..=2 {
                for dx in -2isize..=2 {
                    let (nx, ny) = (cx as isize + dx, cy as isize + dy);
                    if nx < 0 || ny < 0 || nx >= cw as isize || ny >= ch as isize {
                        continue;
                    }
                    let j = ny as usize * cw + nx as usize;
                    if cells[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        // A long thin line counts; a short sliver (a border drawn a little longer by another program) does not.
        if n >= 3 && ((x1 - x0).min(y1 - y0) >= 1 || n >= 8) {
            out.push([to_pt(x0), to_pt(y0), to_pt(x1 + 1), to_pt(y1 + 1)]);
        }
    }
    if out.len() > 12 {
        let r = out.iter().fold([f32::MAX, f32::MAX, 0f32, 0f32], |m, r| [m[0].min(r[0]), m[1].min(r[1]), m[2].max(r[2]), m[3].max(r[3])]);
        out = vec![r];
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm() -> Norm {
        Norm { case: true, numbers: false, markers: false }
    }

    #[test]
    fn keys_ignore_typography() {
        assert_eq!(key("“ﬁne”", &norm()).0, "\"fine\"");
        assert_eq!(key("self-contained", &norm()).0, key("selfcontained", &norm()).0);
        assert_eq!(key("2020–2021", &norm()).0, "2020-2021");
        let n = Norm { case: false, numbers: true, markers: false };
        assert_eq!(key("$1,234.50", &n), ("#1234.50".into(), Some((1234.5, 2))));
        assert_eq!(key("(12)", &n).0, "#-12");
        assert!(is_marker("1.") && is_marker("(a)") && is_marker("iv.") && is_marker("2.3.1") && !is_marker("Hello.") && !is_marker("2020"));
        assert!(is_bullet("•") && !is_bullet("a"));
    }

    #[test]
    fn nearby_changes_are_one() {
        let ops = vec![
            DiffOp::Equal { old_index: 0, new_index: 0, len: 3 },
            DiffOp::Replace { old_index: 3, old_len: 1, new_index: 3, new_len: 1 },
            DiffOp::Equal { old_index: 4, new_index: 4, len: 1 },
            DiffOp::Delete { old_index: 5, old_len: 1, new_index: 5 },
            DiffOp::Equal { old_index: 6, new_index: 5, len: 5 },
            DiffOp::Insert { old_index: 11, new_index: 10, new_len: 2 },
        ];
        assert_eq!(regions(&ops), vec![(3, 6, 3, 5), (11, 11, 10, 12)]);
    }
}
