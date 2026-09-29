//! OCR check: cargo run --release --example ocr_test -- <file> <page> [langs] [tessdata-dir]
//! Recognises one page and prints the lines, word count, confidence and timing.
use paginaut_lib::engine::mupdf::{Document, Tess};
use serde_json::Value;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let page: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let langs = args.get(3).cloned().unwrap_or_else(|| "eng".into());
    let dir = args.get(4).cloned().unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/../native/tessdata").into());
    paginaut_lib::engine::mupdf::init(64 << 20).unwrap();

    let t0 = Instant::now();
    let files: Vec<(String, String)> = langs.split('+').map(|l| (l.to_string(), format!("{dir}/{l}.traineddata"))).collect();
    let tess = Tess::new(&langs, &files).unwrap();
    println!("models loaded in {:?}", t0.elapsed());

    let doc = Document::open(path).unwrap();
    let p = doc.load_page(page).unwrap();
    let b = p.bounds().unwrap();
    let list = p.display_list(0).unwrap();
    let t1 = Instant::now();
    let json = list.ocr(&b, 300.0, &tess, None).unwrap().unwrap();
    println!("page {page} recognised in {:?}", t1.elapsed());
    let v: Value = serde_json::from_str(&json).unwrap();
    if v["blank"].as_i64() == Some(1) {
        println!("blank: nothing to read");
        return;
    }
    let lines = v["l"].as_array().unwrap();
    if std::env::var("OCR_RAW").is_ok() {
        println!("{}", &json[..json.len().min(600)]);
    }
    let mut words = 0;
    let mut conf = 0.0;
    for (i, l) in lines.iter().enumerate() {
        let ws = l["w"].as_array().unwrap();
        words += ws.len();
        conf += ws.iter().map(|w| w[4].as_f64().unwrap()).sum::<f64>();
        if i < 12 {
            let text: Vec<&str> = ws.iter().map(|w| w[5].as_str().unwrap()).collect();
            println!("  {}", text.join(" "));
        }
    }
    println!("{} lines, {} words, mean confidence {:.0}", lines.len(), words, conf / words.max(1) as f64);
    // A second page with the same recogniser (models already loaded).
    if page + 1 < doc.page_count().unwrap() {
        let p = doc.load_page(page + 1).unwrap();
        let b = p.bounds().unwrap();
        let t2 = Instant::now();
        let _ = p.display_list(0).unwrap().ocr(&b, 300.0, &tess, None).unwrap();
        println!("page {} recognised in {:?}", page + 1, t2.elapsed());
    }
}
