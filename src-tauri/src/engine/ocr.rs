//! Text recognition (OCR): pages are recognised in parallel, each worker with its
//! own Tesseract, and the text is added to the pages as an invisible layer (like
//! scanners and Adobe's "Recognize text" do), so search, selection, copy and
//! Read mode work on it and saving keeps it in the file.

use super::mupdf::Tess;
use super::{Doc, DocId, Engine};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Pages are read at this resolution: what Tesseract's models were trained on.
const DPI: f32 = 300.0;

/// Words Tesseract is less sure of than this are left out (mostly specks and
/// patterns in photos read as letters).
const MIN_CONFIDENCE: f64 = 25.0;

/// The language models for a job: Tesseract's language string ("eng+hin") and the
/// file for each language.
#[derive(Clone, Debug)]
pub struct Models {
    pub langs: String,
    pub files: Vec<(String, String)>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct OcrEnd {
    /// Every page asked for was read (not stopped or failed).
    pub complete: bool,
    /// Pages that got text.
    pub pages: usize,
    /// Of those, pages whose earlier recognised text was replaced.
    pub replaced: usize,
    /// Pages that read the right way up only when turned: (page, clockwise turn in degrees).
    pub turned: Vec<(usize, u32)>,
    pub words: usize,
    /// The document changed (a new undo step).
    pub changed: bool,
    /// The document now differs from the file on disk.
    pub dirty: bool,
    pub error: Option<String>,
}

/// A page's recognised text.
struct PageText {
    content: String,
    words: usize,
    /// It replaces the page's earlier OCR layer.
    redo: bool,
    /// The page read the right way up turned this far clockwise.
    rot: u32,
}

/// How many pages have something to read but (almost) no text, among a sample.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanCheck {
    pub sampled: usize,
    pub scans: usize,
}

thread_local! {
    /// This worker's recogniser, for the models it was loaded with.
    static TESS: RefCell<Option<(String, Tess)>> = const { RefCell::new(None) };
}

fn with_tess<R>(m: &Models, f: impl FnOnce(&Tess) -> Result<R, String>) -> Result<R, String> {
    TESS.with(|slot| {
        let mut slot = slot.borrow_mut();
        let key = format!("{:?}", m.files);
        if slot.as_ref().map(|(k, _)| k != &key).unwrap_or(true) {
            *slot = None;
            *slot = Some((key, Tess::new(&m.langs, &m.files)?));
        }
        f(&slot.as_ref().unwrap().1)
    })
}

fn drop_tess() {
    TESS.with(|slot| *slot.borrow_mut() = None);
}

#[derive(Deserialize)]
struct PageJson {
    #[serde(default)]
    l: Vec<LineJson>,
    #[serde(default)]
    blank: u8,
    /// The words are on the page turned this far clockwise (it was sideways or upside down).
    #[serde(default)]
    rot: u32,
}

#[derive(Deserialize)]
struct LineJson {
    b: [f32; 4],
    bl: [f32; 4],
    w: Vec<(f32, f32, f32, f32, f64, String)>,
}

fn is_rtl(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
}

/// Where a recognised line sits: its baseline (skew followed) and text size.
struct LineGeom {
    sin: f32,
    cos: f32,
    tan: f32,
    /// A point on the baseline.
    bx: f32,
    by: f32,
    /// Distance from the baseline down to the bottom of the descenders.
    descent: f32,
    size: f32,
}

impl LineGeom {
    fn new(line: &LineJson) -> LineGeom {
        let [lx0, ly0, lx1, ly1] = line.b;
        let [bx1, by1, bx2, by2] = line.bl;
        let mut angle = if bx2 > bx1 { ((by2 - by1) / (bx2 - bx1)).atan() } else { 0.0 };
        if angle.abs() > 0.35 {
            angle = 0.0;
        }
        let (sin, cos) = angle.sin_cos();
        let (bx, by) = if bx2 > bx1 { (bx1, by1) } else { (lx0, ly1) };
        let g = LineGeom { sin, cos, tan: angle.tan(), bx, by, descent: 0.0, size: 1.0 };
        // Glyph boxes run from the bottom of the descenders up to the top of the line.
        let low = g.base(lx0).max(g.base(lx1));
        let high = g.base(lx0).min(g.base(lx1));
        let descent = (ly1 - low).max(0.0);
        LineGeom { descent, size: ((high - ly0) + descent).max(1.0), ..g }
    }
    fn base(&self, x: f32) -> f32 {
        self.by + (x - self.bx) * self.tan
    }
}

