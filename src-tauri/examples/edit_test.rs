//! Editing text: cargo run --release --example edit_test -- <testfiles dir> <temp dir>
//! Works on copies; never touches the originals.
use paginaut_lib::engine::{Engine, TextEdit};

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
    let p = format!("{tmp}/text.pdf");
    std::fs::copy(format!("{dir}/text-1000.pdf"), &p).unwrap();
    let info = engine.open(&p).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();
    let [w, h] = engine.page_sizes(info.doc_id).unwrap()[0];
    let page_text = |d: &paginaut_lib::engine::Doc| d.area_text(0, [0.0, 0.0, w, h]).unwrap();

    let paras = d.paragraphs(0).unwrap();
    println!("  {} paragraphs; first: {:?} {} pt {} lines, font {}, align {}", paras.len(), paras[0].t.chars().take(30).collect::<String>(), paras[0].s, paras[0].l.len(), paras[0].font, paras[0].a);
    let body = paras.iter().max_by_key(|p| p.l.len()).unwrap().clone();
    println!("  body: {} lines, {:.1} pt, lead {:.1}, font {}, starts {:?}", body.l.len(), body.s, body.lead, body.font, body.t.chars().take(40).collect::<String>());
    check("paragraphs found", paras.len() >= 2 && body.l.len() > 3 && !body.t.is_empty(), String::new());

    // Replace the body with shorter text.
    let new = "Edited by Paginaut: this paragraph was replaced. The quick brown fox jumps over the lazy dog, again and again, until the line wraps.";
    let old_start: String = body.t.split_whitespace().take(6).collect::<Vec<_>>().join(" ");
    let t0 = std::time::Instant::now();
    d.edit_text(0, &TextEdit { para: body.clone(), text: new.into() }).unwrap();
    let ms = t0.elapsed().as_millis();
    let after = page_text(&d);
    let flat = after.split_whitespace().collect::<Vec<_>>().join(" ");
    check("new text there", flat.contains("Edited by Paginaut: this paragraph was replaced."), format!("{ms} ms"));
    // The paragraph's own box now holds only the new text (the lorem text repeats elsewhere on the page).
    let inside = d.area_text(0, body.r).unwrap();
    check("old text gone", !inside.contains("consectetur"), inside.chars().take(60).collect());
    check("heading kept", flat.contains("Page 1"), String::new());
    let paras2 = d.paragraphs(0).unwrap();
    let edited = paras2.iter().find(|p| p.t.contains("Edited by Paginaut"));
    if let Some(e) = edited {
        let (b0, b1) = (body.l[0][4], e.l[0][4]);
        check("first baseline kept", (b0 - b1).abs() < 0.6, format!("{b0:.2} -> {b1:.2}"));
        check("same left edge", (body.l[0][0] - e.l[0][0]).abs() < 1.0, format!("{:.2} -> {:.2}; old {:?} {:?} / new {:?} {:?} {:?} align {}", body.l[0][0], e.l[0][0], body.l[0], body.r, e.l[0], e.r, e.t.chars().take(30).collect::<String>(), body.a));
        check("same size", (body.s - e.s).abs() < 0.1, format!("{} -> {}", body.s, e.s));
        let fam = |f: &str| f.to_lowercase().replace(['-', ' '], "").contains("times") || f.to_lowercase().contains("nimbusrom");
        check("same font family", fam(&body.font) == fam(&e.font), format!("{} -> {}", body.font, e.font));
        check("wraps within the box", e.r[2] <= body.r[2] + body.s, format!("{:.1} vs {:.1}", e.r[2], body.r[2]));
    } else {
        check("edited paragraph found", false, String::new());
    }

    // Undo brings the old text back; redo the edit.
    d.undo(false).unwrap();
    let flat = page_text(&d).split_whitespace().collect::<Vec<_>>().join(" ");
    check("undo", flat.contains(&old_start) && !flat.contains("Edited by Paginaut"), String::new());
    d.undo(true).unwrap();

    // Marathi in a Latin paragraph: letters the font lacks come from a fallback font.
    let heading = d.paragraphs(0).unwrap().into_iter().find(|p| p.t.contains("Page 1")).unwrap();
    d.edit_text(0, &TextEdit { para: heading, text: "पान १ – Page one".into() }).unwrap();
    let flat = page_text(&d).split_whitespace().collect::<Vec<_>>().join(" ");
    check("Marathi heading", flat.contains("पान १ – Page one"), flat.chars().take(40).collect());

    // Saved and reopened.
    let n = info.page_count;
    let saved = engine.save(info.doc_id, (0..n).collect(), vec![0; n], None).unwrap();
    let d = engine.doc(saved.doc_id).unwrap();
    let flat = page_text(&d).split_whitespace().collect::<Vec<_>>().join(" ");
    check("after saving", flat.contains("Edited by Paginaut") && flat.contains("पान १"), String::new());
    check("other pages untouched", d.area_text(1, [0.0, 0.0, w, h]).unwrap().contains("Page 2"), String::new());
    engine.close(saved.doc_id);

    println!("{}", if failures == 0 { "ALL PASSED".to_string() } else { format!("{failures} FAILED") });
    std::process::exit(failures);
}
