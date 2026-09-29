//! Engine benchmark without the UI:
//!   cargo run --release --example bench -- <file.pdf> [workers] [dpi]
use folio_lib::engine::{Engine, RenderReq};
use std::sync::mpsc;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: bench <file> [workers] [dpi]");
    let workers: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(16);
    let dpi: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(150.0);

    let t0 = Instant::now();
    let engine = Engine::new(workers, 512 << 20).unwrap();
    let t_init = t0.elapsed();

    let t1 = Instant::now();
    let info = engine.open(path).unwrap();
    let t_open = t1.elapsed();

    let t2 = Instant::now();
    let sizes = engine.page_sizes(info.doc_id).unwrap();
    let t_sizes = t2.elapsed();

    let n = info.page_count;
    let (tx, rx) = mpsc::channel();
    let t3 = Instant::now();
    for p in 0..n {
        let tx = tx.clone();
        let req = RenderReq { page: p, scale: dpi / 72.0, rotate: 0, clip: None, extras: true };
        engine.render(info.doc_id, req, 0, 0, move |r| {
            tx.send(r.map(|o| o.map(|x| (x.width, x.height)))).unwrap();
        });
    }
    drop(tx);
    let mut ok = 0;
    let mut first = None;
    for r in rx {
        match r {
            Ok(Some(_)) => {
                ok += 1;
                if first.is_none() {
                    first = Some(t3.elapsed());
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("render error: {e}"),
        }
    }
    let t_render = t3.elapsed();

    // Single page latency with a warm cache vs a cold page.
    let one = |p: usize| {
        let (tx, rx) = mpsc::channel();
        let t = Instant::now();
        let req = RenderReq { page: p, scale: 1.5 * 96.0 / 72.0, rotate: 0, clip: None, extras: true };
        engine.render(info.doc_id, req, 0, 100, move |r| tx.send(r.is_ok()).unwrap());
        rx.recv().unwrap();
        t.elapsed()
    };
    let warm = one(0);

    println!("file        : {path}");
    println!("pages       : {n}  (first page {:?} pt)", sizes.first());
    println!("workers     : {workers}");
    println!("engine init : {:?}", t_init);
    println!("open        : {:?}", t_open);
    println!("page sizes  : {:?}", t_sizes);
    println!("render all  : {:?} @ {dpi} dpi ({ok} ok, first page after {:?})", t_render, first.unwrap_or_default());
    println!("per page    : {:.1} ms wall", t_render.as_secs_f64() * 1000.0 / n as f64);
    println!("page 1 @150%: {:?} (display list cached)", warm);
}