type Word = (f32, f32, f32, f32, f64, String);

/// Part of a recognised line: Tesseract sometimes reads across a gutter, joining a
/// line of one column to the line beside it in the next.
struct Seg<'a> {
    line: usize,
    words: Vec<&'a Word>,
    b: [f32; 4],
}

/// Reading order for line pieces: recursive XY-cut. A group is split at an empty
/// vertical gutter (columns, left to right) when both sides are at least a fifth of
/// its width, so line numbers, margin notes and table columns keep their rows;
/// otherwise at its widest empty horizontal gap; pieces left in a row go left to right.
/// A single column of full-width lines never has a gutter, so it reads top to bottom.
fn xy_order(segs: &[Seg], items: Vec<usize>, gutter: f32, out: &mut Vec<usize>) {
    if items.len() <= 1 {
        out.extend(items);
        return;
    }
    // Widest empty gap along an axis: (gap, position to split at).
    let widest = |axis: usize| -> Option<(f32, f32)> {
        let mut spans: Vec<(f32, f32)> = items.iter().map(|&i| (segs[i].b[axis], segs[i].b[axis + 2])).collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut end = spans[0].1;
        let mut best: Option<(f32, f32)> = None;
        for &(a, b) in &spans[1..] {
            if a > end && best.map(|(g, _)| a - end > g).unwrap_or(true) {
                best = Some((a - end, (a + end) / 2.0));
            }
            end = end.max(b);
        }
        best
    };
    let x0 = items.iter().map(|&i| segs[i].b[0]).fold(f32::MAX, f32::min);
    let x1 = items.iter().map(|&i| segs[i].b[2]).fold(f32::MIN, f32::max);
    let split = |axis: usize, at: f32| -> (Vec<usize>, Vec<usize>) { items.iter().partition(|&&i| segs[i].b[axis + 2] <= at) };
    if let Some((gap, at)) = widest(0) {
        let min = (x1 - x0) * 0.2;
        if gap >= gutter && at - x0 >= min && x1 - at >= min {
            let (a, b) = split(0, at);
            xy_order(segs, a, gutter, out);
            xy_order(segs, b, gutter, out);
            return;
        }
    }
    if let Some((_, at)) = widest(1) {
        let (a, b) = split(1, at);
        xy_order(segs, a, gutter, out);
        xy_order(segs, b, gutter, out);
        return;
    }
    // One row (pieces overlapping vertically): left to right.
    let mut items = items;
    items.sort_by(|&a, &b| segs[a].b[0].total_cmp(&segs[b].b[0]));
    out.extend(items);
}

