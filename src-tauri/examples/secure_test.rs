//! Protect, compress, repair: cargo run --release --example secure_test -- <testfiles dir> <temp dir>
//! Works on copies; never touches the originals.
use paginaut_lib::engine::mupdf::render_size;
use paginaut_lib::engine::{Engine, Protection, SaveOptions};

const PRINT: i32 = 1 << 2;
const COPY: i32 = 1 << 4;

fn size(p: &str) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// Page 1 of `path` rendered small (RGBA), to compare how pages look.
fn thumb(engine: &Engine, path: &str) -> Vec<u8> {
    let info = engine.open(path).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    let list = d.list(0, 0).unwrap();
    let b = d.bounds(0).unwrap();
    let (w, h) = render_size(&b, 0.25, 0);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    list.render(&b, 0.25, 0, [0, 0, w as i32, h as i32], &mut buf, None).unwrap();
    drop(list);
    engine.close(info.doc_id);
    buf
}

/// Mean difference per channel (0-255).
fn diff(a: &[u8], b: &[u8]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 255.0;
    }
    a.iter().zip(b).map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs() as f64).sum::<f64>() / a.len() as f64
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
    let all = |n: usize| ((0..n).collect::<Vec<_>>(), vec![0; n]);

    // Protect: open password, permissions password, no printing or copying.
    let p = format!("{tmp}/text.pdf");
    std::fs::copy(format!("{dir}/text-1000.pdf"), &p).unwrap();
    let info = engine.open(&p).unwrap();
    let s = engine.security(info.doc_id).unwrap();
    check("plain file", s.method.is_empty() && s.owner && !s.opens, format!("{:?}", s.method));
    let (order, rot) = all(info.page_count);
    let prot = Protection { remove: false, user: "open1".into(), owner: "own1".into(), perms: !(PRINT | COPY) };
    let saved = engine.save_with(info.doc_id, order, rot, None, SaveOptions { protect: Some(prot), ..Default::default() }).unwrap();
    let s = engine.security(saved.doc_id).unwrap();
    check("protected in place, still open", s.method == "AES" && s.bits == 256 && s.opens && s.owner && s.perms & PRINT == 0, format!("{:?} {} {:x} opens={} owner={}", s.method, s.bits, s.perms, s.opens, s.owner));
    engine.close(saved.doc_id);

    // Reopened: needs the password; the open password alone can't change the protection.
    let info = engine.open(&p).unwrap();
    check("needs a password", info.needs_password, String::new());
    check("wrong password refused", engine.unlock(info.doc_id, "nope").map(|r| r.is_none()).unwrap_or(false), String::new());
    let info = engine.unlock(info.doc_id, "open1").unwrap().expect("unlocks");
    let s = engine.security(info.doc_id).unwrap();
    check("open password isn't the owner's", !s.owner && s.perms & COPY == 0, String::new());
    let (order, rot) = all(info.page_count);
    let res = engine.save_with(info.doc_id, order.clone(), rot.clone(), None, SaveOptions { protect: Some(Protection { remove: true, ..Default::default() }), ..Default::default() });
    check("removing needs the owner", res.is_err(), res.err().unwrap_or_default());
    let wrong = engine.check_owner(info.doc_id, "open2").unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    // A page not read yet: its content can only be read if the keys survived the wrong try.
    let content = d.content_box(700);
    check("wrong owner password refused, file still readable", !wrong && matches!(content, Ok(Some(_))), format!("{content:?}"));
    check("owner password accepted", engine.check_owner(info.doc_id, "own1").unwrap() && engine.security(info.doc_id).unwrap().owner, String::new());
    let saved = engine.save_with(info.doc_id, order, rot, None, SaveOptions { protect: Some(Protection { remove: true, ..Default::default() }), ..Default::default() }).unwrap();
    engine.close(saved.doc_id);
    let info = engine.open(&p).unwrap();
    let s = engine.security(info.doc_id).unwrap();
    check("protection removed", !info.needs_password && s.method.is_empty() && info.page_count == 1000, String::new());
    engine.close(info.doc_id);

    // Compress a scan to a copy, at each level.
    let src = format!("{dir}/scan-300.pdf");
    let before = size(&src);
    let original = thumb(&engine, &src);
    for level in 0..3u8 {
        let p = format!("{tmp}/scan.pdf");
        std::fs::copy(&src, &p).unwrap();
        let info = engine.open(&p).unwrap();
        let (order, rot) = all(info.page_count);
        let out = format!("{tmp}/scan-{level}.pdf");
        let t = std::time::Instant::now();
        let saved = engine.save_with(info.doc_id, order, rot, Some(out.clone()), SaveOptions { compress: Some(level), ..Default::default() });
        let after = size(&out);
        let ok = saved.as_ref().map(|s| s.page_count == info.page_count).unwrap_or(false) && after > 0 && (level == 0 || after < before);
        check(&format!("compress level {level}"), ok, format!("{} KB -> {} KB in {} ms {:?}", before / 1024, after / 1024, t.elapsed().as_millis(), saved.err()));
        let t = thumb(&engine, &out);
        let delta = diff(&original, &t);
        check(&format!("level {level} looks the same"), delta < 6.0, format!("mean difference {delta:.2}"));
    }

    // A colour newspaper: photos stay photos.
    let src = format!("{dir}/Maharashtra Times.pdf");
    let original = thumb(&engine, &src);
    let p = format!("{tmp}/paper.pdf");
    std::fs::copy(&src, &p).unwrap();
    let info = engine.open(&p).unwrap();
    let (order, rot) = all(info.page_count);
    let out = format!("{tmp}/paper-1.pdf");
    let saved = engine.save_with(info.doc_id, order, rot, Some(out.clone()), SaveOptions { compress: Some(1), ..Default::default() });
    let delta = diff(&original, &thumb(&engine, &out));
    check("colour pages compressed", saved.is_ok() && delta < 6.0, format!("{} KB -> {} KB, mean difference {delta:.2}", size(&src) / 1024, size(&out) / 1024));

    // Repair: a damaged file is rewritten cleanly.
    let p = format!("{tmp}/broken.pdf");
    std::fs::copy(format!("{dir}/broken.pdf"), &p).unwrap();
    let info = engine.open(&p).unwrap();
    let s = engine.security(info.doc_id).unwrap();
    check("damage noticed", s.repaired, String::new());
    // As the app does on opening: forms, labels, bookmarks, page sizes.
    let d = engine.doc(info.doc_id).unwrap();
    let _ = d.forms_init();
    let _ = engine.page_labels(info.doc_id);
    let _ = d.outline_json();
    let _ = engine.page_sizes(info.doc_id);
    let (order, rot) = all(info.page_count);
    let saved = engine.save_with(info.doc_id, order, rot, None, SaveOptions { rewrite: true, ..Default::default() }).unwrap();
    let sizes = engine.page_sizes(saved.doc_id);
    check("repaired", !engine.security(saved.doc_id).unwrap().repaired && saved.page_count > 700 && saved.page_count < info.page_count && sizes.is_ok(), format!("{} pages {:?}", saved.page_count, sizes.err()));
    engine.close(saved.doc_id);

    // The same file saved the ordinary way.
    std::fs::copy(format!("{dir}/broken.pdf"), &p).unwrap();
    let info = engine.open(&p).unwrap();
    let (order, rot) = all(info.page_count);
    let saved = engine.save(info.doc_id, order, rot, None).unwrap();
    let sizes = engine.page_sizes(saved.doc_id);
    check("plain save of a damaged file", sizes.is_ok(), format!("{:?}", sizes.err()));
    engine.close(saved.doc_id);

    println!("{}", if failures == 0 { "ALL PASSED".to_string() } else { format!("{failures} FAILED") });
    std::process::exit(failures);
}
