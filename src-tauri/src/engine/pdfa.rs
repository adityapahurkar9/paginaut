//! PDF/A (ISO 19005), parts 2 and 3, level b: checking a document and saving a copy that conforms. The checks and
//! fixes run natively (paginaut.c, fo_pdfa_*); this side words what they found, picks the fonts to embed and saves.

use super::mupdf::Document;
use super::{DocId, Engine, Part};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What fo_pdfa_check found.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Found {
    claim: Option<Claim>,
    part: i32,
    encrypted: bool,
    version: bool,
    xmp: bool,
    mismatch: String,
    intent: i32,
    bad_intent: bool,
    xfa: bool,
    need_appearances: bool,
    scripts: usize,
    actions: usize,
    action_names: String,
    lzw: usize,
    external: usize,
    postscript: usize,
    reference: usize,
    images: usize,
    transfer: usize,
    attachments: usize,
    layers: usize,
    object_metadata: usize,
    gray: Vec<usize>,
    rgb: Vec<usize>,
    cmyk: Vec<usize>,
    font_pages: Vec<usize>,
    annot_types: Vec<usize>,
    annot_flags: Vec<usize>,
    annot_appearances: Vec<usize>,
    fonts: Vec<FontNeed>,
}

/// A font used and not in the file.
#[derive(Deserialize, Clone)]
pub struct FontNeed {
    pub num: i32,
    pub name: String,
    /// The base 14 font it is (or MuPDF knows it as: "Arial" is Helvetica), or "".
    pub base14: String,
    pub simple: bool,
    pub bold: bool,
    pub italic: bool,
    pub serif: bool,
    pub symbolic: bool,
}

/// What the file says it is ("pdfaid" in its XMP).
#[derive(Serialize, Deserialize, Clone)]
pub struct Claim {
    pub part: i32,
    pub conformance: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub id: &'static str,
    pub text: String,
    /// Page numbers (from 1) where it was found; empty when it's the document's.
    pub pages: Vec<usize>,
    /// Save as PDF/A puts it right.
    pub fix: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub claim: Option<Claim>,
    pub part: i32,
    pub issues: Vec<Issue>,
    /// Signatures in the document (a PDF/A copy is a new file: they won't be valid in it).
    pub signed: usize,
}

/// What Save as PDF/A did, and what the copy still breaks.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub path: String,
    pub done: Vec<String>,
    /// Fonts that weren't on this computer, and what was put in instead.
    pub substitutes: Vec<String>,
    pub left: Report,
}

/// What fo_pdfa_fix did.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Done {
    fonts: usize,
    fonts_failed: usize,
    scripts: usize,
    actions: usize,
    annots_removed: usize,
    annots_printed: usize,
    appearances: usize,
    attachments: usize,
    lzw: usize,
    external: usize,
    postscript: usize,
    reference: usize,
    images: usize,
    transfer: usize,
    layers: usize,
    intent: bool,
    defaults: usize,
    xfa: bool,
    encrypted: bool,
    object_metadata: usize,
    glyph_lists: usize,
    /// Unreadable pages a damaged file lost when its page tree was rebuilt (set here, not by the fixer).
    #[serde(skip)]
    lost_pages: usize,
}

/// Why a font a PDF names couldn't be found to put in it.
#[derive(Clone, Copy, PartialEq)]
pub enum FontMissing {
    NotInstalled,
    /// Installed, but its licence doesn't allow putting it in documents.
    Licence,
}

/// A system font file for a font a PDF names (by name, bold, italic): its path, family, and whether it's that font
/// (not a stand-in). Desktop: fonts::find.
pub type FindFont<'a> = &'a dyn Fn(&str, bool, bool) -> Result<(PathBuf, String, bool), FontMissing>;

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn pages_of(lists: &[&Vec<usize>]) -> Vec<usize> {
    let mut all: Vec<usize> = lists.iter().flat_map(|l| l.iter().map(|p| p + 1)).collect();
    all.sort_unstable();
    all.dedup();
    all
}

