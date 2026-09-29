//! Form filling check: cargo run --example form_test -- <form.pdf> <scratch-dir>
//! Fills fields, checks the calculated total, toggles, undoes, saves and reopens.
use paginaut_lib::engine::Engine;
use serde_json::Value;

fn field<'a>(ws: &'a [Value], name: &str) -> &'a Value {
    ws.iter().find(|w| w["name"] == name).unwrap_or_else(|| panic!("no field {name}"))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = std::path::Path::new(&args[2]);
    std::fs::create_dir_all(dir).unwrap();
    let copy = dir.join("form-copy.pdf");
    std::fs::copy(&args[1], &copy).unwrap();
    let engine = Engine::new(4, 64 << 20).unwrap();
    let info = engine.open(copy.to_str().unwrap()).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    let fi = d.forms_init().unwrap();
    println!("fields: {} xfa: {}", fi.fields, fi.xfa);
    let ws = |d: &paginaut_lib::engine::Doc| -> Vec<Value> { serde_json::from_str(&d.widgets_json(0).unwrap()).unwrap() };
    let w = ws(&d);
    for x in &w {
        println!("  {:10} {:9} v={:?} on={} ff={} fs={} r={}", x["name"].as_str().unwrap(), x["t"].as_str().unwrap(), x["v"], x["on"], x["ff"], x["fs"], x["r"]);
    }
    let num = |w: &[Value], name: &str| field(w, name)["n"].as_i64().unwrap() as i32;
    println!("set qty 3: {:?}", d.widget_set(0, num(&w, "qty"), "3").map(|c| c.accepted));
    println!("set price 12.5: {:?}", d.widget_set(0, num(&w, "price"), "12.5").map(|c| c.accepted));
    println!("set qty abc (should be rejected): {:?}", d.widget_set(0, num(&w, "qty"), "abc").map(|c| c.accepted));
    println!("set dob: {:?}", d.widget_set(0, num(&w, "dob"), "05/03/1990").map(|c| c.accepted));
    println!("set name: {:?}", d.widget_set(0, num(&w, "name"), "Asha Rao").map(|c| c.accepted));
    println!("set country: {:?}", d.widget_set(0, num(&w, "country"), "Singapore").map(|c| c.accepted));
    let w = ws(&d);
    println!("total = {:?}, qty = {:?}, dob = {:?}", field(&w, "total")["v"], field(&w, "qty")["v"], field(&w, "dob")["v"]);
    println!("toggle agree: {:?}", d.widget_toggle(0, num(&w, "agree")).map(|c| c.changed));
    let female = w.iter().find(|x| x["name"] == "gender").map(|_| ()).is_some();
    let radios: Vec<i32> = w.iter().filter(|x| x["t"] == "radio").map(|x| x["n"].as_i64().unwrap() as i32).collect();
    println!("radio widgets: {radios:?} {female}");
    d.widget_toggle(0, radios[1]).unwrap();
    let w = ws(&d);
    println!("agree on={} radios on={:?}", field(&w, "agree")["on"], w.iter().filter(|x| x["t"] == "radio").map(|x| x["on"].clone()).collect::<Vec<_>>());
    println!("undo: {:?}", d.undo(false).map(|c| c.accepted));
    let cn = num(&w, "comments");
    d.widget_set(0, cn, "hello").unwrap();
    println!("comments set: {:?}", field(&ws(&d), "comments")["v"]);
    d.undo(false).unwrap();
    println!("comments after undo: {:?}", field(&ws(&d), "comments")["v"]);
    d.undo(true).unwrap();
    println!("comments after redo: {:?}", field(&ws(&d), "comments")["v"]);
    let w = ws(&d);
    println!("after undo radios on={:?}", w.iter().filter(|x| x["t"] == "radio").map(|x| x["on"].clone()).collect::<Vec<_>>());
    println!("redo: {:?}", d.undo(true).map(|c| c.accepted));
    drop(d);
    let n = engine.doc(info.doc_id).unwrap().page_count();
    let saved = engine.save(info.doc_id, (0..n).collect(), vec![0; n], None).unwrap();
    let d2 = engine.doc(saved.doc_id).unwrap();
    let w = ws(&d2);
    println!("after save+reopen: name={:?} total={:?} agree on={} country={:?}", field(&w, "name")["v"], field(&w, "total")["v"], field(&w, "agree")["on"], field(&w, "country")["v"]);
    let flat = dir.join("form-flat.pdf");
    engine.save_flattened(saved.doc_id, flat.to_str().unwrap().into(), (0..n).collect(), vec![0; n], false).unwrap();
    let f = engine.open(flat.to_str().unwrap()).unwrap();
    let fd = engine.doc(f.doc_id).unwrap();
    println!("flattened: fields={} widgets on page={}", fd.forms_init().unwrap().fields, ws(&fd).len());
}
