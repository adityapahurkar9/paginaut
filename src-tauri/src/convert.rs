//! The Convert menu's "From PDF" side: the document's pages as Word, Excel, PowerPoint, pictures or text.
//! Microsoft Office and LibreOffice do what they do best when they're here (Word reads PDFs into editable
//! documents; LibreOffice turns pages into editable slides); Paginaut's own writers cover the rest.

use crate::engine::{free_path, DocId, Engine, Part};
use crate::office::Office;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub doc: DocId,
    /// Pages to convert (document page indices, in this order) and every page's turn.
    pub order: Vec<usize>,
    pub rot: Vec<i32>,
    /// "docx", "xlsx", "pptx", "md", "png", "jpg", "tiff" or "txt".
    pub format: String,
    /// The file to write (for PNG/JPG with several pages: numbered files named after it).
    pub out: String,
    /// "msoffice", "libreoffice" or "builtin"; the best one here when absent.
    pub engine: Option<String>,
    /// Pictures only.
    pub dpi: Option<f32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    pub files: Vec<String>,
    /// Who made them ("Microsoft Word", "LibreOffice", "Paginaut").
    pub by: String,
    /// Anything worth knowing about the result.
    pub note: String,
}

/// What each target can be made with here, best first (for the dialog).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub docx: Vec<String>,
    pub pptx: Vec<String>,
}

pub fn options(office: &Office) -> Options {
    let e = office.engines();
    let ms = e.msoffice.as_ref();
    let lo = e.pack.is_some() || e.installed.is_some();
    let mut docx = Vec::new();
    if ms.is_some_and(|m| m.word) {
        docx.push("msoffice".into());
    }
    docx.push("builtin".into());
    let mut pptx = Vec::new();
    if lo {
        pptx.push("libreoffice".into());
    }
    if ms.is_some_and(|m| m.powerpoint) {
        pptx.push("msoffice".into());
    }
    Options { docx, pptx }
}

fn stem_of(out: &Path) -> String {
    out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "page".into())
}

/// A PDF of the pages as they are now (unsaved changes included) for Word or LibreOffice to read.
fn temp_pdf(engine: &Engine, r: &Request) -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("Paginaut").join("convert");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tmp = free_path(&dir.to_string_lossy(), "pages", "pdf");
    engine.save_parts(r.doc, vec![Part { pages: r.order.clone(), path: tmp.to_string_lossy().into_owned() }], r.rot.clone())?;
    Ok(tmp)
}