fn report(f: Found) -> Report {
    let mut issues = Vec::new();
    let mut add = |id: &'static str, text: String, pages: Vec<usize>, fix: bool| issues.push(Issue { id, text, pages, fix });
    match &f.claim {
        None => add("claim", "Not marked as a PDF/A file".into(), vec![], true),
        Some(c) if c.part != f.part => add("claim", format!("Marked as PDF/A-{}{}, not PDF/A-{}", c.part, c.conformance.to_lowercase(), f.part), vec![], true),
        _ => {}
    }
    if f.encrypted {
        add("encrypted", "Protected with a password or permissions: a PDF/A file can't be".into(), vec![], true);
    }
    if f.version {
        add("version", format!("Saved as PDF 2.0: PDF/A-{} is based on PDF 1.7", f.part), vec![], true);
    }
    if !f.xmp {
        add("metadata", "No XMP metadata (where a PDF/A file says what it is)".into(), vec![], true);
    } else if !f.mismatch.is_empty() {
        add("metadata", format!("The document's details and its XMP metadata differ: {}", f.mismatch), vec![], true);
    }
    let (simple, other): (Vec<&FontNeed>, Vec<&FontNeed>) = f.fonts.iter().partition(|n| n.simple && n.num > 0);
    if !simple.is_empty() {
        let names: Vec<&str> = simple.iter().map(|n| n.name.as_str()).collect();
        add("fonts", format!("Fonts not included in the file: {}", names.join(", ")), pages_of(&[&f.font_pages]), true);
    }
    if !other.is_empty() {
        let names: Vec<&str> = other.iter().map(|n| n.name.as_str()).collect();
        add(
            "fonts-cid",
            format!("Fonts not included that can't be added here (large character sets, as for Asian scripts): {}", names.join(", ")),
            pages_of(&[&f.font_pages]),
            false,
        );
    }
    if f.bad_intent {
        add("intent", "More than one colour profile claims to be the document's, or one is missing".into(), vec![], true);
    }
    let device = pages_of(&[&f.gray, &f.rgb, &f.cmyk]);
    if !device.is_empty() {
        let kinds: Vec<&str> = [(!f.gray.is_empty(), "grey"), (!f.rgb.is_empty(), "RGB"), (!f.cmyk.is_empty(), "CMYK")].iter().filter(|k| k.0).map(|k| k.1).collect();
        let why = if f.intent == 0 { "the document has no colour profile" } else { "the document's colour profile doesn't cover them" };
        add("colours", format!("Colours for a particular kind of device ({}) where {why}", kinds.join(", ")), device, true);
    }
    if f.scripts > 0 {
        add("scripts", format!("JavaScript ({})", plural(f.scripts, "place", "places")), vec![], true);
    }
    if f.actions > 0 {
        add("actions", format!("Actions a PDF/A file can't have: {}", f.action_names), vec![], true);
    }
    if !f.annot_types.is_empty() {
        add("annot-types", "Sound, video or 3D annotations".into(), pages_of(&[&f.annot_types]), true);
    }
    if !f.annot_flags.is_empty() {
        add("annot-flags", "Comments or form fields that don't print, or are hidden".into(), pages_of(&[&f.annot_flags]), true);
    }
    if !f.annot_appearances.is_empty() {
        add("annot-appearances", "Comments or form fields without a drawn look, or with extra looks for pointing and clicking".into(), pages_of(&[&f.annot_appearances]), true);
    }
    if f.need_appearances {
        add("need-appearances", "Form fields that ask the reader to draw them".into(), vec![], true);
    }
    if f.xfa {
        add("xfa", "XFA form data (dynamic forms): the form's ordinary fields stay".into(), vec![], true);
    }
    if f.attachments > 0 {
        let text = if f.part == 2 {
            format!("{} that aren't PDF/A files themselves (PDF/A-3 allows any file)", plural(f.attachments, "attached file", "attached files"))
        } else {
            format!("{} not described as PDF/A-3 asks (their relationship to the document, type or date)", plural(f.attachments, "attached file", "attached files"))
        };
        add("attachments", text, vec![], true);
    }
    if f.object_metadata > 0 {
        add("object-metadata", format!("XMP metadata on pages or pictures ({}), which PDF/A holds to the same rules as the document's", f.object_metadata), vec![], true);
    }
    if f.layers > 0 {
        add("layers", "Layer settings without a name, or that switch layers by themselves".into(), vec![], true);
    }
    if f.lzw > 0 {
        add("lzw", format!("LZW-compressed data ({})", plural(f.lzw, "item", "items")), vec![], true);
    }
    if f.external > 0 {
        add("external", format!("Content kept in other files ({})", plural(f.external, "item", "items")), vec![], true);
    }
    if f.postscript > 0 {
        add("postscript", format!("PostScript code ({})", plural(f.postscript, "item", "items")), vec![], true);
    }
    if f.reference > 0 {
        add("reference", format!("Pictures or pages taken from other files when shown ({})", f.reference), vec![], true);
    }
    if f.images > 0 {
        add("images", format!("Pictures with alternate versions or smoothing switched on ({})", f.images), vec![], true);
    }
    if f.transfer > 0 {
        add("transfer", format!("Transfer functions (colour adjustments for printing) ({})", f.transfer), vec![], true);
    }
    Report { claim: f.claim, part: f.part, issues, signed: 0 }
}

