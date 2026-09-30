//! OCR job check: cargo run --release --example ocr_job -- <scan.pdf> <pages> <scratch-dir> [langs]
//! Recognises the first <pages> pages in parallel, adds the text, searches it,
//! checks a second run adds nothing and a run replacing the text reads it again without doubling it, undoes/redoes, saves those pages as a new
//! PDF, reopens it and searches again.
use paginaut_lib::engine::{ocr::Models, Engine, SearchOptions};
use std::sync::mpsc;
use std::time::Instant;

fn models(langs: &str) -> Models {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../native/tessdata");
    Models { langs: langs.into(), files: langs.split('+').map(|l| (l.to_string(), format!("{dir}/{l}.traineddata"))).collect() }
}

fn run(engine: &std::sync::Arc<Engine>, doc: u64, pages: usize, langs: &str, replace: bool) -> paginaut_lib::engine::ocr::OcrEnd {
    let (tx, rx) = mpsc::channel();
    let t0 = Instant::now();
    engine
        .ocr(doc, (0..pages).collect(), models(langs), replace, 77, |done, total| if done % 8 == 0 || done == total { eprintln!("  {done}/{total}") }, move |end| tx.send(end).unwrap())
        .unwrap();
    let end = rx.recv().unwrap();
    println!(
        "ocr: {:?}, complete={} pages={} replaced={} words={} changed={} error={:?}",
        t0.elapsed(),
        end.complete,
        end.pages,
        end.replaced,
        end.words,
        end.changed,
        end.error
    );
    end
}

fn count(engine: &std::sync::Arc<Engine>, doc: u64, needle: &str) -> usize {
    let (tx, rx) = mpsc::channel();
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let h = hits.clone();
    engine
        .search(doc, needle.into(), SearchOptions::default(), 99, move |_, p| { h.fetch_add(p.quads.len(), std::sync::atomic::Ordering::Relaxed); }, |_, _| {}, move |_| tx.send(()).unwrap())
        .unwrap();
    rx.recv().unwrap();
    hits.load(std::sync::atomic::Ordering::Relaxed)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let pages: usize = args[2].parse().unwrap();
    let dir = std::path::Path::new(&args[3]);
    let langs = args.get(4).cloned().unwrap_or_else(|| "eng".into());
    std::fs::create_dir_all(dir).unwrap();
    let engine = Engine::new(18, 128 << 20).unwrap();
    let info = engine.open(&args[1]).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();
    println!("scan check: {}", serde_json::to_string(&d.scan_check().unwrap()).unwrap());

    let end = run(&engine, info.doc_id, pages, &langs, false);
    assert!(end.changed);
    println!("page 1 text: {:?}", d.text(0).unwrap().lock().all().chars().take(160).collect::<String>());
    println!("scan check after: {}", serde_json::to_string(&d.scan_check().unwrap()).unwrap());
    let word = std::env::var("OCR_WORD").unwrap_or_else(|_| "veniam".into());
    let word = word.as_str();
    println!("'{word}' in memory: {}", count(&engine, info.doc_id, word));
    let hits: usize = (0..pages).map(|i| serde_json::from_str::<Vec<serde_json::Value>>(&d.text(i).unwrap().lock().find(&d.bounds(i).unwrap(), word, 0, false).unwrap()).unwrap().len()).sum();
    println!("'{word}' via page text: {hits}");

    // A second run finds nothing new: the text layer covers what was read.
    let again = run(&engine, info.doc_id, pages, &langs, false);
    // At most a few leftovers (words the first run dropped as unsure).
    assert!(again.words * 20 <= end.words, "second run read the pages again");

    // Recognising again, replacing: every page is read again and its text swapped, not added to.
    let chars = |d: &paginaut_lib::engine::Doc| (0..pages).map(|i| d.text(i).unwrap().lock().char_count()).sum::<usize>();
    let before = chars(&d);
    let replaced = run(&engine, info.doc_id, pages, &langs, true);
    let after = chars(&d);
    println!("replace: chars {before} -> {after}, '{word}' in memory: {}", count(&engine, info.doc_id, word));
    assert!(replaced.replaced > 0 && replaced.words * 10 >= end.words * 9, "the replacing run didn't read the pages again");
    assert!(after < before * 11 / 10, "the text was added to, not replaced");
    d.undo(false).unwrap();
    assert_eq!(chars(&d), before, "undo brings back the text as it was");
    d.undo(true).unwrap();

    d.undo(false).unwrap();
    println!("after undo, page 1 chars: {}", d.text(0).unwrap().lock().char_count());
    d.undo(true).unwrap();
    println!("after redo, page 1 chars: {}", d.text(0).unwrap().lock().char_count());

    let out = dir.join("ocr-out.pdf");
    let t0 = Instant::now();
    let n = info.page_count;
    engine.save(info.doc_id, (0..pages).collect(), vec![0; n], Some(out.to_string_lossy().into_owned())).unwrap();
    println!("saved {} pages in {:?}: {} bytes", pages, t0.elapsed(), std::fs::metadata(&out).unwrap().len());
    let reopened = engine.open(out.to_str().unwrap()).unwrap();
    println!("'{word}' in saved file: {}", count(&engine, reopened.doc_id, word));

}
