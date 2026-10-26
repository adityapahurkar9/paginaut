//! Keeping a PDF/A file PDF/A: cargo run --release --example pdfa_keep_test -- <a PDF> <scratch dir> [part]
//! The PDF is saved as PDF/A first. Then a comment added to it (its text in a font the file hasn't got) shows up as
//! a change that breaks PDF/A; keeping it PDF/A and saving in place leaves a file that conforms and still holds the
//! original bytes. Signing the PDF/A file keeps it PDF/A, with a valid signature. veraPDF can check the files left:
//! "kept (PDF-A[-n]).pdf" and "signed (PDF-A[-n]).pdf" (tools/pdfa-validate.ps1).
use paginaut_lib::dsc;
use paginaut_lib::engine::mupdf::{AnnotEdit, SignRequest};
use paginaut_lib::engine::pdfa::FontMissing;
use paginaut_lib::engine::Engine;
use serde_json::Value;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (src, dir) = (&args[1], std::path::Path::new(&args[2]));
    let part: i32 = args.get(3).map_or(2, |p| p.parse().unwrap());
    let suffix = if part == 2 { String::new() } else { format!("-{part}") };
    std::fs::create_dir_all(dir).unwrap();
    let engine = Engine::new(4, 64 << 20).unwrap();
    let find = |name: &str, bold: bool, italic: bool| {
        paginaut_lib::fonts::find(name, bold, italic).map(|f| (f.path, f.family, f.exact)).map_err(|m| match m {
            paginaut_lib::fonts::Missing::Licence => FontMissing::Licence,
            paginaut_lib::fonts::Missing::NotInstalled => FontMissing::NotInstalled,
        })
    };
    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        failures += !ok as usize;
    };
    let fixable = |path: &str| {
        let r = engine.pdfa_check_file(path, part).unwrap();
        r.issues.iter().filter(|i| i.fix).map(|i| format!("{} [{}]", i.id, i.text)).collect::<Vec<_>>()
    };

    // The PDF/A file everything starts from.
    let base = dir.join(format!("base (PDF-A{suffix}).pdf")).to_string_lossy().into_owned();
    let info = engine.open(src).unwrap();
    let n = info.page_count;
    engine.save_pdfa(info.doc_id, (0..n).collect(), vec![0; n], &base, part, &find).unwrap();
    engine.close(info.doc_id);
    check("the start is PDF/A", fixable(&base).is_empty(), format!("{:?}", fixable(&base)));

    // A comment in a font the file hasn't got: a change that breaks PDF/A, put right as it's saved.
    let kept = dir.join(format!("kept (PDF-A{suffix}).pdf")).to_string_lossy().into_owned();
    std::fs::copy(&base, &kept).unwrap();
    let before = std::fs::read(&kept).unwrap();
    let info = engine.open(&kept).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();
    let note = AnnotEdit {
        rect: Some([60.0, 60.0, 260.0, 100.0]),
        font: Some(("Helv".into(), 12.0, vec![0.0, 0.0, 0.0])),
        contents: Some("Checked for the archive".into()),
        ..Default::default()
    };
    d.annot_edit(0, None, Some("FreeText"), note, false, "tester").unwrap();
    d.annot_edit(0, None, Some("Square"), AnnotEdit { rect: Some([60.0, 120.0, 160.0, 160.0]), color: Some(vec![0.0, 0.0, 1.0]), ..Default::default() }, false, "tester")
        .unwrap();
    drop(d);
    let changes = engine.pdfa_changes(info.doc_id).unwrap();
    let ids: Vec<&str> = changes.as_ref().map_or(vec![], |c| c.issues.iter().map(|i| i.id).collect());
    check("the comments are said to break it", changes.as_ref().is_some_and(|c| c.part == part && !c.issues.is_empty()), format!("{ids:?}"));
    let k = engine.pdfa_keep(info.doc_id, part, &find).unwrap();
    println!("  kept: {}", k.done.join("; "));
    check("nothing left after keeping", engine.pdfa_changes(info.doc_id).unwrap().is_none(), String::new());
    engine.save(info.doc_id, (0..n).collect(), vec![0; n], None).unwrap();
    engine.close(info.doc_id);
    let after = std::fs::read(&kept).unwrap();
    check("saved PDF/A", fixable(&kept).is_empty(), format!("{:?}", fixable(&kept)));
    check("saved as an update (the original bytes kept)", after.len() > before.len() && after[..before.len()] == before[..], format!("{} → {} bytes", before.len(), after.len()));

    // Signing a PDF/A file keeps it PDF/A, and the signature valid.
    let signed = dir.join(format!("signed (PDF-A{suffix}).pdf")).to_string_lossy().into_owned();
    let cert = dsc::test_cert("Paginaut Test Signer").unwrap();
    let req = SignRequest {
        field: -1,
        rect: [300.0, 700.0, 520.0, 760.0],
        dn: ["Paginaut Test Signer".into(), "Paginaut test".into(), String::new(), String::new(), "IN".into()],
        reason: "Archived".into(),
        location: "Pune".into(),
        image: None,
        flags: 1 | 4 | 8 | 16,
        lock: false,
        max: 16384,
    };
    let info = engine.open(&base).unwrap();
    engine.doc(info.doc_id).unwrap().forms_init().unwrap();
    engine.sign(info.doc_id, Some(signed.clone()), 0, req, |data| cert.sign(data, 0)).unwrap();
    engine.close(info.doc_id);
    check("signed, still PDF/A", fixable(&signed).is_empty(), format!("{:?}", fixable(&signed)));
    let info = engine.open(&signed).unwrap();
    let sigs: Vec<Value> = serde_json::from_str(&engine.sigs_json(info.doc_id).unwrap()).unwrap();
    let intact = sigs.iter().filter(|s| s["signed"] == true).all(|s| {
        let (data, cms) = engine.sig_data(info.doc_id, s["i"].as_u64().unwrap() as usize).unwrap();
        dsc::verify(&data, &cms, None).intact
    });
    engine.close(info.doc_id);
    check("the signature is valid", !sigs.is_empty() && intact, format!("{} signature(s)", sigs.len()));

    println!("{}", if failures == 0 { "ALL PASSED" } else { "SOME FAILED" });
    std::process::exit(if failures == 0 { 0 } else { 1 });
}
