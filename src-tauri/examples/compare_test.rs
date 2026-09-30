//! Compare: cargo run --release --example compare_test -- <testfiles dir> <temp dir>
//! Office samples against the PDFs Office made of them (tools/office/make-samples.py): the words should match.
//! A Markdown file against a PDF of it, a PDF against a changed copy, and a long document against itself.
use paginaut_lib::compare::{run, Options, Outcome, SideReq};
use paginaut_lib::engine::Engine;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // compare_test words <file>: each page's words as the comparison reads them.
    if args[1] == "words" {
        let engine = Engine::new(4, 64 << 20).unwrap();
        let info = engine.open(&args[2]).unwrap();
        for (p, (json, size)) in engine.page_words(info.doc_id, 1, |_| {}).unwrap().into_iter().enumerate() {
            println!("page {p} {size:?}: {json}");
        }
        return;
    }
    let (dir, tmp) = (PathBuf::from(&args[1]), PathBuf::from(&args[2]));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let engine = Engine::new(8, 256 << 20).unwrap();
    let stop = AtomicBool::new(false);
    let mut failures = 0;
    let opts = Options { match_case: true, skip_edges: true, pictures: true };
    let side = |engine: &Arc<Engine>, p: &Path| -> SideReq {
        let path = p.to_string_lossy().into_owned();
        if paginaut_lib::officetext::kind(&path).is_some() {
            SideReq { path, doc: None, fixed: false }
        } else {
            let info = engine.open(&path).unwrap();
            SideReq { path, doc: Some(info.doc_id), fixed: !info.reflowable }
        }
    };
    let mut go = |name: &str, a: &Path, b: &Path, most: usize, least: usize| -> Option<Outcome> {
        let (sa, sb) = (side(&engine, a), side(&engine, b));
        let t = Instant::now();
        let r = run(&engine, &sa, &sb, opts, 99, &stop, &|_| {});
        let el = t.elapsed();
        match r {
            Ok(o) => {
                let pics = o.changes.iter().filter(|c| c.kind == "picture").count();
                let n = o.changes.len();
                let ok = n <= most && n >= least;
                println!("{} {name}: {n} changes ({pics} pictures), words {:?}, rough {}, picture pages {:?}, {el:?}", if ok { "PASS" } else { "FAIL" }, o.words, o.rough, o.picture_pages);
                for c in o.changes.iter().take(12) {
                    println!("    {:8} {:?} {:?} → {:?} a{:?} b{:?}", c.kind, c.place, c.old.chars().take(70).collect::<String>(), c.new.chars().take(70).collect::<String>(), c.a.at, c.b.at);
                }
                if !ok {
                    failures += 1;
                }
                Some(o)
            }
            Err(e) => {
                println!("FAIL {name}: {e}");
                failures += 1;
                None
            }
        }
    };
    let office = dir.join("office");
    let refs = office.join("ref");
    for (f, pdf, most) in [("report.docx", "report.pdf", 3), ("letter.docx", "letter.pdf", 3), ("deck.pptx", "deck.pdf", 3), ("budget.xlsx", "budget.pdf", 3), ("invoice.xlsx", "invoice.pdf", 3)] {
        go(&format!("{f} vs PDF"), &office.join(f), &refs.join(pdf), most, 0);
    }
    // The same Word file printed by LibreOffice: other page breaks, the same words.
    go("report PDF (Word) vs PDF (LibreOffice)", &refs.join("report.pdf"), &office.join("lo").join("report.pdf"), 6, 0);

    // Markdown and a PDF made of it.
    let md = dir.join("markdown").join("sample.md");
    let md_pdf = tmp.join("sample.pdf");
    engine.convert_file(&md.to_string_lossy(), &md_pdf.to_string_lossy()).unwrap();
    go("Markdown vs its PDF", &md, &md_pdf, 0, 0);
    // A changed copy of the Markdown: one word changed, one sentence added, one removed.
    let text = std::fs::read_to_string(&md).unwrap();
    let changed = text.replacen("Markdown", "Markdown-ish", 1) + "\n\nA brand new closing paragraph appears here.\n";
    let md2 = tmp.join("changed.md");
    std::fs::write(&md2, changed).unwrap();
    if let Some(photo) = ["photo.png"].iter().map(|p| dir.join("markdown").join(p)).find(|p| p.is_file()) {
        let _ = std::fs::copy(&photo, tmp.join("photo.png"));
    }
    go("Markdown (changed) vs PDF", &md2, &md_pdf, 4, 2);

    // A box drawn in a margin: one picture change, where it is.
    let boxed = tmp.join("boxed.pdf");
    {
        let info = engine.open(&refs.join("report.pdf").to_string_lossy()).unwrap();
        let d = engine.doc(info.doc_id).unwrap();
        d.redact_mark(&[paginaut_lib::engine::RedactMark { page: 1, quads: vec![], rect: Some([20.0, 300.0, 60.0, 380.0]) }], "Tester").unwrap();
        d.redact_apply(true, [0.1, 0.2, 0.6], "").unwrap();
        let n = info.page_count;
        engine.save(info.doc_id, (0..n).collect(), vec![0; n], Some(boxed.to_string_lossy().into_owned())).unwrap();
    }
    if let Some(o) = go("PDF vs a copy with a box drawn", &refs.join("report.pdf"), &boxed, 1, 1) {
        let c = &o.changes[0];
        let b = c.a.boxes[0];
        let ok = c.kind == "picture" && b[0] == 1.0 && b[1] <= 22.0 && b[3] >= 58.0 && b[2] <= 302.0 && b[4] >= 378.0 && b[3] - b[1] < 70.0;
        println!("{} the box is found where it was drawn: {b:?}", if ok { "PASS" } else { "FAIL" });
    }

    // Two versions of an agreement.
    go("agreement 1704 vs 1705", &dir.join("REG AGR_A_1704.pdf"), &dir.join("REG AGR_A_1705.pdf"), 5000, 1);
    // A long document against itself: nothing, quickly.
    go("prospectus vs itself", &dir.join("nse rhp.pdf"), &dir.join("nse rhp.pdf"), 0, 0);
    // Scans: pictures only.
    go("scan vs itself", &dir.join("scan-300.pdf"), &dir.join("scan-300.pdf"), 0, 0);

    println!("{}", if failures == 0 { "ALL PASSED".to_string() } else { format!("{failures} FAILED") });
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
