//! Redaction: cargo run --release --example redact_test -- <testfiles dir> <temp dir>
//! Works on copies; never touches the originals.
use paginaut_lib::engine::{Engine, RedactMark, SearchOptions};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

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
    let [w, h] = engine.page_sizes(info.doc_id).unwrap()[0];
    let words = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ");
    // Hits of a search on page 1 (searches run in the background, on copies of the file unless pages changed).
    let sid = AtomicUsize::new(9000);
    let hits_on_first = |needle: &str| -> usize {
        let (tx, rx) = std::sync::mpsc::channel();
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let opts = SearchOptions { match_case: true, ..Default::default() };
        engine
            .search(info.doc_id, needle.to_string(), opts, sid.fetch_add(1, Ordering::Relaxed) as u64, move |i, h| {
                if i == 0 {
                    n2.fetch_add(h.quads.len(), Ordering::Relaxed);
                }
            }, |_, _| {}, move |_| {
                let _ = tx.send(());
            })
            .unwrap();
        rx.recv().unwrap();
        n.load(Ordering::Relaxed)
    };

    // A comment that fails half-way (its picture isn't one) leaves nothing behind: no change, no undo step.
    let bad = paginaut_lib::engine::mupdf::AnnotEdit { rect: Some([20.0, 20.0, 120.0, 80.0]), image: Some(b"not a picture".to_vec()), ..Default::default() };
    let failed = d.annot_edit(0, None, Some("Stamp"), bad, false, "Tester").is_err();
    let stamps = d.annots_json(0).unwrap().matches("\"Stamp\"").count();
    check("failed comment leaves nothing", failed && !d.has_changes() && stamps == 0, format!("failed {failed}, changed {}, stamps {stamps}", d.has_changes()));

    // A text mark on the first body line of page 1, an area mark on page 2, one on page 3 to cover.
    let body = d.paragraphs(0).unwrap().into_iter().max_by_key(|p| p.l.len()).unwrap();
    let l = body.l[0];
    let line = [l[0], l[1], l[2], l[3]];
    let before = words(&d.area_text(0, line).unwrap());
    let needle: String = words(&d.paragraphs(0).unwrap().into_iter().max_by_key(|p| p.l.len()).unwrap().t).chars().take(24).collect();
    let found_before = hits_on_first(needle.trim());
    let area = [50.0, 300.0, 400.0, 400.0];
    let area_before = words(&d.area_text(1, area).unwrap());
    let quad = vec![l[0], l[1], l[2], l[1], l[0], l[3], l[2], l[3]];
    let n = d.redact_mark(&[RedactMark { page: 0, quads: quad, rect: None }, RedactMark { page: 1, quads: vec![], rect: Some(area) }], "Tester").unwrap();
    check("marks added", n == 2 && d.redaction_pages().unwrap() == vec![(0, 1), (1, 1)], format!("{:?}", d.redaction_pages()));
    check("marks alone change nothing", words(&d.area_text(0, line).unwrap()) == before, String::new());

    let r = d.redact_apply(false, [0.0, 0.0, 0.0], "REDACTED").unwrap();
    check("applied", r.pages == 2 && r.marks == 2 && r.left == 0, format!("{} pages, {} marks, {} letters left", r.pages, r.marks, r.left));
    // The line's box overlaps its neighbours, so their words come along; the line's own must be gone.
    let own: String = words(&body.t).chars().take(60).collect();
    let after = words(&d.area_text(0, line).unwrap());
    check("line removed", after.contains("REDACTED") && !after.contains(own.trim()), format!("{own:?} gone from {after:?}"));
    let after2 = words(&d.area_text(1, area).unwrap());
    check("area removed", !after2.contains("dolor") && !after2.contains("amet") && !area_before.is_empty(), format!("{:?} -> {after2:?}", area_before.chars().take(40).collect::<String>()));
    let rest = words(&d.area_text(0, [0.0, 0.0, w, h]).unwrap());
    check("rest of the page kept", rest.contains("Page 1") && rest.split(' ').count() > 300, format!("{} words", rest.split(' ').count()));
    check("no marks left", d.redaction_pages().unwrap().is_empty(), String::new());
    // Search reads the pages as they are now, not the file on disk.
    let found_after = hits_on_first(needle.trim());
    check("search no longer finds removed text", found_after < found_before, format!("{needle:?}: {found_before} hits before, {found_after} after"));
    d.undo(false).unwrap();
    check("undo", words(&d.area_text(0, line).unwrap()) == before && d.redaction_pages().unwrap().len() == 2, String::new());
    d.undo(true).unwrap();

    // Cover only: the text is still there under the box.
    let cover = [50.0, 200.0, 300.0, 260.0];
    let under = words(&d.area_text(2, cover).unwrap());
    d.redact_mark(&[RedactMark { page: 2, quads: vec![], rect: Some(cover) }], "Tester").unwrap();
    let r = d.redact_apply(true, [0.1, 0.2, 0.6], "").unwrap();
    let still = words(&d.area_text(2, cover).unwrap());
    check("cover only keeps the text (not secure)", r.marks == 1 && still == under && !under.is_empty() && r.left > 0, format!("{} letters under", r.left));

    // Saved and reopened: the removed text isn't anywhere in the file.
    let n = info.page_count;
    let saved = engine.save(info.doc_id, (0..n).collect(), vec![0; n], None).unwrap();
    let d = engine.doc(saved.doc_id).unwrap();
    let after = words(&d.area_text(0, line).unwrap());
    check("after saving", after.contains("REDACTED") && !after.contains(own.trim()) && d.redaction_pages().unwrap().is_empty(), String::new());
    engine.close(saved.doc_id);

    // A scan: the picture's pixels under the box are replaced.
    let p = format!("{tmp}/scan.pdf");
    std::fs::copy(format!("{dir}/scan-300.pdf"), &p).unwrap();
    let info = engine.open(&p).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();
    let id0 = d.pictures(0, true).unwrap()[0].id.clone();
    d.redact_mark(&[RedactMark { page: 0, quads: vec![], rect: Some([60.0, 60.0, 300.0, 120.0]) }], "Tester").unwrap();
    let t = std::time::Instant::now();
    let r = d.redact_apply(false, [0.0, 0.0, 0.0], "").unwrap();
    let id1 = d.pictures(0, true).unwrap().first().map(|p| p.id.clone()).unwrap_or_default();
    check("scan pixels replaced", r.marks == 1 && id0 != id1 && !id1.is_empty(), format!("{} ms", t.elapsed().as_millis()));
    engine.close(info.doc_id);

    // Sanitize: attachments, comments, metadata, scripts; then hidden (OCR-style) text.
    let p = format!("{tmp}/files.pdf");
    std::fs::write(&p, "%PDF-1.7
1 0 obj <</Type/Catalog/Pages 2 0 R/Names<</EmbeddedFiles<</Names[(a.txt) 4 0 R]>>/JavaScript<</Names[(x) 6 0 R]>>>>/OpenAction 6 0 R>> endobj
2 0 obj <</Type/Pages/Kids[3 0 R]/Count 1>> endobj
3 0 obj <</Type/Page/Parent 2 0 R/MediaBox[0 0 300 300]/Contents 7 0 R/Resources<</Font<</F1<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>>>>>/Annots[8 0 R]>> endobj
4 0 obj <</Type/Filespec/F(a.txt)/EF<</F 5 0 R>>>> endobj
5 0 obj <</Type/EmbeddedFile/Length 2>> stream
hi
endstream endobj
6 0 obj <</S/JavaScript/JS(app.alert(1))>> endobj
7 0 obj <</Length 88>> stream
BT /F1 12 Tf 20 200 Td (Seen text) Tj ET BT 3 Tr /F1 12 Tf 20 100 Td (Hidden words) Tj ET
endstream endobj
8 0 obj <</Type/Annot/Subtype/Text/Rect[10 10 30 30]/Contents(a note)>> endobj
trailer <</Root 1 0 R/Size 9/Info<</Title(Secret title)/Author(Someone)>>>>
%%EOF
").unwrap();
    let info = engine.open(&p).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();
    let all = |d: &paginaut_lib::engine::Doc| words(&d.area_text(0, [0.0, 0.0, 300.0, 300.0]).unwrap());
    check("sample has hidden text", all(&d).contains("Hidden words"), all(&d));
    let r = d.sanitize(1 | 2 | 4 | 8).unwrap();
    check("sanitized", r.meta >= 2 && r.scripts >= 2 && r.files == 1 && r.comments == 1, format!("meta {} scripts {} files {} comments {}", r.meta, r.scripts, r.files, r.comments));
    check("attachments gone", d.attachments().unwrap().is_empty() && d.properties().title.is_empty(), String::new());
    check("hidden text kept unless asked", all(&d).contains("Hidden words"), String::new());
    let r = d.sanitize(16).unwrap();
    let t = all(&d);
    check("hidden text removed, seen text kept", r.hidden >= 10 && !t.contains("Hidden") && t.contains("Seen text"), format!("{} letters, {t:?}", r.hidden));
    d.undo(false).unwrap();
    check("undo", all(&d).contains("Hidden words"), String::new());
    engine.close(info.doc_id);

    println!("{}", if failures == 0 { "ALL PASSED".to_string() } else { format!("{failures} FAILED") });
    std::process::exit(failures);
}
