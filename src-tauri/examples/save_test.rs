//! Save engine test: cargo run --release --example save_test -- <testfiles dir> <temp dir>
//! Works on copies; never touches the originals.
use paginaut_lib::engine::Engine;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (src_dir, tmp) = (&args[1], &args[2]);
    std::fs::create_dir_all(tmp).unwrap();
    let engine = Engine::new(4, 64 << 20).unwrap();
    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failures += 1;
        }
    };

    // 1. Incremental save in place: rotate page 1, delete page 3, move page 5 to the front.
    let p = format!("{tmp}/text-copy.pdf");
    std::fs::copy(format!("{src_dir}/text-1000.pdf"), &p).unwrap();
    let before = std::fs::metadata(&p).unwrap().len();
    let info = engine.open(&p).unwrap();
    let n = info.page_count;
    let mut rot = vec![0; n];
    rot[0] = 90;
    let mut order: Vec<usize> = (0..n).filter(|&i| i != 2).collect();
    let five = order.remove(order.iter().position(|&x| x == 4).unwrap());
    order.insert(0, five);
    let t = Instant::now();
    let saved = engine.save(info.doc_id, order.clone(), rot.clone(), None).unwrap();
    let after = std::fs::metadata(&p).unwrap().len();
    check("incremental save", saved.page_count == n - 1, format!("{} → {} pages in {:?}", n, saved.page_count, t.elapsed()));
    check("only appended", after > before && after - before < 200_000, format!("file grew {} bytes", after - before));
    let sizes = engine.page_sizes(saved.doc_id).unwrap();
    // Page 5 is now first (unrotated), original page 1 is second and rotated (wider than tall).
    check("rotation persisted", sizes[1][0] > sizes[1][1], format!("page 2 size {:?}", sizes[1]));
    engine.close(saved.doc_id);
    let reopened = engine.open(&p).unwrap();
    check("reopen after save", reopened.page_count == n - 1, format!("{} pages", reopened.page_count));
    engine.close(reopened.doc_id);

    // 2. A repaired (broken) file can't be appended to: full rewrite in place.
    let b = format!("{tmp}/broken-copy.pdf");
    std::fs::copy(format!("{src_dir}/broken.pdf"), &b).unwrap();
    let info = engine.open(&b).unwrap();
    let n = info.page_count;
    let mut rot = vec![0; n];
    rot[1] = 180;
    let t = Instant::now();
    let res = engine.save(info.doc_id, (0..n).collect(), rot, None);
    check("repaired file rewrite", res.is_ok(), format!("{:?} in {:?}", res.as_ref().map(|i| i.page_count), t.elapsed()));
    if let Ok(i) = res {
        engine.close(i.doc_id);
    }
    check("no temp file left", !std::path::Path::new(&format!("{b}.paginaut-tmp")).exists(), String::new());

    // 3. Save as: new file, deleted pages really gone (smaller file).
    let info = engine.open(&format!("{src_dir}/text-1000.pdf")).unwrap();
    let out = format!("{tmp}/first-100.pdf");
    let res = engine.save(info.doc_id, (0..100).collect(), vec![0; info.page_count], Some(out.clone()));
    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    check("save as (100 of 1000 pages)", res.as_ref().map(|i| i.page_count == 100).unwrap_or(false), format!("{} bytes (original 776,746)", size));
    if let Ok(i) = res {
        check("tab now points at the new file", i.path == out, i.path.clone());
        engine.close(i.doc_id);
    }

    // 4. EPUB → PDF.
    let info = engine.open(&format!("{src_dir}/book.epub")).unwrap();
    let out = format!("{tmp}/book.pdf");
    let n = info.page_count;
    let res = engine.save(info.doc_id, (0..n).collect(), vec![0; n], Some(out.clone()));
    check("epub to pdf", res.as_ref().map(|i| i.is_pdf && i.page_count == n).unwrap_or(false), format!("{:?}", res.as_ref().map(|i| (&i.format, i.page_count)).map_err(|e| e.clone())));

    // 5. Saving an EPUB in place must refuse and leave it open.
    let info = engine.open(&format!("{src_dir}/book.epub")).unwrap();
    let res = engine.save(info.doc_id, (0..info.page_count).collect(), vec![0; info.page_count], None);
    check("epub in-place refused", res.is_err(), format!("{:?}", res.err()));
    check("epub still usable", engine.page_sizes(info.doc_id).is_ok(), String::new());

    // 4b. With the journal on and a comment: a reordering save applies the order.
    let p = format!("{tmp}/order.pdf");
    std::fs::copy(format!("{src_dir}/text-1000.pdf"), &p).unwrap();
    let info = engine.open(&p).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();
    let n = info.page_count;
    let p0 = d.area_text(0, [0.0, 0.0, 600.0, 200.0]).unwrap();
    let note = paginaut_lib::engine::mupdf::AnnotEdit { rect: Some([40.0, 40.0, 60.0, 60.0]), contents: Some("kept".into()), ..Default::default() };
    d.annot_edit(0, None, Some("Text"), note, false, "Tester").unwrap();
    let mut order: Vec<usize> = (0..n).collect();
    order.swap(0, 1);
    let saved = engine.save(info.doc_id, order, vec![0; n], None).unwrap();
    let d = engine.doc(saved.doc_id).unwrap();
    check("reorder with journal", d.area_text(1, [0.0, 0.0, 600.0, 200.0]).unwrap() == p0 && d.annots_json(1).unwrap().contains("kept"), String::new());
    engine.close(saved.doc_id);
    // 4c. The same after a failed Save as.
    let p = format!("{tmp}/order2.pdf");
    std::fs::copy(format!("{src_dir}/text-1000.pdf"), &p).unwrap();
    let info = engine.open(&p).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();
    let note = paginaut_lib::engine::mupdf::AnnotEdit { rect: Some([40.0, 40.0, 60.0, 60.0]), contents: Some("kept".into()), ..Default::default() };
    d.annot_edit(0, None, Some("Text"), note, false, "Tester").unwrap();
    let mut order: Vec<usize> = (0..n).collect();
    order.swap(0, 1);
    let folder = format!("{tmp}/b-folder.pdf");
    std::fs::create_dir_all(&folder).unwrap();
    let _ = engine.save(info.doc_id, order.clone(), vec![0; n], Some(folder));
    let saved = engine.save(info.doc_id, order, vec![0; n], None).unwrap();
    let d = engine.doc(saved.doc_id).unwrap();
    check("reorder after a failed save", d.area_text(1, [0.0, 0.0, 600.0, 200.0]).unwrap() == p0 && d.annots_json(1).unwrap().contains("kept"), format!("p0 has note {}", d.annots_json(0).unwrap().contains("kept")));
    engine.close(saved.doc_id);

    // 4d. After undo and redo.
    let p = format!("{tmp}/order3.pdf");
    std::fs::copy(format!("{src_dir}/text-1000.pdf"), &p).unwrap();
    let info = engine.open(&p).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();
    let note = paginaut_lib::engine::mupdf::AnnotEdit { rect: Some([40.0, 40.0, 60.0, 60.0]), contents: Some("kept".into()), ..Default::default() };
    d.annot_edit(0, None, Some("Text"), note, false, "Tester").unwrap();
    d.undo(false).unwrap();
    d.undo(true).unwrap();
    let mut order: Vec<usize> = (0..n).collect();
    order.swap(0, 1);
    let saved = engine.save(info.doc_id, order, vec![0; n], None).unwrap();
    let d = engine.doc(saved.doc_id).unwrap();
    check("reorder after undo and redo", d.area_text(1, [0.0, 0.0, 600.0, 200.0]).unwrap() == p0 && d.annots_json(1).unwrap().contains("kept"), format!("p0 has note {}", d.annots_json(0).unwrap().contains("kept")));
    engine.close(saved.doc_id);

    // 5. A save that fails keeps every unsaved change, with its undo step: Save as into a folder (can't be written), an
    //    in-place save while another program holds the file, and a full rewrite in place likewise.
    let p = format!("{tmp}/fail.pdf");
    std::fs::copy(format!("{src_dir}/text-1000.pdf"), &p).unwrap();
    let info = engine.open(&p).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();
    let n = info.page_count;
    let note = paginaut_lib::engine::mupdf::AnnotEdit { rect: Some([40.0, 40.0, 60.0, 60.0]), contents: Some("kept".into()), ..Default::default() };
    d.annot_edit(0, None, Some("Text"), note, false, "Tester").unwrap();
    let mut order: Vec<usize> = (0..n).collect();
    order.swap(0, 1);
    let notes = |d: &paginaut_lib::engine::Doc| d.annots_json(0).unwrap().matches("\"Text\"").count();
    let page0 = d.area_text(0, [0.0, 0.0, 600.0, 200.0]).unwrap();
    let bad = format!("{tmp}/a-folder.pdf");
    std::fs::create_dir_all(&bad).unwrap();
    let r = engine.save(info.doc_id, order.clone(), vec![0; n], Some(bad.clone()));
    check("save as that fails keeps the changes", r.is_err() && d.has_changes() && notes(&d) == 1 && d.page_count() == n, format!("{:?}", r.err()));
    check("save as that fails keeps the page order", d.area_text(0, [0.0, 0.0, 600.0, 200.0]).unwrap() == page0, String::new());
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Held by "another program": nobody else may write it. Page order changed, so it's a full rewrite in place.
        let lock = std::fs::OpenOptions::new().read(true).share_mode(1).open(&p).unwrap();
        let r = engine.save(info.doc_id, order.clone(), vec![0; n], None);
        check("rewrite in place on a held file keeps the changes", r.is_err() && d.has_changes() && notes(&d) == 1, format!("{:?}", r.err()));
        let r = engine.save(info.doc_id, (0..n).collect(), vec![0; n], None);
        check("incremental save on a held file keeps the changes", r.is_err() && d.has_changes() && notes(&d) == 1, format!("{:?}", r.err()));
        drop(lock);
    }
    let first_before = d.area_text(0, [0.0, 0.0, 600.0, 200.0]).unwrap();
    d.undo(false).unwrap();
    // (The document may still count as changed: the failed saves' own steps, taken back, leave MuPDF's mark.)
    check("undo still takes the comment off", notes(&d) == 0, format!("notes {}", notes(&d)));
    d.undo(true).unwrap();
    let saved = engine.save(info.doc_id, order, vec![0; n], None).unwrap();
    let d = engine.doc(saved.doc_id).unwrap();
    let first_after = d.area_text(1, [0.0, 0.0, 600.0, 200.0]).unwrap();
    check("then a save works", notes(&d) == 0 && d.annots_json(1).unwrap().contains("kept") && first_after == first_before && saved.not_saved.is_none(), format!("notes p1 {}, p2 has kept {}, text same {}", notes(&d), d.annots_json(1).unwrap().contains("kept"), first_after == first_before));
    engine.close(saved.doc_id);

    println!("{}", if failures == 0 { "ALL PASSED".to_string() } else { format!("{failures} FAILED") });
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
