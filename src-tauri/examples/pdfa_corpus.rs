//! veraPDF's test corpus against our PDF/A check and Save as PDF/A:
//!   cargo run --release --example pdfa_corpus -- <corpus folder, e.g. veraPDF-corpus/PDF_A-2b> <part> <out folder>
//! The corpus names each file for what veraPDF expects ("...-pass-a.pdf" conforms, "...-fail-a.pdf" breaks the clause
//! its folder is named for). For each: our check (a "pass" file should show nothing, a "fail" one something), then a
//! PDF/A copy into <out folder>, for tools/pdfa-validate.ps1 to check with veraPDF. A line per file in
//! <out folder>/results.tsv: name, expected, what our check found, what saving did or why it couldn't.
use paginaut_lib::engine::pdfa::FontMissing;
use paginaut_lib::engine::Engine;
use std::path::{Path, PathBuf};

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
            out.push(p);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, part, tmp) = (Path::new(&args[1]), args[2].parse::<i32>().unwrap(), &args[3]);
    std::fs::create_dir_all(tmp).unwrap();
    let engine = Engine::new(4, 64 << 20).unwrap();
    let find = |name: &str, bold: bool, italic: bool| {
        paginaut_lib::fonts::find(name, bold, italic).map(|f| (f.path, f.family, f.exact)).map_err(|m| match m {
            paginaut_lib::fonts::Missing::Licence => FontMissing::Licence,
            paginaut_lib::fonts::Missing::NotInstalled => FontMissing::NotInstalled,
        })
    };
    let mut files = Vec::new();
    walk(dir, &mut files);
    files.sort();
    let mut tsv = String::from("file\texpected\tfound\tsaved\n");
    let (mut alarms, mut missed, mut caught, mut quiet, mut unsaved) = (0, 0, 0, 0, 0);
    let suffix = if part == 2 { String::new() } else { format!("-{part}") };
    for path in files {
        let stem = path.file_stem().unwrap().to_string_lossy().replace("veraPDF test suite ", "");
        let expected = if stem.contains("-pass") || stem.contains("pass-") { "pass" } else { "fail" };
        let src = path.to_string_lossy().into_owned();
        // Named before it's opened: a crash shows which file.
        eprintln!("> {stem}");
        let t = std::time::Instant::now();
        let info = match engine.open(&src) {
            Ok(i) => i,
            Err(e) => {
                tsv.push_str(&format!("{stem}\t{expected}\topen: {e}\t\n"));
                unsaved += 1;
                continue;
            }
        };
        if info.needs_password {
            tsv.push_str(&format!("{stem}\t{expected}\tneeds a password\t\n"));
            engine.close(info.doc_id);
            continue;
        }
        let (found, said) = match engine.pdfa_check(info.doc_id, part) {
            Ok(r) => (
                r.issues.iter().map(|i| format!("{}{}", i.id, if i.fix { "" } else { "(!)" })).collect::<Vec<_>>().join(" "),
                r.issues.iter().map(|i| i.text.clone()).collect::<Vec<_>>().join(" | "),
            ),
            Err(e) => (format!("check: {e}"), String::new()),
        };
        match (expected, found.is_empty()) {
            ("pass", false) => alarms += 1,
            ("pass", true) => quiet += 1,
            (_, true) => missed += 1,
            _ => caught += 1,
        }
        if std::env::var("PDFA_CHECK_ONLY").is_ok() {
            println!("{stem} [{expected}] found: {found}
    {said}");
            engine.close(info.doc_id);
            continue;
        }
        let out = format!("{tmp}/{stem} (PDF-A{suffix}).pdf");
        let n = info.page_count;
        let saved = match engine.save_pdfa(info.doc_id, (0..n).collect(), vec![0; n], &out, part, &find) {
            Ok(s) => {
                let left: Vec<_> = s.left.issues.iter().map(|i| i.id).collect();
                if left.is_empty() { "ok".to_string() } else { format!("left: {}", left.join(" ")) }
            }
            Err(e) => {
                unsaved += 1;
                format!("error: {e}")
            }
        };
        engine.close(info.doc_id);
        let ms = t.elapsed().as_millis();
        println!("{stem} [{expected}] found: {found} | {saved} ({ms} ms)
    {said}");
        tsv.push_str(&format!("{stem}\t{expected}\t{found}\t{saved}\n"));
    }
    std::fs::write(format!("{tmp}/results.tsv"), tsv).unwrap();
    println!("pass files: {quiet} clean, {alarms} with issues we reported (false alarms?)");
    println!("fail files: {caught} caught, {missed} we saw nothing in");
    println!("not saved: {unsaved}");
}