/// PDF operators for the recognised lines (page space as shown: origin top-left,
/// y down; `o` is the page origin). Each word is stretched to its box on the
/// line's baseline; a trailing space keeps copied text readable. Lines read across
/// a gutter are split there, and the pieces go in reading order (see xy_order):
/// MuPDF groups text into paragraphs in the order it's written. Returns the
/// content and the number of words.
fn page_content(lines: &[LineJson], o: [f32; 2]) -> (String, usize) {
    let geoms: Vec<LineGeom> = lines.iter().map(LineGeom::new).collect();
    let mut segs: Vec<Seg> = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        let size = geoms[li].size;
        let mut cur: Option<Seg> = None;
        for w in line.w.iter().filter(|w| w.4 >= MIN_CONFIDENCE && w.5.chars().any(|c| c.is_alphanumeric()) || w.4 >= 80.0) {
            match cur.as_mut() {
                // A gap over two text heights is a gutter, not a space.
                Some(seg) if w.0 - seg.b[2] <= 2.0 * size => {
                    seg.words.push(w);
                    seg.b = [seg.b[0].min(w.0), seg.b[1].min(w.1), seg.b[2].max(w.2), seg.b[3].max(w.3)];
                }
                _ => {
                    segs.extend(cur.take());
                    cur = Some(Seg { line: li, words: vec![w], b: [w.0, w.1, w.2, w.3] });
                }
            }
        }
        segs.extend(cur);
    }
    // A gutter is at least as wide as the usual text height.
    let mut sizes: Vec<f32> = segs.iter().map(|s| geoms[s.line].size).collect();
    sizes.sort_by(|a, b| a.total_cmp(b));
    let gutter = sizes.get(sizes.len() / 2).copied().unwrap_or(10.0);
    let mut order = Vec::with_capacity(segs.len());
    xy_order(&segs, (0..segs.len()).collect(), gutter, &mut order);

    let mut s = String::new();
    let mut words = 0;
    for i in order {
        let seg = &segs[i];
        let g = &geoms[seg.line];
        let (sin, cos, size) = (g.sin, g.cos, g.size);
        let _ = write!(s, "BT\n3 Tr\n/PgnOCR {size:.2} Tf\n");
        for (k, w) in seg.words.iter().enumerate() {
            let text = &w.5;
            let units: Vec<u16> = text.encode_utf16().collect();
            let n = units.len().max(1) as f32;
            let width = (w.2 - w.0).max(0.5) / cos.max(0.5);
            let scale = width / (n * 0.5 * size) * 100.0;
            let x = w.0 + o[0];
            let y = g.base(w.0) + g.descent + o[1];
            let _ = write!(s, "{cos:.4} {sin:.4} {sin:.4} {:.4} {x:.2} {y:.2} Tm\n{scale:.1} Tz\n", -cos);
            let last = k + 1 == seg.words.len();
            if text.chars().any(is_rtl) && !text.chars().any(|c| c.is_ascii_alphanumeric()) {
                // Right-to-left: characters stay in reading order (for copying) but
                // are placed from the right end of the box leftwards.
                s.push('[');
                for (j, u) in units.iter().enumerate() {
                    if j == 0 && units.len() > 1 {
                        let _ = write!(s, "{}", -500 * (units.len() as i32 - 1));
                    } else if j > 0 {
                        s.push_str("1000");
                    }
                    let _ = write!(s, "<{u:04X}>");
                }
                s.push_str("]TJ\n");
                if !last {
                    s.push_str("<0020>Tj\n");
                }
            } else {
                s.push('<');
                for u in &units {
                    let _ = write!(s, "{u:04X}");
                }
                if !last {
                    s.push_str("0020");
                }
                s.push_str(">Tj\n");
            }
            words += 1;
        }
        s.push_str("ET\n");
    }
    (s, words)
}

/// The text of a page that was read turned `rot` degrees clockwise: written the way it reads, then turned
/// back onto the page as it is (page space as shown, `b` its bounds).
fn turned_content(lines: &[LineJson], b: &[f32; 4], rot: u32) -> (String, usize) {
    let (w, h) = (b[2] - b[0], b[3] - b[1]);
    // From the turned page (x', y') to the page: x = a x' + c y' + e, y = b x' + d y' + f.
    let [a, bb, c, d, e, f] = match rot {
        90 => [0.0, -1.0, 1.0, 0.0, 0.0, h],
        180 => [-1.0, 0.0, 0.0, -1.0, w, h],
        270 => [0.0, 1.0, -1.0, 0.0, w, 0.0],
        _ => return page_content(lines, [b[0], b[1]]),
    };
    let (content, n) = page_content(lines, [0.0, 0.0]);
    if content.is_empty() {
        return (content, n);
    }
    (format!("q\n{a} {bb} {c} {d} {} {} cm\n{content}Q\n", e + b[0], f + b[1]), n)
}

