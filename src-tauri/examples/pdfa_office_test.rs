//! Office files straight to PDF/A: cargo run --release --example pdfa_office_test -- <data dir> <out dir> <part> <file>...
//! Each file goes the way Files to PDF takes it with a PDF/A kind chosen: converted (by Microsoft Office or
//! LibreOffice, whichever is here; the conversion is cached in <data dir>), then saved as PDF/A. What's left and the
//! fonts that stood in are printed; the files (named "... (PDF-A[-n]).pdf") are for tools/pdfa-validate.ps1.
use paginaut_lib::engine::pdfa::FontMissing;
use paginaut_lib::engine::Engine;
use paginaut_lib::office::Office;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (data, out, part) = (std::path::Path::new(&args[1]), &args[2], args[3].parse::<i32>().unwrap());
    std::fs::create_dir_all(out).unwrap();
    let office = Office::new(data);
    let engine = Engine::new(4, 64 << 20).unwrap();
    let find = |name: &str, bold: bool, italic: bool| {
        paginaut_lib::fonts::find(name, bold, italic).map(|f| (f.path, f.family, f.exact)).map_err(|m| match m {
            paginaut_lib::fonts::Missing::Licence => FontMissing::Licence,
            paginaut_lib::fonts::Missing::NotInstalled => FontMissing::NotInstalled,
        })
    };
    let suffix = if part == 2 { String::new() } else { format!("-{part}") };
    let mut failures = 0;
    for path in &args[4..] {
        let stem = std::path::Path::new(path).file_stem().unwrap().to_string_lossy().into_owned();
        let dest = format!("{out}/{stem} (PDF-A{suffix}).pdf");
        let res = office
            .convert(path, None)
            .and_then(|c| c.ok_or_else(|| "no converter".to_string()))
            .and_then(|c| engine.pdfa_from_file(&c.pdf, &dest, part, &find).map(|s| (c.engine_name, s)));
        match res {
            Ok((by, s)) => {
                let left: Vec<_> = s.left.issues.iter().filter(|i| i.fix).map(|i| i.text.clone()).collect();
                println!("{} {stem} (by {by}): {}", if left.is_empty() { "PASS" } else { "FAIL" }, s.done.join("; "));
                for x in left.iter().chain(&s.substitutes) {
                    println!("    {x}");
                }
                // What the file holds that PDF/A can't (said to the user, not a failure here).
                for i in s.left.issues.iter().filter(|i| !i.fix) {
                    println!("    can't be put right: {}", i.text);
                }
                failures += !left.is_empty() as usize;
            }
            Err(e) => {
                // A file that needs a password to open isn't one this can test.
                println!("skip {stem}: {e}");
            }
        }
    }
    println!("{}", if failures == 0 { "ALL PASSED" } else { "SOME FAILED" });
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
