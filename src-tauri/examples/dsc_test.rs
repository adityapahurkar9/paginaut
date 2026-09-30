//! Digital signature check: cargo run --example dsc_test -- <file.pdf> <scratch-dir>
//! Signs with a throwaway in-memory certificate (nothing is added to your
//! certificate store), signs again as a second signer, checks both, then
//! checks that an edit after signing and a tampered byte are both caught.
use paginaut_lib::dsc;
use paginaut_lib::engine::{mupdf::SignRequest, Engine};
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
        dn: [name.into(), "Paginaut test".into(), String::new(), String::new(), "IN".into()],
        reason: "I approve this document".into(),
        location: "Pune".into(),
        image: None,
        flags: 1 | 4 | 8 | 16, // labels, date, name, name as graphic
        lock: false,
        max: 16384,
    };

    // First signer, visible, into a new file.
    let signed1 = dir.join("dsc-signed-1.pdf").to_string_lossy().into_owned();
    let cert = dsc::test_cert("Paginaut Test Signer").unwrap();
    println!("cert: {:?}", cert.summary().name);
    let info = engine.open(src.to_str().unwrap()).unwrap();
    // As in the app: the undo journal is on.
    engine.doc(info.doc_id).unwrap().forms_init().unwrap();
    let t = std::time::Instant::now();
    engine
        .sign(info.doc_id, Some(signed1.clone()), 0, req("Paginaut Test Signer", [300.0, 700.0, 520.0, 760.0]), |data| cert.sign(data, 0))
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
    let e = paginaut_lib::engine::mupdf::AnnotEdit { rect: Some([50.0, 50.0, 150.0, 90.0]), ..Default::default() };
    d.annot_edit(0, None, Some("Square"), e, false, "tester").unwrap();
    drop(d);
    engine.save(info.doc_id, vec![0], vec![0], None).unwrap();
    engine.close(info.doc_id);
    for (s, v) in check(&engine, &edited) {
        println!("after edit: {} changed {} invalidAt {} | intact {}", s["name"], s["changed"], s["invalidAt"], v.intact);
    }

    // A changed byte inside the signed part: caught.
    let mut bytes = std::fs::read(&signed1).unwrap();
    let pos = bytes.windows(12).position(|w| w == b"Paginaut tes").unwrap_or(200);
    bytes[pos] ^= 0x20;
    let tampered = dir.join("dsc-tampered.pdf").to_string_lossy().into_owned();
    std::fs::write(&tampered, bytes).unwrap();
    for (s, v) in check(&engine, &tampered) {
        println!("tampered: {} | intact {} trust {} {}", s["name"], v.intact, v.trust, v.detail);
    }

    // Signing in place that fails (a cancelled PIN, no timestamp): the file is exactly as it was; then it works.
    let inplace = dir.join("dsc-inplace.pdf").to_string_lossy().into_owned();
    std::fs::copy(&src, &inplace).unwrap();
    let before = std::fs::read(&inplace).unwrap();
    let info = engine.open(&inplace).unwrap();
    let r = engine.sign(info.doc_id, None, 0, req("Cancelled", [0.0; 4]), |_| Err("Signing was cancelled.".into()));
    let after = std::fs::read(&inplace).unwrap();
    println!("in place, failed: {:?} | file unchanged {} | no temporary left {}", r.err(), before == after, !std::path::Path::new(&format!("{inplace}.signing.tmp")).exists());
    engine.sign(info.doc_id, None, 0, req("Paginaut Test Signer", [0.0; 4]), |data| cert.sign(data, 0)).unwrap();
    engine.close(info.doc_id);
    let sigs = check(&engine, &inplace);
    println!("in place, then signed: {} signature(s), all intact {}", sigs.len(), sigs.iter().all(|(_, v)| v.intact));

    // A certificate file (PAGINAUT_TEST_PFX, password in PAGINAUT_TEST_PFX_PW), and a trusted timestamp from the
    // service in PAGINAUT_TEST_TSA (needs internet). Both optional.
    let pfx = std::env::var("PAGINAUT_TEST_PFX").ok();
    let tsa = std::env::var("PAGINAUT_TEST_TSA").ok();
    if pfx.is_none() && tsa.is_none() {
        return;
    }
    let file_cert = match &pfx {
        Some(p) => {
            let wrong = dsc::Cert::from_pfx(p, "not the password").err();
            println!("pfx, wrong password: {wrong:?}");
            dsc::Cert::from_pfx(p, &std::env::var("PAGINAUT_TEST_PFX_PW").unwrap_or_default()).unwrap()
        }
        None => dsc::test_cert("Timestamp Signer").unwrap(),
    };
    println!("signer: {:?}", file_cert.summary().name);
    let stamped = dir.join("dsc-timestamped.pdf").to_string_lossy().into_owned();
    let info = engine.open(src.to_str().unwrap()).unwrap();
    engine.doc(info.doc_id).unwrap().forms_init().unwrap();
    let mut r = req("File Signer", [0.0; 4]);
    r.max = 40 * 1024;
    let t = std::time::Instant::now();
    engine
        .sign(info.doc_id, Some(stamped.clone()), 0, r, |data| {
            let cms = file_cert.sign(data, 0)?;
            match &tsa {
                Some(url) => dsc::add_timestamp(&cms, url).map(|(c, when)| {
                    println!("timestamp from {url}: {when} ({} bytes of signature)", c.len());
                    c
                }),
                None => Ok(cms),
            }
        })
        .unwrap();
    println!("signed in {:?}", t.elapsed());
    engine.close(info.doc_id);
    let mut signed_i = 0;
    for (s, v) in check(&engine, &stamped) {
        signed_i = s["i"].as_u64().unwrap() as usize;
        println!("file/timestamp: {} | intact {} trust {} signer {:?} | timestamp {} by {:?} trusted {} | {}", s["name"], v.intact, v.trust, v.signer, v.timestamp, v.tsa, v.timestamp_trusted, v.detail);
    }
    let info = engine.open(&stamped).unwrap();
    let (_, cms) = engine.sig_data(info.doc_id, signed_i).unwrap();
    engine.close(info.doc_id);
    let rv = dsc::revocation(&cms);
    println!("revocation: {} ({})", rv.status, rv.detail);
}
