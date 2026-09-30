//! Shaped text check: cargo run --release --example shaped_dump -- <testfiles dir> <temp dir> [text]
//! Stamps a line of (Marathi) text on a copy of a page, saves it, and prints what reading it back gives: the text
//! and its words with their boxes.
use paginaut_lib::engine::mupdf::StampPart;
use paginaut_lib::engine::{Engine, StampPage};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, tmp) = (&args[1], &args[2]);
    let line = args.get(3).cloned().unwrap_or_else(|| "पान क्रमांक २ — महाराष्ट्र किंमत".into());
    std::fs::create_dir_all(tmp).unwrap();
    let p = format!("{tmp}/shaped-src.pdf");
    std::fs::copy(format!("{dir}/form.pdf"), &p).unwrap();
    let engine = Engine::new(4, 64 << 20).unwrap();
    let info = engine.open(&p).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();
    let b = d.bounds(0).unwrap();
    let part = StampPart {
        text: Some(line.clone()),
        image: None,
        font: Some("sans-serif".into()),
        size: 14.0,
        color: [0.0, 0.0, 0.0],
        bold: false,
        align: Some("left".into()),
        width: b[2] - b[0] - 72.0,
        height: 0.0,
        valign: 0,
        matrix: [1.0, 0.0, 0.0, 1.0, 36.0, 30.0],
        opacity: 1.0,
    };
    d.stamp_pages(&[StampPage { page: 0, parts: vec![part] }], "Header", false, false).unwrap();
    let out = format!("{tmp}/shaped.pdf");
    engine.save(info.doc_id, vec![0], vec![0], Some(out.clone())).unwrap();
    engine.close(info.doc_id);
    let again = engine.open(&out).unwrap();
    let text = engine.doc(again.doc_id).unwrap().text(0).unwrap().lock().all();
    let got = text.lines().find(|l| l.contains(line.chars().next().unwrap())).unwrap_or("");
    println!("wrote: {line}\nread:  {got}\nsame: {}", got.contains(&line));
    for (json, _) in engine.page_words(again.doc_id, 1, |_| {}).unwrap().into_iter().take(1) {
        let words: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap_or_default();
        for w in words.iter().filter(|w| w.to_string().chars().any(|c| ('\u{900}'..='\u{97f}').contains(&c))) {
            println!("  {w}");
        }
    }
}
