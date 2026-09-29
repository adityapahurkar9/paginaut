//! Scan check: cargo run --release --example ocr_check -- <file>...
//! Prints how many sampled pages would be offered text recognition (see Doc::scan_check).
use paginaut_lib::engine::Engine;

fn main() {
    let engine = Engine::new(4, 64 << 20).unwrap();
    for path in std::env::args().skip(1) {
        let t0 = std::time::Instant::now();
        let info = engine.open(&path).unwrap();
        let check = engine.doc(info.doc_id).unwrap().scan_check().unwrap();
        println!("{path}: {} of {} sampled pages look scanned ({:?})", check.scans, check.sampled, t0.elapsed());
        engine.close(info.doc_id);
    }
}