impl Doc {
    /// Samples up to 12 pages spread through the document: how many have something
    /// to read but under 30 characters of text: scans, and also pages whose text is
    /// pictures or outlines (web pages printed to PDF, some designed documents).
    pub fn scan_check(&self) -> Result<ScanCheck, String> {
        if !self.inner.lock().doc.is_pdf() {
            return Ok(ScanCheck { sampled: 0, scans: 0 });
        }
        let n = self.page_count();
        let k = n.min(12);
        let mut scans = 0;
        for j in 0..k {
            let i = j * n / k;
            let b = self.bounds(i)?;
            let list = self.fresh_list(i)?;
            // Ink over 0.3% of the page: more than a logo or a few rules.
            if list.text()?.char_count() < 30 && list.untexted_ink(&b)? >= 30 {
                scans += 1;
            }
        }
        Ok(ScanCheck { sampled: k, scans })
    }

    /// Adds recognised text (content per page) as one undo step; returns (pages, replaced, words).
    fn apply_ocr(&self, results: &BTreeMap<usize, PageText>) -> Result<(usize, usize, usize), String> {
        let mut inner = self.inner.lock();
        let (mut pages, mut replaced, mut words, mut font) = (0, 0, 0, 0);
        inner.doc.begin_op("Recognize text");
        let res = (|| -> Result<(), String> {
            for (&i, r) in results {
                // Nothing read (a page left blank): an earlier layer stays rather than going for nothing.
                if r.content.is_empty() {
                    continue;
                }
                let page = self.page(&mut inner, i)?;
                if r.redo {
                    page.ocr_apply("", &mut font)?;
                    replaced += 1;
                }
                page.ocr_apply(&r.content, &mut font)?;
                pages += 1;
                words += r.words;
            }
            Ok(())
        })();
        if let Err(e) = res {
            // Nothing of a half-added layer stays, and no undo step is left that the app doesn't know of (as
            // annot_edit does): the pages kept in memory go, since some had the text put on them.
            inner.doc.abandon_op();
            inner.pages.clear();
            drop(inner);
            self.content_changed();
            return Err(e);
        }
        inner.doc.end_op();
        drop(inner);
        if pages > 0 {
            self.content_changed();
        }
        Ok((pages, replaced, words))
    }

    /// Pages carrying text Paginaut recognised.
    pub fn ocr_layer_pages(&self) -> Result<usize, String> {
        self.inner.lock().doc.ocr_layer_count()
    }

    /// Removes the text layers Paginaut added (one undo step). Returns the pages changed.
    pub fn remove_ocr(&self) -> Result<usize, String> {
        let mut inner = self.inner.lock();
        if !inner.doc.is_pdf() {
            return Ok(0);
        }
        self.took_out.store(true, std::sync::atomic::Ordering::Release);
        let mut count = 0;
        let mut font = 0;
        inner.doc.begin_op("Remove recognized text");
        let res = (|| -> Result<(), String> {
            for i in 0..self.page_count() {
                let page = self.page(&mut inner, i)?;
                if page.has_ocr_layer() {
                    page.ocr_apply("", &mut font)?;
                    count += 1;
                }
            }
            Ok(())
        })();
        if let Err(e) = res {
            inner.doc.abandon_op();
            inner.pages.clear();
            drop(inner);
            self.content_changed();
            return Err(e);
        }
        inner.doc.end_op();
        drop(inner);
        if count > 0 {
            self.content_changed();
        }
        Ok(count)
    }
}

