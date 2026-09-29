//! Annotation check: cargo run --example annot_test -- <file.pdf> <scratch-dir>
//! Adds each kind of annotation, changes and deletes some, undoes, saves and reopens.
use paginaut_lib::engine::{mupdf::AnnotEdit, Engine};
use serde_json::Value;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::Path::new(&args[2]);
    std::fs::create_dir_all(dir).unwrap();
    let copy = dir.join("annot-copy.pdf");
    std::fs::copy(&args[1], &copy).unwrap();
    let engine = Engine::new(4, 64 << 20).unwrap();
    let info = engine.open(copy.to_str().unwrap()).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();
    let red = Some(vec![1.0, 0.0, 0.0]);
    let yellow = Some(vec![1.0, 0.9, 0.0]);
    let mk = |kind: &str, e: AnnotEdit| d.annot_edit(0, None, Some(kind), e, false, "Tester").map(|(n, _)| n);
    let hl = mk("Highlight", AnnotEdit { quads: Some(vec![72.0, 40.0, 300.0, 40.0, 72.0, 60.0, 300.0, 60.0]), color: yellow.clone(), ..Default::default() });
    let ink = mk("Ink", AnnotEdit { ink: Some(vec![vec![80.0, 600.0, 120.0, 620.0, 160.0, 600.0, 200.0, 640.0]]), color: red.clone(), border: Some(2.0), ..Default::default() });
    let ft = mk("FreeText", AnnotEdit { rect: Some([300.0, 600.0, 520.0, 630.0]), font: Some(("Helv".into(), 12.0, vec![0.0, 0.0, 0.8])), contents: Some("Hello from Paginaut".into()), ..Default::default() });
    let sq = mk("Square", AnnotEdit { rect: Some([80.0, 660.0, 200.0, 720.0]), color: red.clone(), border: Some(1.5), ..Default::default() });
    let ln = mk("Line", AnnotEdit { line: Some([250.0, 700.0, 400.0, 740.0]), line_endings: Some([0, 4]), color: red.clone(), border: Some(2.0), ..Default::default() });
    let st = mk("Stamp", AnnotEdit { rect: Some([420.0, 660.0, 560.0, 710.0]), icon: Some("Approved".into()), ..Default::default() });
    let note = mk("Text", AnnotEdit { rect: Some([560.0, 40.0, 580.0, 60.0]), contents: Some("A sticky note".into()), icon: Some("Comment".into()), color: yellow.clone(), ..Default::default() });
    println!("created: hl={hl:?} ink={ink:?} ft={ft:?} sq={sq:?} line={ln:?} stamp={st:?} note={note:?}");
    let list: Vec<Value> = serde_json::from_str(&d.annots_json(0).unwrap()).unwrap();
    for a in &list {
        println!("  {} n={} r={} c={} text={:?}", a["t"], a["n"], a["r"], a["c"], a["text"]);
    }
    // Recolour the square, move the note, delete the line.
    d.annot_edit(0, Some(sq.clone().unwrap()), None, AnnotEdit { color: Some(vec![0.0, 0.4, 1.0]), ..Default::default() }, false, "").unwrap();
    d.annot_edit(0, Some(note.clone().unwrap()), None, AnnotEdit { rect: Some([500.0, 100.0, 520.0, 120.0]), ..Default::default() }, false, "").unwrap();
    d.annot_edit(0, Some(ln.clone().unwrap()), None, AnnotEdit::default(), true, "").unwrap();
    let count = |d: &paginaut_lib::engine::Doc| serde_json::from_str::<Vec<Value>>(&d.annots_json(0).unwrap()).unwrap().len();
    println!("after delete: {}", count(&d));
    d.undo(false).unwrap();
    println!("after undo (line back): {}", count(&d));
    println!("doc list: {}", d.doc_annots_json().unwrap());
    drop(d);
    let n = engine.doc(info.doc_id).unwrap().page_count();
    let saved = engine.save(info.doc_id, (0..n).collect(), vec![0; n], None).unwrap();
    let d2 = engine.doc(saved.doc_id).unwrap();
    let list: Vec<Value> = serde_json::from_str(&d2.annots_json(0).unwrap()).unwrap();
    println!("after save+reopen: {} annotations: {:?}", list.len(), list.iter().map(|a| a["t"].as_str().unwrap().to_string()).collect::<Vec<_>>());
    // Render to check nothing breaks.
    let b = d2.bounds(0).unwrap();
    println!("page bounds {b:?}");
}
