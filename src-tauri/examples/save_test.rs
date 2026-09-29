//! Save engine test: cargo run --release --example save_test -- <testfiles dir> <temp dir>
//! Works on copies; never touches the originals.
use folio_lib::engine::Engine;
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
    check("no temp file left", !std::path::Path::new(&format!("{b}.folio-tmp")).exists(), String::new());

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

    println!("{}", if failures == 0 { "ALL PASSED".to_string() } else { format!("{failures} FAILED") });
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
