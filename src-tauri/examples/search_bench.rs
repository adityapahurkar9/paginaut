//! Search scaling benchmark: cargo run --release --example search_bench -- <file> <needle> [workers...]
use folio_lib::engine::Engine;
use std::sync::mpsc;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let needle = &args[2];
    let counts: Vec<usize> = args[3..].iter().filter_map(|s| s.parse().ok()).collect();
    for &w in if counts.is_empty() { &[1usize, 4, 16][..] } else { &counts[..] } {
        let engine = Engine::new(w, 128 << 20).unwrap();
        let info = engine.open(path).unwrap();
        let (tx, rx) = mpsc::channel();
        let t = Instant::now();
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let h2 = hits.clone();
        engine
            .search(info.doc_id, needle.clone(), Default::default(), 1, move |_, v| {
                h2.fetch_add(v.quads.len(), std::sync::atomic::Ordering::Relaxed);
            }, |_, _| {}, move |_| tx.send(()).unwrap())
            .unwrap();
        rx.recv().unwrap();
        println!("{w:>2} workers: {:?}  ({} hits)", t.elapsed(), hits.load(std::sync::atomic::Ordering::Relaxed));
    }
}
