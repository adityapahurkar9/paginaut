//! Inserting pages from other files: cargo run --release --example insert_test -- <testfiles dir> <temp dir>
//! Works on copies; never touches the originals.
use paginaut_lib::engine::{Engine, InsertSource, Part};
use std::time::Instant;

fn src(path: String) -> InsertSource {
    InsertSource { path, pages: None, password: None }
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
    let a4 = [595.0, 842.0];

    // 1. A form, a picture (fitted to A4) and a blank page into a 1000-page text PDF.
    let p = format!("{tmp}/text-copy.pdf");
    std::fs::copy(format!("{dir}/text-1000.pdf"), &p).unwrap();
    let before = std::fs::metadata(&p).unwrap().len();
    let info = engine.open(&p).unwrap();
    let id = info.doc_id;
    let n = info.page_count;
    let d = engine.doc(id).unwrap();
    d.forms_init().unwrap();
    let t = Instant::now();
    let ins = engine.insert_pages(id, vec![src(format!("{dir}/form.pdf"))], None).unwrap();
    check("form inserted", ins.first == n && ins.count >= 1 && d.page_count() == n + ins.count, format!("{} page(s) at {} in {:?}", ins.count, ins.first, t.elapsed()));
    let form_page = ins.first;
    let widgets = d.widgets_json(form_page).unwrap();
    check("its fields came along", widgets.matches("{\"n\":").count() > 0, format!("{} widgets", widgets.matches("{\"n\":").count()));
    let fields = d.forms_init().unwrap().fields;
    check("fields registered with the form", fields > 0, format!("{fields} fields"));

    let pic = engine.insert_pages(id, vec![src(format!("{dir}/photo.png"))], Some(a4)).unwrap();
    let [w, h] = pic.sizes[0];
    check("picture fitted to A4", pic.count == 1 && ((w - 595.0).abs() < 1.0 || (w - 842.0).abs() < 1.0), format!("{w}×{h}"));
    let blank = engine.insert_blank(id, 612.0, 792.0, 2).unwrap();
    check("blank pages", blank.count == 2 && blank.sizes[1] == [612.0, 792.0], format!("{:?}", blank.sizes));
    let total = d.page_count();

    // 2. The same form again: its field names are taken, so they get a suffix.
    let again = engine.insert_pages(id, vec![src(format!("{dir}/form.pdf"))], None).unwrap();
    let fields2 = d.forms_init().unwrap().fields;
    check("second copy's fields renamed, not merged", fields2 == fields * 2, format!("{fields} → {fields2} fields"));
    let w2 = d.widgets_json(again.first).unwrap();
    check("renamed widgets still load", w2.matches("{\"n\":").count() == widgets.matches("{\"n\":").count(), String::new());

    // 3. Undo removes the pages, redo brings them back.
    d.undo(false).unwrap();
    check("undo", d.page_count() == total, format!("{} pages", d.page_count()));
    d.undo(true).unwrap();
    check("redo", d.page_count() == total + again.count, format!("{} pages", d.page_count()));

    // 4. Save in place with every page kept: only appended.
    let all = d.page_count();
    let saved = engine.save(id, (0..all).collect(), vec![0; all], None).unwrap();
    let after = std::fs::metadata(&p).unwrap().len();
    check("incremental save", saved.page_count == all, format!("{all} pages, file grew {} bytes", after - before));
    engine.close(saved.doc_id);
    let re = engine.open(&p).unwrap();
    let d = engine.doc(re.doc_id).unwrap();
    let f = d.forms_init().unwrap().fields;
    check("reopened: pages and fields", re.page_count == all && f == fields2, format!("{} pages, {f} fields", re.page_count));
    check("reopened: picture page size", engine.page_sizes(re.doc_id).unwrap()[pic.first] == pic.sizes[0], String::new());
    engine.close(re.doc_id);

    // 4b. Inserted in the middle (page order changed): fields must survive saving.
    let m = format!("{tmp}/links-copy.pdf");
    std::fs::copy(format!("{dir}/outline-links.pdf"), &m).unwrap();
    let info = engine.open(&m).unwrap();
    engine.doc(info.doc_id).unwrap().forms_init().unwrap();
    let ins = engine.insert_pages(info.doc_id, vec![src(format!("{dir}/form.pdf"))], None).unwrap();
    let mut order: Vec<usize> = (0..info.page_count).collect();
    order.insert(1, ins.first);
    let saved = engine.save(info.doc_id, order, vec![0; ins.first + ins.count], None).unwrap();
    let f = engine.doc(saved.doc_id).unwrap().forms_init().unwrap().fields;
    let w = engine.doc(saved.doc_id).unwrap().widgets_json(1).unwrap();
    check("fields kept when inserted in the middle", f == fields, format!("{f} fields, {} widgets on page 2", w.matches("{\"n\":").count()));
    engine.close(saved.doc_id);

    // 4c. Page labels follow the pages: delete the 3rd page, insert a blank after the 1st.
    let l = format!("{tmp}/labels-copy.pdf");
    std::fs::copy(format!("{dir}/labels.pdf"), &l).unwrap();
    let info = engine.open(&l).unwrap();
    let before_labels = engine.page_labels(info.doc_id).unwrap().unwrap_or_default();
    engine.doc(info.doc_id).unwrap().forms_init().unwrap();
    let b = engine.insert_blank(info.doc_id, 612.0, 792.0, 1).unwrap();
    let mut order: Vec<usize> = (0..info.page_count).filter(|&i| i != 2).collect();
    order.insert(1, b.first);
    let saved = engine.save(info.doc_id, order, vec![0; b.first + 1], None).unwrap();
    let after_labels = engine.page_labels(saved.doc_id).unwrap().unwrap_or_default();
    check("page labels renumbered", after_labels.len() == before_labels.len(), format!("{before_labels:?} → {after_labels:?}"));
    engine.close(saved.doc_id);

    // 4d. Bookmarks whose pages are all deleted stay gone (MuPDF drops them; they must not come back).
    let b = format!("{tmp}/outline-copy.pdf");
    std::fs::copy(format!("{dir}/outline-links.pdf"), &b).unwrap();
    let info = engine.open(&b).unwrap();
    // Chapters start on pages 1, 4, 7, 10, 13, 16, 19: keep only pages 2-3.
    let saved = engine.save(info.doc_id, vec![1, 2], vec![0; info.page_count], None).unwrap();
    let outline = engine.doc(saved.doc_id).unwrap().outline_json().unwrap();
    let size = std::fs::metadata(&b).unwrap().len();
    check("bookmarks to deleted pages stay gone", outline == "[]", format!("{outline} ({size} bytes)"));
    engine.close(saved.doc_id);

    // 5. Inserted then deleted before saving: the file must not keep them.
    let q = format!("{tmp}/text-copy2.pdf");
    std::fs::copy(format!("{dir}/text-1000.pdf"), &q).unwrap();
    let info = engine.open(&q).unwrap();
    engine.doc(info.doc_id).unwrap().forms_init().unwrap();
    let ins = engine.insert_pages(info.doc_id, vec![src(format!("{dir}/Maharashtra Times.pdf"))], None).unwrap();
    let all = info.page_count + ins.count;
    let mut rot = vec![0; all];
    rot[0] = 90;
    let saved = engine.save(info.doc_id, (0..info.page_count).collect(), rot, None).unwrap();
    let size = std::fs::metadata(&q).unwrap().len();
    check("unused inserted pages left out", saved.page_count == info.page_count && size < before + 100_000, format!("{} pages, {size} bytes (original {before})", saved.page_count));
    engine.close(saved.doc_id);

    // 6. Pages in the middle, chosen pages only, internal links dropped.
    let info = engine.open(&format!("{dir}/outline-links.pdf")).unwrap();
    let ins = engine
        .insert_pages(info.doc_id, vec![InsertSource { path: format!("{dir}/outline-links.pdf"), pages: Some(vec![0]), password: None }], None)
        .unwrap();
    let orig_links = engine.doc(info.doc_id).unwrap().links_json(0).unwrap();
    let new_links = engine.doc(info.doc_id).unwrap().links_json(ins.first).unwrap();
    check("one chosen page", ins.count == 1, String::new());
    // Only page 1 was copied: its link to page 1 stays (now the copy itself), the others go.
    check("links to pages left out", new_links.matches("\"p\":").count() == 1 && new_links.contains(&format!("\"p\":{}", ins.first)) && new_links.contains("mupdf.com"), format!("original {orig_links} / copy {new_links}"));
    let bad = engine.insert_pages(info.doc_id, vec![InsertSource { path: format!("{dir}/outline-links.pdf"), pages: Some(vec![999]), password: None }], None);
    check("bad page refused", bad.is_err(), format!("{:?}", bad.err()));
    engine.close(info.doc_id);

    // 7. Password-protected source; a non-PDF target.
    let info = engine.open(&format!("{dir}/text-1000.pdf")).unwrap();
    let r = engine.insert_pages(info.doc_id, vec![src(format!("{dir}/locked.pdf"))], None);
    check("asks for a password", matches!(&r, Err(e) if e.starts_with("PASSWORD:")), format!("{:?}", r.err()));
    let r = engine.insert_pages(info.doc_id, vec![InsertSource { path: format!("{dir}/locked.pdf"), pages: None, password: Some("nope".into()) }], None);
    check("wrong password", matches!(&r, Err(e) if e.starts_with("WRONG_PASSWORD:")), format!("{:?}", r.err()));
    check("nothing added", engine.doc(info.doc_id).unwrap().page_count() == info.page_count, String::new());
    engine.close(info.doc_id);
    let info = engine.open(&format!("{dir}/book.epub")).unwrap();
    let r = engine.insert_blank(info.doc_id, 612.0, 792.0, 1);
    check("EPUB refused", r.is_err(), format!("{:?}", r.err()));
    engine.close(info.doc_id);

    // 8. A big scan into a small file, then Save as.
    let info = engine.open(&format!("{dir}/form.pdf")).unwrap();
    engine.doc(info.doc_id).unwrap().forms_init().unwrap();
    let t = Instant::now();
    let ins = engine.insert_pages(info.doc_id, vec![src(format!("{dir}/scan-300.pdf"))], None).unwrap();
    let took = t.elapsed();
    let all = info.page_count + ins.count;
    let out = format!("{tmp}/form-plus-scan.pdf");
    let t = Instant::now();
    let saved = engine.save(info.doc_id, (0..all).collect(), vec![0; all], Some(out.clone())).unwrap();
    check("scan merged", saved.page_count == all, format!("{} pages inserted in {took:?}, saved in {:?}, {} MB", ins.count, t.elapsed(), std::fs::metadata(&out).unwrap().len() >> 20));
    engine.close(saved.doc_id);

    // 9. Merge into a new file: bookmarks per file with each file's own under it.
    let out = format!("{tmp}/merged.pdf");
    let t = Instant::now();
    let n = engine
        .merge(
            vec![
                InsertSource { path: format!("{dir}/outline-links.pdf"), pages: Some((0..10).collect()), password: None },
                src(format!("{dir}/form.pdf")),
                src(format!("{dir}/photo.png")),
            ],
            Some(a4),
            true,
            &out,
        )
        .unwrap();
    let info = engine.open(&out).unwrap();
    let outline = engine.doc(info.doc_id).unwrap().outline_json().unwrap();
    let fields = engine.doc(info.doc_id).unwrap().forms_init().unwrap().fields;
    check("merged", n == 12 && info.page_count == 12, format!("{n} pages in {:?}", t.elapsed()));
    check("a bookmark per file", outline.contains("\"outline-links\"") && outline.contains("\"form\"") && outline.contains("\"photo\""), outline.chars().take(300).collect());
    // outline-links has chapters every 3 pages; pages 0-9 keep "Introduction" … "Signing" (page 12) is left out.
    check("file's own bookmarks kept for its pages", outline.contains("Marking up") && !outline.contains("Appendix"), String::new());
    check("form fields merged", fields == 12, format!("{fields} fields"));
    // Page 1's contents links to chapters on pages 1, 4, 7, 10 (kept) and 13, 16, 19 (left out).
    let links = engine.doc(info.doc_id).unwrap().links_json(0).unwrap();
    check("links to copied pages point at them", links.matches("\"p\":").count() == 4 && links.contains("\"p\":9") && links.contains("mupdf.com"), links);
    engine.close(info.doc_id);
    check("no temp file left", !std::path::Path::new(&format!("{out}.paginaut-tmp")).exists(), String::new());

    // 10. Extract: chosen pages (one rotated, one added but not saved) into a new file.
    let info = engine.open(&format!("{dir}/text-1000.pdf")).unwrap();
    engine.doc(info.doc_id).unwrap().forms_init().unwrap();
    let blank = engine.insert_blank(info.doc_id, 300.0, 300.0, 1).unwrap();
    let mut rot = vec![0; blank.first + 1];
    rot[4] = 90;
    let out = format!("{tmp}/extract.pdf");
    let sizes = engine.save_parts(info.doc_id, vec![Part { pages: vec![4, 1, blank.first], path: out.clone() }], rot.clone()).unwrap();
    let x = engine.open(&out).unwrap();
    let ps = engine.page_sizes(x.doc_id).unwrap();
    check("extract", x.page_count == 3 && ps[0][0] > ps[0][1] && ps[2] == [300.0, 300.0], format!("{} pages, {} bytes, sizes {:?}", x.page_count, sizes[0], ps));
    check("document left as it was", engine.doc(info.doc_id).unwrap().page_count() == blank.first + 1, String::new());
    engine.close(x.doc_id);
    engine.close(info.doc_id);

    // 11. Split a 241 MB scan into parts of about 40 MB.
    let info = engine.open(&format!("{dir}/scan-300.pdf")).unwrap();
    let t = Instant::now();
    let groups = engine.split_by_size(info.doc_id, (0..info.page_count).collect(), 40e6).unwrap();
    let took = t.elapsed();
    let parts: Vec<Part> = groups.iter().enumerate().map(|(i, g)| Part { pages: g.clone(), path: format!("{tmp}/scan-part-{}.pdf", i + 1) }).collect();
    let t = Instant::now();
    let sizes = engine.save_parts(info.doc_id, parts, vec![0; info.page_count]).unwrap();
    let max = *sizes.iter().max().unwrap();
    check(
        "split by size",
        groups.len() >= 6 && max <= 44_000_000 && groups.iter().map(|g| g.len()).sum::<usize>() == info.page_count,
        format!("{} parts ({:?}) grouped in {took:?}, written in {:?}; sizes {:?} MB", groups.len(), groups.iter().map(|g| g.len()).collect::<Vec<_>>(), t.elapsed(), sizes.iter().map(|s| s >> 20).collect::<Vec<_>>()),
    );
    engine.close(info.doc_id);

    // 12. Pages from another open document keep its unsaved form answers.
    let form = engine.open(&format!("{dir}/form.pdf")).unwrap();
    let fd = engine.doc(form.doc_id).unwrap();
    fd.forms_init().unwrap();
    let w: serde_json::Value = serde_json::from_str(&fd.widgets_json(0).unwrap()).unwrap();
    let name = w.as_array().unwrap().iter().find(|x| x["t"] == "text").unwrap()["n"].as_i64().unwrap() as i32;
    fd.widget_set(0, name, "Unsaved answer").unwrap();
    let info = engine.open(&format!("{dir}/text-1000.pdf")).unwrap();
    engine.doc(info.doc_id).unwrap().forms_init().unwrap();
    let ins = engine.insert_from_open(info.doc_id, form.doc_id, vec![0]).unwrap();
    let copied = engine.doc(info.doc_id).unwrap().widgets_json(ins.first).unwrap();
    check("copied from an open tab with its unsaved answers", copied.contains("Unsaved answer"), String::new());
    engine.close(info.doc_id);
    engine.close(form.doc_id);

    println!("{}", if failures == 0 { "ALL PASSED".to_string() } else { format!("{failures} FAILED") });
    std::process::exit(failures);
}
