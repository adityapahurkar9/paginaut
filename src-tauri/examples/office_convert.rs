//! The Office quality test's engine side:
//!   cargo run --release --example office_convert -- convert <out dir> <file>...
//!     converts Office files with MuPDF's built-in support (A4 layout);
//!   cargo run --release --example office_convert -- dump <out dir> <pdf>...
//!     writes each page as <out>/<name>-<n>.ppm (1 px per point) and its text as <name>-<n>.txt,
//!     for tools/office/compare.py.
use paginaut_lib::engine::mupdf::{self, Document};
use std::time::Instant;

fn stem(path: &str) -> String {
    std::path::Path::new(path).file_stem().unwrap().to_string_lossy().into_owned()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (mode, out) = (&args[1], &args[2]);
    std::fs::create_dir_all(out).unwrap();
    mupdf::init(64 << 20).unwrap();
    for path in &args[3..] {
        let name = stem(path);
        let t = Instant::now();
        let d = match Document::open(path) {
            Ok(d) => d,
            Err(e) => {
                println!("{name}: can't open: {e}");
                continue;
            }
        };
        if d.is_reflowable() {
            d.layout(595.0, 842.0, 11.0).unwrap(); // A4
        }
        let n = d.page_count().unwrap();
        if mode == "convert" {
            let order: Vec<usize> = (0..n).collect();
            let r = d.convert_to_pdf(&format!("{out}/{name}.pdf"), &order, &vec![0; n]);
            println!("{name}: {n} pages in {:?} {r:?}", t.elapsed());
            continue;
        }
        for i in 0..n {
            let page = d.load_page(i).unwrap();
            let b = page.bounds().unwrap();
            let (w, h) = mupdf::render_size(&b, 1.0, 0);
            let mut px = vec![0u8; (w * h * 4) as usize];
            let list = page.display_list(0).unwrap();
            list.render(&b, 1.0, 0, [0, 0, w as i32, h as i32], &mut px, None).unwrap();
            let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
            ppm.extend(px.chunks(4).flat_map(|p| [p[0], p[1], p[2]]));
            std::fs::write(format!("{out}/{name}-{i}.ppm"), ppm).unwrap();
            std::fs::write(format!("{out}/{name}-{i}.txt"), list.text().map(|t| t.all()).unwrap_or_default()).unwrap();
        }
        println!("{name}: {n} pages");
    }
}