/// Recognition threads for one job: half the workers (Tesseract keeps a core
/// busy for a second or more per page), at most 8, leaving the rest for drawing.
fn ocr_threads(workers: usize) -> usize {
    std::env::var("PAGINAUT_OCR_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or((workers / 2).clamp(1, 8))
}

impl Engine {
    /// Recognises `pages` (source indices, in the order to read them; usually
    /// starting at the page you're on) and adds their text to the document as one
    /// undo step. Stopping (cancelling `sid`) keeps what was read so far.
    /// `progress(done, total)` runs after each page; `finished` once at the end.
    /// With `replace`, pages Paginaut recognised before are read again and their text replaced.
    #[allow(clippy::too_many_arguments)]
    pub fn ocr(
        self: &Arc<Self>,
        doc: DocId,
        pages: Vec<usize>,
        models: Models,
        replace: bool,
        sid: u64,
        progress: impl Fn(usize, usize) + Send + Sync + 'static,
        finished: impl FnOnce(OcrEnd) + Send + 'static,
    ) -> Result<(), String> {
        let d = self.doc(doc)?;
        if !d.inner.lock().doc.is_pdf() {
            return Err("Only PDFs can take recognised text. Save this file as a PDF first.".into());
        }
        let n = d.page_count();
        let pages: Vec<usize> = pages.into_iter().filter(|&p| p < n).collect();
        let total = pages.len();
        let runners = ocr_threads(self.workers()).min(total).max(1);
        let pages = Arc::new(pages);
        let next = Arc::new(AtomicUsize::new(0));
        let done = Arc::new(AtomicUsize::new(0));
        let remaining = Arc::new(AtomicUsize::new(runners));
        let results: Arc<Mutex<BTreeMap<usize, PageText>>> = Arc::new(Mutex::new(BTreeMap::new()));
        let error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let progress = Arc::new(progress);
        let engine = Arc::downgrade(self);
        let finished = Arc::new(Mutex::new(Some(Box::new(move |cancelled: bool, results: BTreeMap<usize, PageText>, error: Option<String>| {
            let mut end = OcrEnd { complete: !cancelled && error.is_none(), error, ..Default::default() };
            // A closed document takes nothing.
            if let Some(d) = engine.upgrade().and_then(|e| e.doc(doc).ok()) {
                match d.apply_ocr(&results) {
                    Ok((p, r, w)) => {
                        end.pages = p;
                        end.replaced = r;
                        end.turned = results.iter().filter(|(_, t)| t.rot != 0 && !t.content.is_empty()).map(|(&i, t)| (i, t.rot)).collect();
                        end.words = w;
                        end.changed = p > 0;
                        end.dirty = d.has_changes();
                    }
                    Err(e) => {
                        end.complete = false;
                        end.error.get_or_insert(e);
                    }
                }
            }
            finished(end);
        }) as Box<dyn FnOnce(bool, BTreeMap<usize, PageText>, Option<String>) + Send>)));

        for _ in 0..runners {
            let (d, pages, next, done, remaining, results, error, progress, finished, models) = (
                d.clone(),
                pages.clone(),
                next.clone(),
                done.clone(),
                remaining.clone(),
                results.clone(),
                error.clone(),
                progress.clone(),
                finished.clone(),
                models.clone(),
            );
            self.pool.submit(sid, -10, move |ctl| {
                let halted = || ctl.is_cancelled() || error.lock().is_some();
                loop {
                    let k = next.fetch_add(1, Ordering::AcqRel);
                    if k >= total || halted() {
                        break;
                    }
                    let i = pages[k];
                    let res = super::guarded(i, || -> Result<Option<PageText>, String> {
                        let b = d.bounds(i)?;
                        let redo = replace && {
                            let mut inner = d.inner.lock();
                            d.page(&mut inner, i)?.has_ocr_layer()
                        };
                        let list = d.fresh_list(i)?;
                        let Some(json) = with_tess(&models, |t| list.ocr(&b, DPI, t, Some(&ctl.cookie), redo))? else {
                            return Ok(None);
                        };
                        let page: PageJson = serde_json::from_str(&json).map_err(|e| e.to_string())?;
                        if page.blank == 1 || page.l.is_empty() {
                            return Ok(Some(PageText { content: String::new(), words: 0, redo, rot: 0 }));
                        }
                        let (content, words) = turned_content(&page.l, &b, page.rot);
                        Ok(Some(PageText { content, words, redo, rot: page.rot }))
                    });
                    match res {
                        Ok(Some(r)) => {
                            results.lock().insert(i, r);
                            progress(done.fetch_add(1, Ordering::AcqRel) + 1, total);
                        }
                        Ok(None) => break,
                        Err(e) => {
                            error.lock().get_or_insert(format!("page {}: {e}", i + 1));
                        }
                    }
                }
                // The models go as soon as this runner is done (reloading takes milliseconds).
                drop_tess();
                super::mupdf::release_heap();
                if remaining.fetch_sub(1, Ordering::AcqRel) == 1 {
                    if let Some(f) = finished.lock().take() {
                        let r = std::mem::take(&mut *results.lock());
                        f(ctl.is_cancelled(), r, error.lock().take());
                    }
                }
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_places_words_on_the_baseline() {
        let lines = vec![LineJson {
            b: [10.0, 20.0, 110.0, 32.0],
            bl: [10.0, 30.0, 110.0, 30.0],
            w: vec![(10.0, 20.0, 50.0, 32.0, 90.0, "Hello".into()), (60.0, 20.0, 110.0, 30.0, 90.0, "world".into())],
        }];
        let (s, n) = page_content(&lines, [0.0, 0.0]);
        assert_eq!(n, 2);
        assert!(s.contains("/PgnOCR 12.00 Tf"), "{s}");
        assert!(s.contains("1.0000 0.0000 0.0000 -1.0000 10.00 32.00 Tm"), "{s}");
        assert!(s.contains("<00480065006C006C006F0020>Tj"), "{s}");
        assert!(s.contains("<0077006F0072006C0064>Tj"), "{s}");
    }

    fn line(y: f32, words: &[(f32, f32, &str)]) -> LineJson {
        let (x0, x1) = (words[0].0, words[words.len() - 1].1);
        LineJson { b: [x0, y, x1, y + 10.0], bl: [x0, y + 8.0, x1, y + 8.0], w: words.iter().map(|&(a, b, t)| (a, y, b, y + 10.0, 90.0, t.to_string())).collect() }
    }

    /// The words in the order the content writes them.
    #[test]
    fn a_turned_page_is_turned_back() {
        // A word at the top-left of a page read turned 90 degrees clockwise (612 x 792 page).
        let lines = vec![line(40.0, &[(20.0, 60.0, "Sideways")])];
        let (s, n) = super::turned_content(&lines, &[0.0, 0.0, 612.0, 792.0], 90);
        assert_eq!(n, 1);
        assert!(s.starts_with("q\n0 -1 1 0 0 792 cm\n"), "{s}");
        assert!(s.ends_with("Q\n"));
        // Upright: as before, no matrix.
        let (u, _) = super::turned_content(&lines, &[0.0, 0.0, 612.0, 792.0], 0);
        assert!(u.starts_with("BT"));
    }

    fn texts(s: &str) -> Vec<String> {
        s.lines()
            .filter_map(|l| l.strip_prefix('<').and_then(|l| l.strip_suffix(">Tj")))
            .map(|h| {
                let u: Vec<u16> = (0..h.len() / 4).map(|i| u16::from_str_radix(&h[i * 4..i * 4 + 4], 16).unwrap()).collect();
                String::from_utf16(&u).unwrap().trim().to_string()
            })
            .collect()
    }

    #[test]
    fn a_line_read_across_a_gutter_is_split_into_its_columns() {
        // Tesseract joined each article line with the sidebar line beside it.
        let lines = vec![
            line(10.0, &[(10.0, 40.0, "Heading"), (300.0, 340.0, "Weather")]),
            line(30.0, &[(10.0, 40.0, "body1"), (45.0, 200.0, "more"), (300.0, 340.0, "Sunny")]),
            line(50.0, &[(10.0, 40.0, "body2"), (300.0, 340.0, "28C")]),
        ];
        let (s, n) = page_content(&lines, [0.0, 0.0]);
        assert_eq!(n, 7);
        assert_eq!(texts(&s), ["Heading", "body1", "more", "body2", "Weather", "Sunny", "28C"]);
    }

    #[test]
    fn full_width_text_and_narrow_margins_keep_their_rows() {
        // A full-width page with line numbers in the margin: numbers stay on their lines.
        let lines = vec![
            line(10.0, &[(10.0, 20.0, "1"), (60.0, 500.0, "first")]),
            line(30.0, &[(10.0, 20.0, "2"), (60.0, 500.0, "second")]),
            line(50.0, &[(60.0, 500.0, "third")]),
        ];
        let (s, _) = page_content(&lines, [0.0, 0.0]);
        assert_eq!(texts(&s), ["1", "first", "2", "second", "third"]);
    }
}
