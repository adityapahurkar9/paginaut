//! PDF/A: check, Save as PDF/A, check again: cargo run --release --example pdfa_test -- <testfiles dir> <temp dir>
//! Every PDF in the folder is saved as PDF/A-2b into the temp folder; what's left must be only what can't be fixed,
//! and page 1 must look as it did. Works on copies; never touches the originals.
use paginaut_lib::engine::mupdf::render_size;
use paginaut_lib::engine::pdfa::{FontMissing, Report};
use paginaut_lib::engine::{Engine, Protection, SaveOptions};

/// Page 1 rendered small (RGBA).
fn thumb(engine: &Engine, path: &str) -> Vec<u8> {
    let info = engine.open(path).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    let list = d.list(0, 1).unwrap();
    let b = d.bounds(0).unwrap();
    let (w, h) = render_size(&b, 0.25, 0);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    list.render(&b, 0.25, 0, [0, 0, w as i32, h as i32], &mut buf, None).unwrap();
    drop(list);
    engine.close(info.doc_id);
    buf
}

fn diff(a: &[u8], b: &[u8]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 255.0;
    }
    a.iter().zip(b).map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs() as f64).sum::<f64>() / a.len() as f64
}

fn show(r: &Report) -> String {
    r.issues.iter().map(|i| format!("{}{}", i.id, if i.fix { "" } else { "(!)" })).collect::<Vec<_>>().join(" ")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, tmp) = (&args[1], &args[2]);
    std::fs::create_dir_all(tmp).unwrap();
    let engine = Engine::new(4, 64 << 20).unwrap();
    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failures += 1;
        }
    };
    let find = |name: &str, bold: bool, italic: bool| {
        paginaut_lib::fonts::find(name, bold, italic).map(|f| (f.path, f.family, f.exact)).map_err(|m| match m {
            paginaut_lib::fonts::Missing::Licence => FontMissing::Licence,
            paginaut_lib::fonts::Missing::NotInstalled => FontMissing::NotInstalled,
        })
    };
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "pdf")).collect();
    files.sort();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let src = path.to_string_lossy().into_owned();
        let info = match engine.open(&src) {
            Ok(i) => i,
            Err(e) => {
                println!("skip {name}: {e}");
                continue;
            }
        };
        if info.needs_password {
            // Checked without unlocking: only the protection can be said.
            println!("skip {name}: needs a password");
            engine.close(info.doc_id);
            continue;
        }
        let t = std::time::Instant::now();
        let before = engine.pdfa_check(info.doc_id, 2).unwrap();
        println!("{name}: {} ({} ms)", show(&before), t.elapsed().as_millis());
        let out = format!("{tmp}/{} (PDF-A).pdf", name.trim_end_matches(".pdf"));
        let n = info.page_count;
        let t = std::time::Instant::now();
        let saved = engine.save_pdfa(info.doc_id, (0..n).collect(), vec![0; n], &out, 2, &find);
        engine.close(info.doc_id);
        match saved {
            Ok(s) => {
                println!("  done ({} ms): {}", t.elapsed().as_millis(), s.done.join("; "));
                if !s.substitutes.is_empty() {
                    println!("  stand-ins: {}", s.substitutes.join("; "));
                }
                let fixable: Vec<_> = s.left.issues.iter().filter(|i| i.fix).map(|i| format!("{} [{}]", i.id, i.text)).collect();
                check(&format!("{name} conforms after saving"), fixable.is_empty() && s.left.claim.as_ref().is_some_and(|c| c.part == 2), format!("left: {}", show(&s.left)));
                let d = diff(&thumb(&engine, &src), &thumb(&engine, &out));
                // Fonts put in where none were can change the letters a little; nothing else should.
                let limit = if before.issues.iter().any(|i| i.id == "fonts") { 6.0 } else { 1.0 };
                check(&format!("{name} page 1 looks the same"), d < limit, format!("differs by {d:.2}"));
                let after = engine.open(&out).unwrap();
                engine.close(after.doc_id);
                // A damaged file can lose pages it can't read (said in `done`); nothing else may.
                let lost = s.done.iter().any(|t| t.contains("left out"));
                check(&format!("{name} keeps its pages"), after.page_count == n || lost, format!("{n} → {}", after.page_count));
                let bytes = std::fs::read(&out).unwrap();
                // XMP from standard schemas survives the rewrite; a labelling tool's own doesn't (tools/make-pdfa-samples.py).
                let has = |s: &str| bytes.windows(s.len()).any(|w| w == s.as_bytes());
                if name == "rights.pdf" {
                    check("rights.pdf keeps its rights and IDs", has("Copyright 2026 Paginaut tests") && has("<xmpMM:DocumentID>uuid:1234-abcd"), String::new());
                }
                if name == "untyped-font.pdf" {
                    check("untyped-font.pdf gets its font", s.done.iter().any(|d| d.contains("1 font put in")), s.done.join("; "));
                }
                if name == "labelled.pdf" {
                    check("labelled.pdf drops the labelling properties", !has("pdfx") && !has("MSIP"), String::new());
                }
                check(&format!("{name} has a file ID"), bytes.windows(4).any(|w| w == b"/ID[" || w == b"/ID "), String::new());
            }
            Err(e) => check(&format!("{name} saved as PDF/A"), false, e),
        }
    }
    // The rest works on testfiles' labels.pdf (other folders: only the files above).
    if !std::path::Path::new(&format!("{dir}/labels.pdf")).exists() {
        println!("{}", if failures == 0 { "ALL PASSED" } else { "SOME FAILED" });
        std::process::exit(if failures == 0 { 0 } else { 1 });
    }

    // An attached file that isn't a PDF: PDF/A-2 takes it out, PDF/A-3 keeps it, described.
    let with = format!("{tmp}/attached.pdf");
    let note = format!("{tmp}/note.txt");
    std::fs::copy(format!("{dir}/labels.pdf"), &with).unwrap();
    std::fs::write(&note, "an attached note").unwrap();
    let info = engine.open(&with).unwrap();
    engine.doc(info.doc_id).unwrap().attachment_add(&note).unwrap();
    let n = info.page_count;
    let before2 = engine.pdfa_check(info.doc_id, 2).unwrap();
    let before3 = engine.pdfa_check(info.doc_id, 3).unwrap();
    check("attachment found (part 2)", before2.issues.iter().any(|i| i.id == "attachments"), show(&before2));
    check("attachment found (part 3)", before3.issues.iter().any(|i| i.id == "attachments"), show(&before3));
    let out2 = format!("{tmp}/attached (PDF-A-2).pdf");
    let out3 = format!("{tmp}/attached (PDF-A-3).pdf");
    let s2 = engine.save_pdfa(info.doc_id, (0..n).collect(), vec![0; n], &out2, 2, &find).unwrap();
    let s3 = engine.save_pdfa(info.doc_id, (0..n).collect(), vec![0; n], &out3, 3, &find).unwrap();
    // Copies leave the open document as it was: its attachment is still unsaved.
    check("copies leave the document unsaved", engine.doc(info.doc_id).unwrap().has_changes(), String::new());
    engine.close(info.doc_id);
    let files = |p: &str| {
        let i = engine.open(p).unwrap();
        let n = engine.doc(i.doc_id).unwrap().attachments().unwrap().len();
        engine.close(i.doc_id);
        n
    };
    check("PDF/A-2 takes the attachment out", s2.left.issues.is_empty() && files(&out2) == 0, format!("{} / {}", show(&s2.left), s2.done.join("; ")));
    check("PDF/A-3 keeps it, described", s3.left.issues.is_empty() && files(&out3) == 1 && s3.left.claim.as_ref().is_some_and(|c| c.part == 3), format!("{} / {}", show(&s3.left), s3.done.join("; ")));

    // A permissions password the user doesn't have: protection can't be taken off for them; with it, it can.
    let locked = format!("{tmp}/protected.pdf");
    std::fs::copy(format!("{dir}/labels.pdf"), &locked).unwrap();
    let info = engine.open(&locked).unwrap();
    let n = info.page_count;
    let prot = Protection { remove: false, user: "test".into(), owner: "own".into(), perms: -1 };
    let saved = engine.save_with(info.doc_id, (0..n).collect(), vec![0; n], None, SaveOptions { protect: Some(prot), ..Default::default() }).unwrap();
    engine.close(saved.doc_id);
    let info = engine.open(&locked).unwrap();
    let info = engine.unlock(info.doc_id, "test").unwrap().expect("unlocks");
    let out = format!("{tmp}/protected (PDF-A).pdf");
    let refused = engine.save_pdfa(info.doc_id, (0..n).collect(), vec![0; n], &out, 2, &find);
    check("open password alone can't take protection off", refused.as_ref().is_err_and(|e| e.contains("permissions password")), format!("{:?}", refused.as_ref().err()));
    engine.check_owner(info.doc_id, "own").unwrap();
    let s = engine.save_pdfa(info.doc_id, (0..n).collect(), vec![0; n], &out, 2, &find).unwrap();
    engine.close(info.doc_id);
    let reopened = engine.open(&out).unwrap();
    check("with the owner's password, saved unprotected", !reopened.needs_password && s.left.issues.is_empty(), format!("{} / {}", show(&s.left), s.done.join("; ")));
    engine.close(reopened.doc_id);

    println!("{}", if failures == 0 { "ALL PASSED" } else { "SOME FAILED" });
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