pub fn run(engine: &Engine, office: &Office, r: &Request, cancel: &AtomicBool, progress: &mut dyn FnMut(usize, usize)) -> Result<Done, String> {
    if r.order.is_empty() {
        return Err("No pages to convert.".into());
    }
    let out = PathBuf::from(&r.out);
    let dir = out.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let rot = |page: usize| r.rot.get(page).copied().unwrap_or(0);
    let stopped = || cancel.load(Ordering::Acquire);
    let opts = options(office);
    let pick = |list: &[String]| -> Option<String> {
        match &r.engine {
            Some(e) if list.contains(e) => Some(e.clone()),
            Some(_) => None,
            None => list.first().cloned(),
        }
    };
    let done = |by: &str, note: String| Ok(Done { files: vec![r.out.clone()], by: by.into(), note });
    match r.format.as_str() {
        "txt" => {
            engine.write_document(r.doc, &r.order, &r.rot, &r.out, "txt", "")?;
            let len = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
            done("Paginaut", if len < 8 { "The pages have no text (scans need Tools › OCR first).".into() } else { String::new() })
        }
        "docx" => match pick(&opts.docx).ok_or("That program isn't available.")?.as_str() {
            "msoffice" => {
                progress(0, 1);
                let tmp = temp_pdf(engine, r)?;
                let res = crate::office::com::pdf_to_word(&tmp.to_string_lossy(), &r.out, TIMEOUT);
                let _ = std::fs::remove_file(&tmp);
                res.map_err(|e| format!("Microsoft Word couldn't convert the pages: {e}"))?;
                done("Microsoft Word", String::new())
            }
            _ => {
                let d = engine.doc(r.doc)?;
                crate::docx::write(&d, &r.order, &r.out, &mut |k, n| {
                    progress(k, n);
                    !stopped()
                })?;
                done("Paginaut", "The text, tables and pictures in reading order; the page layout isn't kept. Microsoft Word keeps more of it.".into())
            }
        },
        "md" => {
            let d = engine.doc(r.doc)?;
            let n = crate::markdown::write(&d, &r.order, &r.out, &mut |k, n| {
                progress(k, n);
                !stopped()
            })?;
            let stem = stem_of(&out);
            done("Paginaut", if n > 0 { format!("{n} {} saved in \"{stem}_files\" next to it.", if n == 1 { "picture" } else { "pictures" }) } else { String::new() })
        }
        "xlsx" => {
            let d = engine.doc(r.doc)?;
            let mut found = Vec::new();
            for (k, &page) in r.order.iter().enumerate() {
                if stopped() {
                    return Err("stopped".into());
                }
                progress(k, r.order.len());
                for (i, t) in d.tables(page)?.into_iter().enumerate() {
                    let name = if i == 0 { format!("Page {}", page + 1) } else { format!("Page {} ({})", page + 1, i + 1) };
                    found.push((name, t));
                }
            }
            if found.is_empty() {
                return Err("No tables were found on these pages. (Scanned pages need Tools › OCR first.)".into());
            }
            let refs: Vec<(String, &crate::engine::table::Table)> = found.iter().map(|(n, t)| (n.clone(), t)).collect();
            std::fs::write(&out, crate::engine::table::workbook(&refs)).map_err(|e| format!("Couldn't write {}: {e}", r.out))?;
            let n = found.len();
            done("Paginaut", format!("{n} {} found, a sheet each.", if n == 1 { "table" } else { "tables" }))
        }
        "pptx" => match pick(&opts.pptx).ok_or("Making PowerPoint files needs LibreOffice (free, in Convert › Office suite choices) or Microsoft PowerPoint.")?.as_str() {
            "libreoffice" => {
                progress(0, 1);
                let tmp = temp_pdf(engine, r)?;
                let res = office.pdf_to_slides(&tmp.to_string_lossy(), &out);
                let _ = std::fs::remove_file(&tmp);
                res?;
                done("LibreOffice", String::new())
            }
            _ => {
                // PowerPoint doesn't read PDFs: a slide per page, as a picture.
                let dir = std::env::temp_dir().join("Paginaut").join("convert");
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let mut pictures = Vec::new();
                let d = engine.doc(r.doc)?;
                let res = (|| -> Result<(), String> {
                    for (k, &page) in r.order.iter().enumerate() {
                        if stopped() {
                            return Err("stopped".into());
                        }
                        progress(k, r.order.len());
                        let b = d.bounds(page)?;
                        let (w, h) = if rot(page) % 180 == 0 { (b[2] - b[0], b[3] - b[1]) } else { (b[3] - b[1], b[2] - b[0]) };
                        let file = free_path(&dir.to_string_lossy(), &format!("slide-{}", k + 1), "png");
                        std::fs::write(&file, d.page_image(page, rot(page), "png", 150.0)?).map_err(|e| e.to_string())?;
                        pictures.push((file.to_string_lossy().into_owned(), w, h));
                    }
                    crate::office::com::picture_slides(pictures.clone(), &r.out, TIMEOUT)
                })();
                for (f, _, _) in &pictures {
                    let _ = std::fs::remove_file(f);
                }
                res?;
                done("Microsoft PowerPoint", "Each page is a picture on its slide. For editable slides, get LibreOffice (Convert › Office suite choices).".into())
            }
        },
        "png" | "jpg" | "tiff" => {
            let dpi = r.dpi.unwrap_or(150.0).clamp(36.0, 600.0);
            let d = engine.doc(r.doc)?;
            if r.format == "tiff" {
                let mut tiff = crate::tiff::Tiff::create(&out)?;
                for (k, &page) in r.order.iter().enumerate() {
                    if stopped() {
                        return Err("stopped".into());
                    }
                    progress(k, r.order.len());
                    tiff.add_pnm(&d.page_image(page, rot(page), "pnm", dpi)?, dpi)?;
                }
                tiff.finish()?;
                return done("Paginaut", String::new());
            }
            let stem = stem_of(&out);
            let mut files = Vec::new();
            for (k, &page) in r.order.iter().enumerate() {
                if stopped() {
                    return Err("stopped".into());
                }
                progress(k, r.order.len());
                // One page: the name chosen. Several: numbered after it ("report-1.png"), next to it.
                let file = if r.order.len() == 1 { out.clone() } else { free_path(&dir, &format!("{stem}-{}", page + 1), &r.format) };
                std::fs::write(&file, d.page_image(page, rot(page), &r.format, dpi)?).map_err(|e| format!("Couldn't write {}: {e}", file.display()))?;
                files.push(file.to_string_lossy().into_owned());
            }
            Ok(Done { files, by: "Paginaut".into(), note: String::new() })
        }
        f => Err(format!("unknown format {f}")),
    }
}
