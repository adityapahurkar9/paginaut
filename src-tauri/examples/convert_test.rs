//! Converting from PDF: cargo run --release --example convert_test -- <testfiles dir> <temp dir> [app data dir]
//! Uses the Office samples' reference PDFs (tools/office/make-samples.py). With the app's data folder given, its
//! LibreOffice pack is used too; Microsoft Office when installed. Checks each file is made and reads back.
use paginaut_lib::convert::{options, run, Request};
use paginaut_lib::engine::Engine;
use paginaut_lib::office::Office;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, tmp) = (PathBuf::from(&args[1]).join("office").join("ref"), PathBuf::from(&args[2]));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let data = args.get(3).map(PathBuf::from).unwrap_or_else(|| tmp.join("data"));
    let office = Office::new(&data);
    let engine = Engine::new(4, 64 << 20).unwrap();
    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failures += 1;
        }
    };
    let report = engine.open(&dir.join("report.pdf").to_string_lossy()).unwrap();
    let budget = engine.open(&dir.join("budget.pdf").to_string_lossy()).unwrap();
    let all = |n: usize| (0..n).collect::<Vec<usize>>();
    let opts = options(&office);
    println!("Word from: {:?}, PowerPoint from: {:?}", opts.docx, opts.pptx);
    let stop = AtomicBool::new(false);
    let go = |doc: &paginaut_lib::engine::DocInfo, format: &str, engine_: Option<&str>, name: &str| {
        let out = tmp.join(name).to_string_lossy().into_owned();
        let req = Request { doc: doc.doc_id, order: all(doc.page_count), shown: Vec::new(), rot: vec![0; doc.page_count], format: format.into(), out, engine: engine_.map(Into::into), dpi: Some(100.0) };
        let t = Instant::now();
        let r = run(&engine, &office, &req, &stop, &mut |_, _| {});
        (r, t.elapsed())
    };

    for (format, eng, name) in [("docx", Some("builtin"), "report-mupdf.docx"), ("txt", None, "report.txt"), ("tiff", None, "report.tiff"), ("png", None, "report.png"), ("jpg", None, "report.jpg")] {
        let (r, t) = go(&report, format, eng, name);
        let files = r.as_ref().map(|d| d.files.clone()).unwrap_or_default();
        let sizes: Vec<u64> = files.iter().map(|f| std::fs::metadata(f).map(|m| m.len()).unwrap_or(0)).collect();
        check(&format!("{format} ({})", eng.unwrap_or("best")), r.is_ok() && !files.is_empty() && sizes.iter().all(|&s| s > 100), format!("{} files {:?} in {t:?} {:?}", files.len(), sizes.iter().take(3).collect::<Vec<_>>(), r.as_ref().err()));
    }
    let text = std::fs::read_to_string(tmp.join("report.txt")).unwrap_or_default();
    check("text has the words", text.contains("Quarterly Report") && text.contains("Figures by region"), format!("{} chars", text.len()));
    // Five pages numbered after the name chosen.
    check("pictures numbered per page", (1..=5).all(|n| tmp.join(format!("report-{n}.png")).is_file()), String::new());

    let (r, t) = go(&budget, "xlsx", None, "budget.xlsx");
    check("tables to Excel", r.as_ref().is_ok_and(|d| d.note.contains("table")), format!("{:?} in {t:?}", r.as_ref().map(|d| &d.note)));
    let (r, _) = go(&report, "xlsx", None, "report.xlsx");
    check("tables to Excel (report)", r.is_ok(), format!("{:?}", r.as_ref().map(|d| &d.note)));

    if opts.docx.contains(&"msoffice".to_string()) {
        let (r, t) = go(&report, "docx", Some("msoffice"), "report-word.docx");
        check("docx (Microsoft Word)", r.as_ref().is_ok_and(|d| d.by == "Microsoft Word") && tmp.join("report-word.docx").is_file(), format!("in {t:?} {:?}", r.as_ref().err()));
    }
    for e in &opts.pptx {
        let name = format!("report-{e}.pptx");
        let (r, t) = go(&report, "pptx", Some(e), &name);
        check(&format!("pptx ({e})"), r.is_ok() && tmp.join(&name).is_file(), format!("in {t:?} {:?}", r.as_ref().map(|d| &d.note)));
    }
    // Markdown out: headings, lists, a table, the picture in a folder beside it.
    let (r, t) = go(&report, "md", None, "report.md");
    let md = std::fs::read_to_string(tmp.join("report.md")).unwrap_or_default();
    check(
        "Markdown from PDF",
        r.is_ok() && md.contains("# Quarterly Report") && md.contains("| Region |") && md.contains("- New office opened in Pune") && (tmp.join("report_files").join("picture-1.jpg").is_file() || tmp.join("report_files").join("picture-1.png").is_file())
            && md.matches("Quarterly Report - Q3 2026").count() == 0,
        format!("in {t:?}, {} chars {:?}", md.len(), r.as_ref().map(|d| &d.note)),
    );
    println!("---- report.md (start) ----\n{}\n----", md.chars().take(900).collect::<String>());
    // Markdown in: a file, and text being edited (pictures from the file's folder).
    let sample = PathBuf::from(&args[1]).join("markdown").join("sample.md");
    if sample.is_file() {
        let pdf = tmp.join("sample.pdf").to_string_lossy().into_owned();
        let r = engine.convert_file(&sample.to_string_lossy(), &pdf);
        let back = r.as_ref().ok().and_then(|_| engine.open(&pdf).ok());
        check("Markdown to PDF (A4)", back.as_ref().is_some_and(|i| (i.first_size[0] - 595.0).abs() < 1.0), format!("{:?} {:?}", r.err(), back.map(|i| i.page_count)));
        let text = std::fs::read_to_string(&sample).unwrap() + "\n\n## Added while editing\n";
        match engine.open_markdown(&sample.to_string_lossy(), &text) {
            Ok(info) => {
                let d = engine.doc(info.doc_id).unwrap();
                let all: String = (0..info.page_count).map(|i| d.area_text(i, [0.0, 0.0, 595.0, 842.0]).unwrap_or_default()).collect();
                check("Markdown shown from edited text", all.contains("Added while editing") && info.path == sample.to_string_lossy(), format!("{} pages", info.page_count));
            }
            Err(e) => check("Markdown shown from edited text", false, e),
        }
    }

    // To PDF: a web page, laid out on A4.
    let html = tmp.join("page.html");
    std::fs::write(&html, "<html><body><h1>Web page</h1><p>Paginaut makes a PDF of this page.</p><ul><li>One</li><li>Two</li></ul></body></html>").unwrap();
    let pdf = tmp.join("page.pdf").to_string_lossy().into_owned();
    let r = engine.convert_file(&html.to_string_lossy(), &pdf);
    let back = r.as_ref().ok().and_then(|_| engine.open(&pdf).ok());
    check("web page to PDF (A4)", back.as_ref().is_some_and(|i| i.is_pdf && (i.first_size[0] - 595.0).abs() < 1.0), format!("{:?} {:?}", r.err(), back.map(|i| i.first_size)));
    let r = engine.convert_file(&dir.join("report.pdf").to_string_lossy(), &pdf);
    check("a PDF isn't converted again", r.is_err(), format!("{:?}", r.err()));

    if failures == 0 {
        println!("ALL PASSED");
    } else {
        println!("{failures} FAILED");
        std::process::exit(1);
    }
}