fn found(doc: &Document, part: i32) -> Result<Found, String> {
    serde_json::from_str(&doc.pdfa_check(part)?).map_err(|e| e.to_string())
}

fn check_part(part: i32) -> Result<(), String> {
    if part == 2 || part == 3 { Ok(()) } else { Err("PDF/A-2 and PDF/A-3 are the parts Paginaut makes and checks.".into()) }
}

/// Base 14 stand-ins for fonts not on this computer.
fn stand_in(n: &FontNeed) -> &'static str {
    match (n.serif, n.bold, n.italic) {
        (true, false, false) => "Times-Roman",
        (true, true, false) => "Times-Bold",
        (true, false, true) => "Times-Italic",
        (true, true, true) => "Times-BoldItalic",
        (false, false, false) => "Helvetica",
        (false, true, false) => "Helvetica-Bold",
        (false, false, true) => "Helvetica-Oblique",
        (false, true, true) => "Helvetica-BoldOblique",
    }
}

const BASE14: [&str; 14] = [
    "Courier", "Courier-Bold", "Courier-Oblique", "Courier-BoldOblique", "Helvetica", "Helvetica-Bold", "Helvetica-Oblique", "Helvetica-BoldOblique",
    "Times-Roman", "Times-Bold", "Times-Italic", "Times-BoldItalic", "Symbol", "ZapfDingbats",
];

/// The fixer's font lines ("num\tpath\tbase14"), and what was put in instead of fonts that couldn't be, with why
/// (said to the user).
fn choose_fonts(needs: &[FontNeed], find: FindFont) -> (String, Vec<String>) {
    let mut spec = String::new();
    let mut subs = Vec::new();
    for n in needs.iter().filter(|n| n.simple && n.num > 0) {
        let is_base14 = BASE14.contains(&n.name.as_str());
        // The font itself where it's installed; a base 14 font from MuPDF's copies (same widths); otherwise a stand-in.
        let found = if is_base14 { Err(FontMissing::NotInstalled) } else { find(&n.name, n.bold, n.italic) };
        let why = |missing: FontMissing| match missing {
            FontMissing::Licence => "its licence doesn't allow putting it in files",
            FontMissing::NotInstalled => "not on this computer",
        };
        match found {
            Ok((path, family, exact)) => {
                spec.push_str(&format!("{}\t{}\t\n", n.num, path.to_string_lossy()));
                if !exact {
                    subs.push(format!("{}: not on this computer, {family} used instead", n.name));
                }
            }
            Err(_) if is_base14 => spec.push_str(&format!("{}\t\t{}\n", n.num, n.base14)),
            Err(missing) if !n.base14.is_empty() => {
                spec.push_str(&format!("{}\t\t{}\n", n.num, n.base14));
                subs.push(format!("{}: {}, {} used instead (same letter widths)", n.name, why(missing), n.base14));
            }
            Err(missing) if !n.symbolic => {
                let s = stand_in(n);
                spec.push_str(&format!("{}\t\t{s}\n", n.num));
                subs.push(format!("{}: {}, {s} used instead", n.name, why(missing)));
            }
            Err(missing) => subs.push(format!("{}: {}, and no other font can draw its symbols, so it stays out", n.name, why(missing))),
        }
    }
    (spec, subs)
}

