//! Page numbers, headers and watermarks: cargo run --release --example stamp_test -- <testfiles dir> <temp dir>
//! Works on copies; never touches the originals.
use paginaut_lib::engine::mupdf::StampPart;
use paginaut_lib::engine::{Engine, StampPage};
use std::time::Instant;

fn text(t: &str, size: f32, width: f32, valign: u8, matrix: [f32; 6]) -> StampPart {
    StampPart {
        text: Some(t.into()),
        image: None,
        font: Some("sans-serif".into()),
        size,
        color: [0.2, 0.2, 0.2],
        bold: false,
        align: Some("center".into()),
        width,
        height: 0.0,
        valign,
        matrix,
        opacity: 1.0,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, tmp) = (&args[1], &args[2]);
    std::fs::create_dir_all(tmp).unwrap();
    let engine = Engine::new(4, 64 << 20).unwrap();
    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failures += 1;
        }
    };
    let p = format!("{tmp}/text.pdf");
    std::fs::copy(format!("{dir}/text-1000.pdf"), &p).unwrap();
    let info = engine.open(&p).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();
    let n = info.page_count;
    let [w, h] = engine.page_sizes(info.doc_id).unwrap()[0];
    let footer = |i: usize| StampPage { page: i, parts: vec![text(&format!("Page {} of {n}", i + 1), 9.0, w - 72.0, 2, [1.0, 0.0, 0.0, 1.0, 36.0, h - 24.0])] };

    // 1. A footer on every page.
    let t = Instant::now();
    d.stamp_pages(&(0..n).map(footer).collect::<Vec<_>>(), "Header", false, true).unwrap();
    let took = t.elapsed();
    let t0 = d.text(0).unwrap().lock().all();
    let t998 = d.text(998).unwrap().lock().all();
    check("page numbers on 1000 pages", t0.contains("Page 1 of 1000") && t998.contains("Page 999 of 1000"), format!("{took:?}"));

    // 2. Replacing: the old ones go.
    d.stamp_pages(&[footer(0)], "Header", false, true).unwrap();
    let t0 = d.text(0).unwrap().lock().all();
    check("replace, not pile up", t0.matches("Page 1 of 1000").count() == 1, format!("{} times", t0.matches("Page 1 of 1000").count()));

    // 3. A Marathi header (shaped by MuPDF's layout, fonts built in).
    d.stamp_pages(&[StampPage { page: 1, parts: vec![text("पान क्रमांक २ — महाराष्ट्र", 11.0, w - 72.0, 0, [1.0, 0.0, 0.0, 1.0, 36.0, 20.0])] }], "Header", false, false).unwrap();
    let t1 = d.text(1).unwrap().lock().all();
    check("Marathi header", t1.contains("महाराष्ट्र") || t1.contains("पान"), t1.lines().find(|l| l.contains('प')).unwrap_or("").to_string());

    // 4. Watermarks: text at 45° behind, a picture in front, faint.
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (c, s) = (std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    let wm = StampPart { opacity: 0.2, ..text("CONFIDENTIAL", 60.0, 800.0, 1, [c, -s, s, c, cx - 400.0 * c, cy + 400.0 * s]) };
    let pic = StampPart { text: None, image: Some(format!("{dir}/photo.png")), width: 200.0, height: 300.0, opacity: 0.3, ..text("", 1.0, 200.0, 0, [1.0, 0.0, 0.0, 1.0, cx - 100.0, cy - 150.0]) };
    d.stamp_pages(&[StampPage { page: 2, parts: vec![wm.clone()] }, StampPage { page: 3, parts: vec![pic] }], "Watermark", true, true).unwrap();
    let t2 = d.text(2).unwrap().lock().all();
    check("text watermark", t2.contains("CONFIDENTIAL"), String::new());

    // 5. Removing, and undo.
    let removed = d.remove_stamps("Watermark", None).unwrap();
    check("remove watermark", removed == 2 && !d.text(2).unwrap().lock().all().contains("CONFIDENTIAL"), format!("{removed} pages"));
    d.undo(false).unwrap();
    check("undo remove", d.text(2).unwrap().lock().all().contains("CONFIDENTIAL"), String::new());

    // 6. Saved and reopened: still there, and still removable.
    let saved = engine.save(info.doc_id, (0..n).collect(), vec![0; n], None).unwrap();
    let d = engine.doc(saved.doc_id).unwrap();
    d.forms_init().unwrap();
    check("after reopening", d.text(500).unwrap().lock().all().contains("Page 501 of 1000"), String::new());
    let t = Instant::now();
    let removed = d.remove_stamps("Header", None).unwrap();
    check("remove headers after reopening", removed == n && !d.text(500).unwrap().lock().all().contains("Page 501 of 1000"), format!("{removed} pages in {:?}", t.elapsed()));
    check("watermark kept", d.text(2).unwrap().lock().all().contains("CONFIDENTIAL"), String::new());
    engine.close(saved.doc_id);

    println!("{}", if failures == 0 { "ALL PASSED".to_string() } else { format!("{failures} FAILED") });
    std::process::exit(failures);
}
