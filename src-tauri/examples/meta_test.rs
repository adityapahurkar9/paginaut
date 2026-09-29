//! Bookmarks, page labels, properties: cargo run --release --example meta_test -- <testfiles dir> <temp dir>
//! Works on copies; never touches the originals.
use paginaut_lib::engine::mupdf::{LabelRange, OutlineNode};
use paginaut_lib::engine::{Engine, Properties};

fn node(t: &str, p: i32, k: Vec<OutlineNode>) -> OutlineNode {
    OutlineNode { t: t.into(), p, y: 0.0, k }
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
    let p = format!("{tmp}/text.pdf");
    std::fs::copy(format!("{dir}/text-1000.pdf"), &p).unwrap();
    let info = engine.open(&p).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    d.forms_init().unwrap();

    // Bookmarks.
    let tree = vec![node("Part one", 0, vec![node("Chapter 1", 1, vec![]), node("Chapter 2", 9, vec![])]), node("Part two", 500, vec![]), node("मराठी", 999, vec![])];
    d.set_outline(&tree).unwrap();
    let out = d.outline_json().unwrap();
    check("bookmarks written", out.contains("\"Chapter 2\",\"p\":9") && out.contains("\"Part two\",\"p\":500") && out.contains("मराठी"), out.chars().take(220).collect());
    d.undo(false).unwrap();
    check("undo bookmarks", d.outline_json().unwrap() == "[]", String::new());
    d.undo(true).unwrap();

    // Page labels.
    let ranges = vec![
        LabelRange { s: 0, st: "r".into(), p: String::new(), n: 1 },
        LabelRange { s: 4, st: "D".into(), p: String::new(), n: 1 },
        LabelRange { s: 990, st: "D".into(), p: "A-".into(), n: 1 },
    ];
    d.set_page_labels(&ranges).unwrap();
    let labels = engine.page_labels(info.doc_id).unwrap().unwrap_or_default();
    check("page labels", labels.get(0).map(String::as_str) == Some("i") && labels.get(4).map(String::as_str) == Some("1") && labels.get(991).map(String::as_str) == Some("A-2"), format!("{:?} … {:?}", &labels[..6.min(labels.len())], labels.get(991)));
    let back = d.page_label_ranges().unwrap();
    check("label ranges read back", back.len() == 3 && back[2].p == "A-" && back[1].st == "D", String::new());

    // Properties: an empty title on a file with no Info dictionary yet is fine.
    let blank = d.set_properties(&Properties { author: "x".into(), ..Default::default() });
    check("empty title, no Info yet", blank.is_ok() && d.properties().author == "x", format!("{:?}", blank.err()));
    // Properties.
    d.set_properties(&Properties { title: "Test title".into(), author: "आदित्य".into(), subject: "S".into(), keywords: "a, b".into(), ..Default::default() }).unwrap();
    let pr = d.properties();
    check("properties", pr.title == "Test title" && pr.author == "आदित्य" && pr.keywords == "a, b", format!("{} / {}", pr.title, pr.author));

    // All of it survives saving (in place) and reopening.
    let n = info.page_count;
    let saved = engine.save(info.doc_id, (0..n).collect(), vec![0; n], None).unwrap();
    let d = engine.doc(saved.doc_id).unwrap();
    let out = d.outline_json().unwrap();
    let labels = engine.page_labels(saved.doc_id).unwrap().unwrap_or_default();
    check("after reopening", out.contains("Chapter 2") && labels.get(2).map(String::as_str) == Some("iii") && d.properties().author == "आदित्य" && saved.title == "Test title", String::new());
    engine.close(saved.doc_id);

    println!("{}", if failures == 0 { "ALL PASSED".to_string() } else { format!("{failures} FAILED") });
    std::process::exit(failures);
}
