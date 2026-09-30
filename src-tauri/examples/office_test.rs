//! Office conversion: cargo run --release --example office_test -- <testfiles dir> <temp dir> [LibreOffice folder]
//! Needs the samples from tools/office/make-samples.py (testfiles/office). Converts them with Microsoft Office
//! (when installed) and LibreOffice (the folder given, e.g. an unpacked pack), writing the PDFs to
//! testfiles/office/word and testfiles/office/lo for tools/office/compare.py; then checks protected files
//! fail cleanly, and opening an Office file through the engine.
use paginaut_lib::engine::Engine;
use paginaut_lib::office::{com, libre, Office};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, tmp) = (PathBuf::from(&args[1]).join("office"), PathBuf::from(&args[2]));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failures += 1;
        }
    };
    let samples = ["report.docx", "letter.docx", "budget.xlsx", "invoice.xlsx", "deck.pptx"];
    let locked = ["locked.docx", "locked.xlsx", "locked.pptx"];
    let timeout = Duration::from_secs(120);

    if com::installed(com::App::Word) {
        let out = dir.join("word");
        std::fs::create_dir_all(&out).unwrap();
        for f in samples {
            let app = paginaut_lib::office::kind(f).unwrap();
            let pdf = out.join(Path::new(f).with_extension("pdf").file_name().unwrap());
            let t = Instant::now();
            let r = com::convert(app, &dir.join(f).to_string_lossy(), &pdf.to_string_lossy(), timeout);
            check(&format!("{} converts {f}", app.name()), r.is_ok() && pdf.is_file(), format!("{:?} in {:?}", r, t.elapsed()));
        }
        for f in locked {
            let app = paginaut_lib::office::kind(f).unwrap();
            let t = Instant::now();
            let r = com::convert(app, &dir.join(f).to_string_lossy(), &tmp.join("locked.pdf").to_string_lossy(), timeout);
            check(&format!("{} refuses {f} without asking", app.name()), r.as_ref().is_err_and(|e| e.contains("password")), format!("{:?} in {:?}", r, t.elapsed()));
        }
    } else {
        println!("SKIP Microsoft Office isn't installed");
    }

    if let Some(lo_dir) = args.get(3) {
        let lo = libre::Lo { soffice: Path::new(lo_dir).join("program").join("soffice.exe"), version: String::new(), pack: true };
        let out = dir.join("lo");
        std::fs::create_dir_all(&out).unwrap();
        let profile = tmp.join("lo-profile");
        for f in samples {
            let pdf = out.join(Path::new(f).with_extension("pdf").file_name().unwrap());
            let t = Instant::now();
            let r = libre::convert(&lo, &profile, &dir.join(f).to_string_lossy(), &pdf, timeout);
            check(&format!("LibreOffice converts {f}"), r.is_ok() && pdf.is_file(), format!("{:?} in {:?}", r, t.elapsed()));
        }
        for f in locked {
            let t = Instant::now();
            let r = libre::convert(&lo, &profile, &dir.join(f).to_string_lossy(), &tmp.join("locked.pdf"), timeout);
            check(&format!("LibreOffice refuses {f} without asking"), r.is_err() && t.elapsed() < Duration::from_secs(60), format!("{:?} in {:?}", r, t.elapsed()));
        }
    }

    // Through the engine: an Office file opens as a PDF made from it; taking its pages works too.
    let office = Arc::new(Office::new(&tmp.join("data")));
    let engine = Engine::new(4, 64 << 20).unwrap();
    let o = office.clone();
    engine.set_converter(Arc::new(move |p: &str, e: Option<&str>| o.convert(p, e)));
    let docx = dir.join("letter.docx").to_string_lossy().into_owned();
    let t = Instant::now();
    match engine.open(&docx) {
        Ok(info) => {
            let c = info.converted.as_ref();
            let ms = office.engines().msoffice.is_some();
            check(
                "open a .docx",
                info.is_pdf && info.page_count == 2 && c.is_some_and(|c| c.from == docx && (c.engine == "msoffice") == ms),
                format!("{} pages, {:?} in {:?}", info.page_count, c.map(|c| &c.engine_name), t.elapsed()),
            );
            let t = Instant::now();
            let again = engine.open(&docx).unwrap();
            check("reopening uses the cached PDF", t.elapsed() < Duration::from_millis(500) && again.path == info.path, format!("{:?}", t.elapsed()));
            let saved = tmp.join("letter.pdf").to_string_lossy().into_owned();
            check("saving over the cached PDF is refused", engine.save(info.doc_id, (0..2).collect(), vec![0, 0], None).is_err(), String::new());
            let s = engine.save(info.doc_id, (0..2).collect(), vec![0, 0], Some(saved.clone())).unwrap();
            check("Save as makes a PDF of its own", s.converted.is_none() && s.path == saved && s.page_count == 2, s.path.clone());
        }
        Err(e) => check("open a .docx", false, e),
    }
    match engine.peek(&dir.join("deck.pptx").to_string_lossy(), None) {
        Ok(p) => check("peek at a .pptx (pages to insert)", p.pages == 7 && p.is_pdf, format!("{} pages", p.pages)),
        Err(e) => check("peek at a .pptx (pages to insert)", false, e),
    }
    let r = engine.open(&dir.join("locked.docx").to_string_lossy());
    check("a protected .docx gives a message", r.as_ref().is_err_and(|e| e.contains("password")), format!("{:?}", r.as_ref().err()));
    // Built-in preview when asked for.
    match engine.open_with(&docx, Some("builtin")) {
        Ok(info) => check("built-in text preview", info.converted.is_none() && !info.is_pdf && info.page_count >= 1, format!("{} pages", info.page_count)),
        Err(e) => check("built-in text preview", false, e),
    }

    if failures == 0 {
        println!("ALL PASSED");
    } else {
        println!("{failures} FAILED");
        std::process::exit(1);
    }
}
