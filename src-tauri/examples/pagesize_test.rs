//! Crop and resize: cargo run --release --example pagesize_test -- <testfiles dir> <temp dir>
//! Works on copies; never touches the originals.
use paginaut_lib::engine::mupdf::{AnnotEdit, PageSizeOptions};
use paginaut_lib::engine::{Engine, PageSizeOp};
use serde_json::Value;

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
    let hide = PageSizeOptions { fit: 0, anchor: [0.5, 0.5], remove: false, fill: 0, color: [1.0, 1.0, 1.0] };
    let size = |id, p| engine.page_sizes(id).unwrap()[p];

    // 1. Crop, hiding the edges; undo and redo.
    let p = format!("{tmp}/text.pdf");
    std::fs::copy(format!("{dir}/text-1000.pdf"), &p).unwrap();
    let info = engine.open(&p).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();
    let [w, h] = size(info.doc_id, 0);
    let cb = d.content_box(0).unwrap();
    check("content box", cb.is_some_and(|b| b[0] > 0.0 && b[2] < w && b[3] > b[1]), format!("{cb:?} on {w}×{h}"));
    let r = d.page_size(&[PageSizeOp { page: 0, crop: [50.0, 60.0, w - 50.0, h - 60.0], size: None }], &hide).unwrap();
    check("crop (hide edges)", (r[0][0] - (w - 100.0)).abs() < 0.5 && (r[0][1] - (h - 120.0)).abs() < 0.5, format!("{:?} → {:?}", [w, h], r[0]));
    d.undo(false).unwrap();
    check("undo crop", size(info.doc_id, 0) == [w, h], format!("{:?}", size(info.doc_id, 0)));
    d.undo(true).unwrap();
    check("redo crop", (size(info.doc_id, 0)[0] - (w - 100.0)).abs() < 0.5, String::new());

    // 2. Crop removing what's outside: text outside is gone for good.
    let cut = [0.0, 0.0, w, 120.0];
    d.page_size(&[PageSizeOp { page: 1, crop: cut, size: None }], &PageSizeOptions { remove: true, ..hide }).unwrap();
    let t1 = d.text(1).unwrap().lock().all();
    let t1_before = engine.open(&format!("{dir}/text-1000.pdf")).map(|i| engine.doc(i.doc_id).unwrap().text(1).unwrap().lock().all()).unwrap();
    check("remove outside", t1.len() * 3 < t1_before.len() && !t1.is_empty(), format!("page 2 text {} → {} chars: {:?}", t1_before.len(), t1.len(), t1));

    // 2b. A redaction mark someone placed (not applied) survives "Remove outside", text under it too.
    let n = d.annot_edit(5, None, Some("Redact"), AnnotEdit { rect: Some([60.0, 60.0, 300.0, 120.0]), ..Default::default() }, false, "Tester").unwrap().0;
    let before5 = d.text(5).unwrap().lock().all().len();
    d.page_size(&[PageSizeOp { page: 5, crop: [0.0, 0.0, w, h - 100.0], size: None }], &PageSizeOptions { remove: true, ..hide }).unwrap();
    let list: Vec<Value> = serde_json::from_str(&d.annots_json(5).unwrap()).unwrap();
    let after5 = d.text(5).unwrap().lock().all().len();
    check("pending redaction left alone", list.iter().any(|a| a["n"] == n && a["t"] == "Redact") && after5 * 10 > before5 * 8, format!("{before5} → {after5} chars, {} annots", list.len()));

    // 3. Resize to Letter (fit, coloured margins): the text is all still there.
    let colour = PageSizeOptions { fit: 0, anchor: [0.5, 0.5], remove: false, fill: 1, color: [1.0, 0.9, 0.6] };
    let r = d.page_size(&[PageSizeOp { page: 2, crop: [0.0, 0.0, w, h], size: Some([612.0, 792.0]) }], &colour).unwrap();
    let t2 = d.text(2).unwrap().lock().all();
    let t2_before = engine.open(&format!("{dir}/text-1000.pdf")).map(|i| engine.doc(i.doc_id).unwrap().text(2).unwrap().lock().all()).unwrap();
    check("resize to Letter", r[0] == [612.0, 792.0] && t2.split_whitespace().collect::<Vec<_>>() == t2_before.split_whitespace().collect::<Vec<_>>(), format!("{:?}", r[0]));

    // 4. Comments move with the content: a square in the middle of an A4 page, page halved in size.
    let n = d.annot_edit(3, None, Some("Square"), AnnotEdit { rect: Some([100.0, 100.0, 200.0, 200.0]), color: Some(vec![1.0, 0.0, 0.0]), border: Some(2.0), ..Default::default() }, false, "Tester").unwrap().0;
    d.page_size(&[PageSizeOp { page: 3, crop: [0.0, 0.0, w, h], size: Some([w / 2.0, h / 2.0]) }], &hide).unwrap();
    let list: Vec<Value> = serde_json::from_str(&d.annots_json(3).unwrap()).unwrap();
    let rect: Vec<f64> = list.iter().find(|a| a["n"] == n).map(|a| a["r"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect()).unwrap_or_default();
    check("comment moved and scaled", rect.len() == 4 && (rect[0] - 50.0).abs() < 2.0 && (rect[2] - 100.0).abs() < 2.0, format!("{rect:?}"));

    // 5. Bigger page with the edges stretched out (a picture page), then save and reopen.
    let pic = engine.insert_pages(info.doc_id, vec![paginaut_lib::engine::InsertSource { path: format!("{dir}/photo.png"), pages: None, password: None }], None).unwrap();
    let [pw, ph] = pic.sizes[0];
    let ext = PageSizeOptions { fit: 2, anchor: [0.5, 0.5], remove: false, fill: 2, color: [0.0; 3] };
    let r = d.page_size(&[PageSizeOp { page: pic.first, crop: [0.0, 0.0, pw, ph], size: Some([pw + 200.0, ph + 100.0]) }], &ext).unwrap();
    check("extend edges", r[0] == [pw + 200.0, ph + 100.0], format!("{:?} → {:?}", [pw, ph], r[0]));
    // 6. A scan on a bigger page with its own paper in the new space: a picture covering the page behind it.
    let scan = engine
        .insert_pages(info.doc_id, vec![paginaut_lib::engine::InsertSource { path: format!("{dir}/scan-300.pdf"), pages: Some(vec![0]), password: None }], None)
        .unwrap();
    let [sw, sh] = scan.sizes[0];
    let paper = PageSizeOptions { fit: 0, anchor: [0.5, 0.5], remove: false, fill: 3, color: [0.0; 3] };
    let r = d.page_size(&[PageSizeOp { page: scan.first, crop: [0.0, 0.0, sw, sh], size: Some([sw * 1.4, sh * 1.25]) }], &paper).unwrap();
    let whole = d.pictures(scan.first, false).unwrap().iter().any(|p| p.r[0] < 1.0 && p.r[1] < 1.0 && (p.r[2] - r[0][0]).abs() < 1.0 && (p.r[3] - r[0][1]).abs() < 1.0);
    std::fs::write(format!("{tmp}/paper-fill.png"), d.area_data(scan.first, [0.0, 0.0, r[0][0], r[0][1]], 0, "png", 40.0).unwrap()).unwrap();
    std::fs::write(format!("{tmp}/paper-corner.png"), d.area_data(scan.first, [0.0, 0.0, 160.0, 120.0], 0, "png", 300.0).unwrap()).unwrap();
    check("paper texture", whole, format!("{:?}", d.pictures(scan.first, false).unwrap().iter().map(|p| p.r).collect::<Vec<_>>()));

    let n_all = d.page_count();
    let saved = engine.save(info.doc_id, (0..n_all).collect(), vec![0; n_all], None).unwrap();
    let sizes = engine.page_sizes(saved.doc_id).unwrap();
    check(
        "saved and reopened",
        (sizes[0][0] - (w - 100.0)).abs() < 0.5 && sizes[2] == [612.0, 792.0] && sizes[pic.first] == [pw + 200.0, ph + 100.0],
        format!("{:?} {:?} {:?}", sizes[0], sizes[2], sizes[pic.first]),
    );
    let d = engine.doc(saved.doc_id).unwrap();
    check("resized page's text after reopening", d.text(2).unwrap().lock().all().split_whitespace().count() == t2_before.split_whitespace().count(), String::new());
    engine.close(saved.doc_id);

    // 6. A form: fields move with the page and still work.
    let f = format!("{tmp}/form.pdf");
    std::fs::copy(format!("{dir}/form.pdf"), &f).unwrap();
    let info = engine.open(&f).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();
    let [w, h] = size(info.doc_id, 0);
    let before: Vec<Value> = serde_json::from_str(&d.widgets_json(0).unwrap()).unwrap();
    d.page_size(&[PageSizeOp { page: 0, crop: [0.0, 0.0, w, h], size: Some([h, w]) }], &hide).unwrap();
    let after: Vec<Value> = serde_json::from_str(&d.widgets_json(0).unwrap()).unwrap();
    let text = after.iter().find(|x| x["t"] == "text").unwrap();
    let ok = d.widget_set(0, text["n"].as_i64().unwrap() as i32, "Still works").map(|c| c.accepted).unwrap_or(false);
    check("form on a landscape page", before.len() == after.len() && ok, format!("{} fields, first now at {}", after.len(), text["r"]));
    engine.close(info.doc_id);

    println!("{}", if failures == 0 { "ALL PASSED".to_string() } else { format!("{failures} FAILED") });
    std::process::exit(failures);
}
