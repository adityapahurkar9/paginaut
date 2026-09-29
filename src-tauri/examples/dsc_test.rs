//! Digital signature check: cargo run --example dsc_test -- <file.pdf> <scratch-dir>
//! Signs with a throwaway in-memory certificate (nothing is added to your
//! certificate store), signs again as a second signer, checks both, then
//! checks that an edit after signing and a tampered byte are both caught.
use folio_lib::dsc;
use folio_lib::engine::{mupdf::SignRequest, Engine};
use serde_json::Value;

fn check(engine: &Engine, path: &str) -> Vec<(Value, dsc::SigCheck)> {
    let info = engine.open(path).unwrap();
    let list: Vec<Value> = serde_json::from_str(&engine.sigs_json(info.doc_id).unwrap()).unwrap();
    let out = list
        .into_iter()
        .filter(|s| s["signed"] == true)
        .map(|s| {
            let i = s["i"].as_u64().unwrap() as usize;
            let (data, cms) = engine.sig_data(info.doc_id, i).unwrap();
            let v = dsc::verify(&data, &cms, None);
            (s, v)
        })
        .collect();
    engine.close(info.doc_id);
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::Path::new(&args[2]);
    std::fs::create_dir_all(dir).unwrap();
    let src = dir.join("dsc-src.pdf");
    std::fs::copy(&args[1], &src).unwrap();
    let engine = Engine::new(4, 64 << 20).unwrap();

    let req = |name: &str, rect: [f32; 4]| SignRequest {
        field: -1,
        rect,
        dn: [name.into(), "Folio test".into(), String::new(), String::new(), "IN".into()],
        reason: "I approve this document".into(),
        location: "Pune".into(),
        image: None,
        flags: 1 | 4 | 8 | 16, // labels, date, name, name as graphic
        lock: false,
        max: 16384,
    };

    // First signer, visible, into a new file.
    let signed1 = dir.join("dsc-signed-1.pdf").to_string_lossy().into_owned();
    let cert = dsc::test_cert("Folio Test Signer").unwrap();
    println!("cert: {:?}", cert.summary().name);
    let info = engine.open(src.to_str().unwrap()).unwrap();
    // As in the app: the undo journal is on.
    engine.doc(info.doc_id).unwrap().forms_init().unwrap();
    let t = std::time::Instant::now();
    engine
        .sign(info.doc_id, Some(signed1.clone()), 0, req("Folio Test Signer", [300.0, 700.0, 520.0, 760.0]), |data| cert.sign(data, 0))
        .unwrap();
    println!("signed 1 in {:?}", t.elapsed());
    engine.close(info.doc_id);
    for (s, v) in check(&engine, &signed1) {
        println!("after 1: {} page {} changed {} invalidAt {} | intact {} trust {} signer {:?} {}", s["name"], s["page"], s["changed"], s["invalidAt"], v.intact, v.trust, v.signer, v.detail);
    }

    // Second signer, invisible, in place on a copy.
    let signed2 = dir.join("dsc-signed-2.pdf").to_string_lossy().into_owned();
    std::fs::copy(&signed1, &signed2).unwrap();
    let cert2 = dsc::test_cert("Second Signer").unwrap();
    let info = engine.open(&signed2).unwrap();
    engine.sign(info.doc_id, None, 0, req("Second Signer", [0.0; 4]), |data| cert2.sign(data, 0)).unwrap();
    engine.close(info.doc_id);
    for (s, v) in check(&engine, &signed2) {
        println!("after 2: {} changed {} invalidAt {} | intact {} trust {} signer {:?}", s["name"], s["changed"], s["invalidAt"], v.intact, v.trust, v.signer);
    }

    // An annotation added after signing: the signatures still check out, and are marked changed.
    let edited = dir.join("dsc-edited.pdf").to_string_lossy().into_owned();
    std::fs::copy(&signed2, &edited).unwrap();
    let info = engine.open(&edited).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();
    let e = folio_lib::engine::mupdf::AnnotEdit { rect: Some([50.0, 50.0, 150.0, 90.0]), ..Default::default() };
    d.annot_edit(0, None, Some("Square"), e, false, "tester").unwrap();
    drop(d);
    engine.save(info.doc_id, vec![0], vec![0], None).unwrap();
    engine.close(info.doc_id);
    for (s, v) in check(&engine, &edited) {
        println!("after edit: {} changed {} invalidAt {} | intact {}", s["name"], s["changed"], s["invalidAt"], v.intact);
    }

    // A changed byte inside the signed part: caught.
    let mut bytes = std::fs::read(&signed1).unwrap();
    let pos = bytes.windows(9).position(|w| w == b"Folio tes").unwrap_or(200);
    bytes[pos] ^= 0x20;
    let tampered = dir.join("dsc-tampered.pdf").to_string_lossy().into_owned();
    std::fs::write(&tampered, bytes).unwrap();
    for (s, v) in check(&engine, &tampered) {
        println!("tampered: {} | intact {} trust {} {}", s["name"], v.intact, v.trust, v.detail);
    }
}
