//! Reflow extraction check: cargo run --release --example reflow_dump -- <file> <out-dir> [pages...]
//! Times extracting every page, and writes the JSON of the listed pages to <out-dir>/p<N>.json.
use paginaut_lib::engine::Engine;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (path, out) = (&args[1], &args[2]);
    let keep: Vec<usize> = args[3..].iter().filter_map(|s| s.parse().ok()).collect();
    let engine = Engine::new(18, 128 << 20).unwrap();
    let info = engine.open(path).unwrap();
    let (tx, rx) = mpsc::channel();
    let pages = Arc::new(Mutex::new(Vec::new()));
    let p2 = pages.clone();
    let t = Instant::now();
    engine
        .reflow(info.doc_id, 0, 1, move |i, json| p2.lock().unwrap().push((i, json)), move |c, e| tx.send((c, e)).unwrap())
        .unwrap();
    let (complete, error) = rx.recv().unwrap();
    let pages = pages.lock().unwrap();
    let bytes: usize = pages.iter().map(|p| p.1.len()).sum();
    println!("{} pages in {:?}, {} KB of JSON, complete={complete} error={error:?}", pages.len(), t.elapsed(), bytes / 1024);
    std::fs::create_dir_all(out).unwrap();
    for (i, json) in pages.iter() {
        if keep.contains(i) {
            std::fs::write(format!("{out}/p{i}.json"), json).unwrap();
        }
    }
}
