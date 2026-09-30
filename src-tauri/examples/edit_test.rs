//! Editing text: cargo run --release --example edit_test -- <testfiles dir> <temp dir>
//! Works on copies; never touches the originals.
use paginaut_lib::engine::mupdf::StampPart;
use paginaut_lib::engine::{Engine, StampPage, TextEdit};

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
    d.edit_text(0, &TextEdit { para: body.clone(), text: new.into(), runs: None }).unwrap();
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
    d.edit_text(0, &TextEdit { para: heading, text: "पान १ – Page one".into(), runs: None }).unwrap();
    let flat = page_text(&d).split_whitespace().collect::<Vec<_>>().join(" ");
    check("Marathi heading", flat.contains("पान १ – Page one"), flat.chars().take(40).collect());

    // Bold and italic words within a paragraph: read back as runs of those styles.
    let para = d.paragraphs(0).unwrap().into_iter().filter(|p| p.t.len() > 80 && !p.t.contains("Edited by Paginaut")).last().unwrap();
    let runs: Vec<(String, u8)> = vec![("Plain words, then ".into(), 0), ("bold ones".into(), 1), (", then ".into(), 0), ("italic ones".into(), 2), (" and both.".into(), 3)];
    let text: String = runs.iter().map(|r| r.0.as_str()).collect();
    d.edit_text(0, &TextEdit { para, text: text.clone(), runs: Some(runs) }).unwrap();
    let back = d.paragraphs(0).unwrap().into_iter().find(|p| p.t.contains("Plain words")).unwrap();
    let style_of = |word: &str| back.u.iter().find(|(t, _)| t.contains(word)).map(|r| r.1);
    check(
        "bold and italic runs",
        back.t == text && style_of("Plain") == Some(0) && style_of("bold") == Some(1) && style_of("italic") == Some(2) && style_of("both") == Some(3),
        format!("{:?}", back.u),
    );

    // Pictures on page 3, which also has a page number stamped on it.
    let footer = StampPart {
        text: Some("Stamped footer 3".into()),
        image: None,
        font: Some("sans-serif".into()),
        size: 9.0,
        color: [0.2, 0.2, 0.2],
        bold: false,
        align: Some("center".into()),
        width: w - 72.0,
        height: 0.0,
        valign: 2,
        matrix: [1.0, 0.0, 0.0, 1.0, 36.0, h - 24.0],
        opacity: 1.0,
    };
    d.stamp_pages(&[StampPage { page: 2, parts: vec![footer] }], "Header", false, false).unwrap();
    let photo = std::fs::read(format!("{dir}/photo.png")).unwrap();
    let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1.0);
    let own = |d: &paginaut_lib::engine::Doc| d.pictures(2, true).unwrap();
    let none = [0.0f32; 4];
    d.picture_edit(2, none, 3, [100.0, 100.0, 300.0, 250.0], &photo).unwrap();
    let pics = own(&d);
    check("picture added", pics.len() == 1 && near(pics[0].r, [100.0, 100.0, 300.0, 250.0]), format!("{:?}", pics.iter().map(|p| p.r).collect::<Vec<_>>()));
    d.picture_edit(2, pics[0].r, 1, [200.0, 300.0, 300.0, 375.0], &[]).unwrap();
    let pics = own(&d);
    check("picture moved and resized", pics.len() == 1 && near(pics[0].r, [200.0, 300.0, 300.0, 375.0]), format!("{:?}", pics.iter().map(|p| p.r).collect::<Vec<_>>()));
    d.picture_edit(2, pics[0].r, 2, [50.0, 50.0, 150.0, 125.0], &photo).unwrap();
    let pics = own(&d);
    check("picture replaced", pics.len() == 1 && near(pics[0].r, [50.0, 50.0, 150.0, 125.0]), format!("{:?}", pics.iter().map(|p| p.r).collect::<Vec<_>>()));
    if pics.is_empty() {
        let svg = String::from_utf8(d.area_data(2, [0.0, 0.0, w, h], 0, "svg", 72.0).unwrap()).unwrap();
        println!("  all pictures: {:?}
  svg images: {}", d.pictures(2, false).unwrap().iter().map(|p| p.r).collect::<Vec<_>>(), svg.matches("<image").count());
        for (i, _) in svg.match_indices("<image") { println!("  {}", &svg[i..(i + 160).min(svg.len())]); }
        std::process::exit(1);
    }
    // A text edit on the same page too, then the stamp must still come off.
    let para = d.paragraphs(2).unwrap().into_iter().max_by_key(|p| p.l.len()).unwrap();
    d.edit_text(2, &TextEdit { para, text: "Page three, edited.".into(), runs: None }).unwrap();
    d.picture_edit(2, pics[0].r, 0, none, &[]).unwrap();
    check("picture deleted", own(&d).is_empty(), String::new());
    d.undo(false).unwrap();
    check("delete undone", own(&d).len() == 1, String::new());
    d.undo(true).unwrap();
    let t3 = d.area_text(2, [0.0, 0.0, w, h]).unwrap();
    check("stamp kept through edits", t3.contains("Stamped footer 3") && t3.contains("Page three, edited."), format!("{:?}", t3.lines().filter(|l| l.contains("Stamped") || l.contains("three") || l.contains("Page")).collect::<Vec<_>>()));
    let removed = d.remove_stamps("Header", Some(vec![2])).unwrap();
    let t3 = d.area_text(2, [0.0, 0.0, w, h]).unwrap();
    check("stamp still removable", removed >= 1 && !t3.contains("Stamped footer 3") && t3.contains("Page three, edited."), format!("{removed} removed"));
    let bad = d.picture_edit(2, [1.0, 1.0, 2.0, 2.0], 0, none, &[]);
    check("no picture there: an error, page kept", bad.is_err() && d.area_text(2, [0.0, 0.0, w, h]).unwrap().contains("Page three"), format!("{:?}", bad.err()));

    // A moved picture keeps its place in what the page draws: still under red text stamped over it.
    d.picture_edit(1, none, 3, [100.0, 400.0, 250.0, 520.0], &photo).unwrap();
    let cover = StampPart {
        text: Some("COVER COVER".into()),
        image: None,
        font: Some("sans-serif".into()),
        size: 30.0,
        color: [1.0, 0.0, 0.0],
        bold: false,
        align: Some("left".into()),
        width: 300.0,
        height: 0.0,
        valign: 0,
        matrix: [1.0, 0.0, 0.0, 1.0, 90.0, 440.0],
        opacity: 1.0,
    };
    d.stamp_pages(&[StampPage { page: 1, parts: vec![cover] }], "Watermark", false, false).unwrap();
    let order = |d: &paginaut_lib::engine::Doc| {
        let svg = String::from_utf8(d.area_data(1, [0.0, 0.0, w, h], 0, "svg", 72.0).unwrap()).unwrap();
        (svg.find("<image"), svg.rfind("#ff0000"))
    };
    let pic = d.pictures(1, true).unwrap().into_iter().find(|p| near(p.r, [100.0, 400.0, 250.0, 520.0])).map(|p| p.r).unwrap_or(none);
    d.picture_edit(1, pic, 1, [120.0, 420.0, 240.0, 510.0], &[]).unwrap();
    let (img, red) = order(&d);
    let moved = d.pictures(1, true).unwrap().iter().any(|p| near(p.r, [120.0, 420.0, 240.0, 510.0]));
    check("moved picture stays under what covered it", moved && img.is_some() && red.is_some() && img < red, format!("picture at {img:?}, red text at {red:?}, moved {moved}"));
    d.picture_edit(1, [120.0, 420.0, 240.0, 510.0], 2, [130.0, 430.0, 230.0, 505.0], &photo).unwrap();
    let (img, red) = order(&d);
    check("replaced picture too", img.is_some() && img < red, format!("picture at {img:?}, red text at {red:?}"));
    std::fs::write(format!("{tmp}/picture-order.png"), d.area_data(1, [60.0, 380.0, 400.0, 540.0], 0, "png", 110.0).unwrap()).unwrap();
    d.undo(false).unwrap();
    d.undo(false).unwrap();
    check("moves undo", d.pictures(1, true).unwrap().iter().any(|p| near(p.r, [100.0, 400.0, 250.0, 520.0])), String::new());

    // A table row the text finder took for one block: each cell edits on its own, figures set right.
    {
        let rhp = format!("{tmp}/table-edit.pdf");
        std::fs::copy(format!("{dir}/nse rhp.pdf"), &rhp).unwrap();
        let t = engine.open(&rhp).unwrap();
        let td = engine.doc(t.doc_id).unwrap();
        td.forms_init().unwrap();
        let paras = td.paragraphs(32).unwrap();
        let cell = paras.iter().find(|p| p.t == "36,226.64").cloned();
        check("table row split into cells", cell.as_ref().is_some_and(|c| c.a == "r" && c.l.len() == 1), format!("{:?}", cell.as_ref().map(|c| (&c.t, &c.a, c.l.len()))));
        if let Some(cell) = cell {
            let area = [cell.r[0] - 60.0, cell.r[1] - 2.0, 560.0, cell.r[3] + 2.0];
            td.edit_text(32, &TextEdit { para: cell, text: "36,300.00".into(), runs: None }).unwrap();
            let row = td.area_text(32, area).unwrap().split_whitespace().collect::<Vec<_>>().join(" ");
            std::fs::write(format!("{tmp}/table-cell.png"), td.area_data(32, [50.0, 105.0, 330.0, 160.0], 0, "png", 200.0).unwrap()).unwrap();
            check("one cell edited, the row kept", row.contains("36,300.00") && !row.contains("36,226.64") && row.contains("79.44%") && row.contains("121,296.37"), row);
        }
        engine.close(t.doc_id);
    }

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
