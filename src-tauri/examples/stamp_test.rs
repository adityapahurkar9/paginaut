//! Page numbers, headers and watermarks: cargo run --release --example stamp_test -- <testfiles dir> <temp dir>
//! Works on copies; never touches the originals.
use paginaut_lib::engine::mupdf::{render_size, StampPart};
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
    // Joined letters (क्र, ष्ट्र) and a vowel sign drawn before its letter (कि) copy as written (ActualText).
    let marathi = "पान क्रमांक २ — महाराष्ट्र किंमत";
    d.stamp_pages(&[StampPage { page: 1, parts: vec![text(marathi, 11.0, w - 72.0, 0, [1.0, 0.0, 0.0, 1.0, 36.0, 20.0])] }], "Header", false, false).unwrap();
    let t1 = d.text(1).unwrap().lock().all();
    check("Marathi header", t1.contains(marathi), t1.lines().find(|l| l.contains('प')).unwrap_or("").to_string());
    let found = serde_json::from_str::<Vec<serde_json::Value>>(&d.text(1).unwrap().lock().find(&d.bounds(1).unwrap(), "क्रमांक", 0, false).unwrap()).unwrap().len();
    check("Marathi search", found == 1, format!("{found} found"));

    // 4. Watermarks: text at 45° behind, a picture in front, faint.
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (c, s) = (std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    let wm = StampPart { opacity: 0.2, color: [1.0, 0.0, 0.0], ..text("CONFIDENTIAL", 60.0, 800.0, 1, [c, -s, s, c, cx - 400.0 * c, cy + 400.0 * s]) };
    let pic = StampPart { text: None, image: Some(format!("{dir}/photo.png")), width: 200.0, height: 300.0, opacity: 0.3, ..text("", 1.0, 200.0, 0, [1.0, 0.0, 0.0, 1.0, cx - 100.0, cy - 150.0]) };
    d.stamp_pages(&[StampPage { page: 2, parts: vec![wm.clone()] }, StampPage { page: 3, parts: vec![pic] }], "Watermark", true, true).unwrap();
    let t2 = d.text(2).unwrap().lock().all();
    check("text watermark", t2.contains("CONFIDENTIAL"), String::new());
    // Red at 20% on white is pale pink; none of it may come out solid red (the opacity once got lost in the file).
    let (solid, faint) = {
        let list = d.list(2, 1).unwrap();
        let b = d.bounds(2).unwrap();
        let (rw, rh) = render_size(&b, 0.5, 0);
        let mut buf = vec![0u8; (rw * rh * 4) as usize];
        list.render(&b, 0.5, 0, [0, 0, rw as i32, rh as i32], &mut buf, None).unwrap();
        let px = buf.chunks(4);
        let solid = px.clone().filter(|p| p[0] > 200 && p[1] < 100 && p[2] < 100).count();
        let faint = px.filter(|p| p[0] > 235 && (180..235).contains(&p[1]) && (180..235).contains(&p[2])).count();
        (solid, faint)
    };
    check("text watermark is faint", solid == 0 && faint > 500, format!("{solid} solid red, {faint} pale red pixels"));

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

    // 7. The last watermark gone: its "Watermark" layer leaves the file too (other readers went on listing it).
    let has_layer = |path: &str| std::fs::read(path).unwrap().windows(13).any(|w| w == b"PaginautLayer");
    let with = format!("{tmp}/with-watermark.pdf");
    let without = format!("{tmp}/without-watermark.pdf");
    engine.save_parts(saved.doc_id, vec![paginaut_lib::engine::Part { pages: (0..n).collect(), path: with.clone() }], vec![0; n]).unwrap();
    d.remove_stamps("Watermark", None).unwrap();
    engine.save_parts(saved.doc_id, vec![paginaut_lib::engine::Part { pages: (0..n).collect(), path: without.clone() }], vec![0; n]).unwrap();
    check("the Watermark layer goes with the last watermark", has_layer(&with) && !has_layer(&without), format!("with {}, without {}", has_layer(&with), has_layer(&without)));
    engine.close(saved.doc_id);

    println!("{}", if failures == 0 { "ALL PASSED".to_string() } else { format!("{failures} FAILED") });
    std::process::exit(failures);
}