/// The time now as a PDF date and as XMP writes it, tab between ("D:20261026093000Z\t2026-10-26T09:30:00Z"), in UTC.
fn now_dates() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let (hh, mm, ss) = (rem / 3600, rem % 3600 / 60, rem % 60);
    format!("D:{y:04}{m:02}{d:02}{hh:02}{mm:02}{ss:02}Z\t{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

fn done_text(d: &Done, part: i32) -> Vec<String> {
    let mut out = vec![format!("Marked as PDF/A-{part}b, with XMP metadata from the document's details")];
    let mut say = |n: usize, text: String| {
        if n > 0 {
            out.push(text);
        }
    };
    say(d.fonts, format!("{} put in the file", plural(d.fonts, "font", "fonts")));
    say(d.intent as usize, "An sRGB colour profile added for the document".into());
    say(d.defaults, "Colour profiles added where the pages use device colours".into());
    say(d.encrypted as usize, "Protection removed (a PDF/A file can't have it)".into());
    say(d.scripts, format!("JavaScript removed ({})", plural(d.scripts, "place", "places")));
    say(d.actions, format!("{} removed", plural(d.actions, "action PDF/A doesn't allow", "actions PDF/A doesn't allow")));
    say(d.annots_removed, format!("{} removed (sound, video, 3D, or never shown)", plural(d.annots_removed, "annotation", "annotations")));
    say(d.annots_printed, format!("{} set to print", plural(d.annots_printed, "comment or field", "comments or fields")));
    say(d.appearances, format!("{} drawn", plural(d.appearances, "comment or field", "comments or fields")));
    say(d.attachments, if part == 2 { format!("{} removed (not PDF/A files)", plural(d.attachments, "attached file", "attached files")) } else { format!("{} described as PDF/A-3 asks", plural(d.attachments, "attached file", "attached files")) });
    say(d.xfa as usize, "XFA form data removed".into());
    say(d.lzw, format!("LZW data recompressed ({})", d.lzw));
    say(d.external + d.reference, "Links to content in other files removed".into());
    say(d.postscript, "PostScript code removed".into());
    say(d.images, "Picture alternates and smoothing removed".into());
    say(d.transfer, "Transfer functions removed".into());
    say(d.layers, "Layer settings named".into());
    say(d.object_metadata, "XMP metadata on pages or pictures removed".into());
    say(d.glyph_lists, "Glyph lists of font subsets removed (optional, and often not exact)".into());
    say(d.lost_pages, format!("{} left out: damaged beyond reading", plural(d.lost_pages, "page", "pages")));
    say(d.fonts_failed, format!("{} couldn't be put in", plural(d.fonts_failed, "font", "fonts")));
    out
}

impl Engine {
    /// Checks an open document against PDF/A-`part` (2 or 3) level b.
    pub fn pdfa_check(&self, id: DocId, part: i32) -> Result<Report, String> {
        check_part(part)?;
        let d = self.doc(id)?;
        if !d.inner.lock().doc.is_pdf() {
            return Err("Only PDFs can be checked for PDF/A.".into());
        }
        // A private copy when the file is as saved: a long check doesn't hold up the pages being drawn.
        let changed = d.inner.lock().doc.has_changes();
        let f = match if changed { None } else { d.private_copy()? } {
            Some(copy) => found(&copy, part)?,
            None => found(&d.inner.lock().doc, part)?,
        };
        let mut r = report(f);
        let sigs: Vec<serde_json::Value> = serde_json::from_str(&self.sigs_json(id)?).map_err(|e| e.to_string())?;
        r.signed = sigs.iter().filter(|s| s["signed"] == true).count();
        Ok(r)
    }

    /// Checks a PDF on disk (a copy just saved, say).
    pub fn pdfa_check_file(&self, path: &str, part: i32) -> Result<Report, String> {
        check_part(part)?;
        let doc = Document::open(path)?;
        if doc.needs_password() {
            return Err("It needs a password to open.".into());
        }
        Ok(report(found(&doc, part)?))
    }

    /// Saves the pages in `order` (turned by `rot`) as a new PDF/A-`part` level b file at `out`, fixing what can be
    /// fixed, then checks the copy.
    pub fn save_pdfa(&self, id: DocId, order: Vec<usize>, rot: Vec<i32>, out: &str, part: i32, find: FindFont) -> Result<Saved, String> {
        check_part(part)?;
        let d = self.doc(id)?;
        if super::same_file(out, &d.path()) {
            return Err("Choose another name: the PDF/A copy is a new file, and this one stays as it is.".into());
        }
        if !self.security(id)?.owner {
            return Err("Enter the permissions password first (Properties, Protection): a PDF/A file can't keep this file's protection, and only its owner can take it off.".into());
        }
        let tmp = super::temp_path(id, "pdfa");
        let written = format!("{out}.paginaut-tmp");
        let res = (|| -> Result<Saved, String> {
            let unchanged = order.len() == d.page_count() && order.iter().enumerate().all(|(i, &p)| i == p) && rot.iter().all(|&r| r == 0);
            let pdf = d.inner.lock().doc.is_pdf();
            if unchanged && pdf {
                // The pages as they are: the document itself, not rebuilt through page edits (damaged page trees survive).
                let inner = d.inner.lock();
                if inner.doc.has_changes() {
                    inner.doc.save_copy(&tmp)?;
                } else {
                    std::fs::copy(d.path(), &tmp).map_err(|e| e.to_string())?;
                }
            } else {
                self.save_parts(id, vec![Part { pages: order, path: tmp.clone() }], rot)?;
            }
            let doc = Document::open(&tmp)?;
            if doc.needs_password() {
                if let Some(pw) = d.password.lock().as_deref() {
                    doc.authenticate(pw);
                }
            }
            // A damaged file (repaired when opened) gets a sound page tree first, as Repair does.
            let lost = doc.fix_page_tree()?;
            // The fonts from the objects alone: reading every page's content here too would double the work.
            #[derive(Deserialize)]
            struct Fonts {
                fonts: Vec<FontNeed>,
            }
            let needs: Fonts = serde_json::from_str(&doc.pdfa_fonts()?).map_err(|e| e.to_string())?;
            let (spec, substitutes) = choose_fonts(&needs.fonts, find);
            let mut done: Done = serde_json::from_str(&doc.pdfa_fix(part, &spec, &now_dates())?).map_err(|e| e.to_string())?;
            done.lost_pages = lost;
            doc.save_pdf_with(&written, Some(None), false)?;
            drop(doc);
            std::fs::rename(&written, out).or_else(|_| std::fs::copy(&written, out).map(|_| ())).map_err(|e| format!("Couldn't write {out}: {e}"))?;
            let left = self.pdfa_check_file(out, part)?;
            Ok(Saved { path: out.to_string(), done: done_text(&done, part), substitutes, left })
        })();
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(&written);
        res
    }
}
