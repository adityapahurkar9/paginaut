//! Pictures, areas and attachments: cargo run --release --example extract_test -- <testfiles dir> <temp dir>
//! Works on copies; never touches the originals.
use paginaut_lib::engine::Engine;

const WITH_FILES: &str = "%PDF-1.7
1 0 obj <</Type/Catalog/Pages 2 0 R/Names<</EmbeddedFiles<</Names[(notes.txt) 4 0 R]>>>>>> endobj
2 0 obj <</Type/Pages/Kids[3 0 R]/Count 1>> endobj
3 0 obj <</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]/Annots[6 0 R]>> endobj
4 0 obj <</Type/Filespec/F(notes.txt)/UF(notes.txt)/Desc(Some notes)/EF<</F 5 0 R>>>> endobj
5 0 obj <</Type/EmbeddedFile/Subtype/text#2Fplain/Length 11/Params<</Size 11>>>> stream
hello world
endstream endobj
6 0 obj <</Type/Annot/Subtype/FileAttachment/Rect[10 10 30 30]/FS 7 0 R>> endobj
7 0 obj <</Type/Filespec/F(run.exe)/EF<</F 8 0 R>>>> endobj
8 0 obj <</Type/EmbeddedFile/Length 2>> stream
MZ
endstream endobj
trailer <</Root 1 0 R/Size 9>>
%%EOF
";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (dir, tmp) = (&args[1], &args[2]);
    let _ = std::fs::remove_dir_all(tmp);
    std::fs::create_dir_all(tmp).unwrap();
    let engine = Engine::new(4, 64 << 20).unwrap();
    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!("{} {name}: {detail}", if ok { "PASS" } else { "FAIL" });
        if !ok {
            failures += 1;
        }
    };

    // Pictures in a scan: the stored JPEG comes out byte for byte.
    let info = engine.open(&format!("{dir}/scan-300.pdf")).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    let pics = d.pictures(0, false).unwrap();
    let p = pics.first();
    check("scan picture found", p.is_some_and(|p| p.w == 2480 && p.h == 3509 && p.k == "jpg"), format!("{:?}", p.map(|p| (p.w, p.h, &p.k, p.r))));
    let (bytes, ext) = d.picture_data(0, 0, 0).unwrap();
    check("stored JPEG as is", ext == "jpg" && bytes.starts_with(&[0xff, 0xd8]) && bytes.len() == p.map_or(0, |p| p.n), format!("{} bytes", bytes.len()));
    let (png, ext) = d.picture_data(0, 0, 1).unwrap();
    check("as PNG", ext == "png" && png.starts_with(b"\x89PNG"), format!("{} KB", png.len() / 1024));
    let (pdf, ext) = d.picture_data(0, 0, 2).unwrap();
    let one = format!("{tmp}/picture.pdf");
    std::fs::write(&one, &pdf).unwrap();
    let o = engine.open(&one).unwrap();
    let size = engine.page_sizes(o.doc_id).unwrap()[0];
    // Stored as it is inside the PDF too: not much bigger than the JPEG.
    check("as a one-page PDF", ext == "pdf" && o.page_count == 1 && (size[0] - 595.0).abs() < 1.0 && pdf.len() < bytes.len() + 4096, format!("{size:?} {} bytes", pdf.len()));
    engine.close(o.doc_id);
    let out = format!("{tmp}/scan-pictures");
    std::fs::create_dir_all(&out).unwrap();
    let names: Vec<String> = (1..=5).map(|i| i.to_string()).collect();
    let r = d.save_pictures(&[0, 1, 2, 3, 4], &names, 100, 0, &out, "scan").unwrap();
    let files = std::fs::read_dir(&out).unwrap().count();
    check("save all (5 pages)", r.saved == 5 && files == 5, format!("saved {} small {} repeats {}", r.saved, r.small, r.repeats));
    engine.close(info.doc_id);

    // A colour newspaper: pictures, with repeats counted once.
    let info = engine.open(&format!("{dir}/Maharashtra Times.pdf")).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    let all: Vec<usize> = (0..info.page_count).collect();
    let names: Vec<String> = all.iter().map(|i| (i + 1).to_string()).collect();
    let out = format!("{tmp}/paper-pictures");
    std::fs::create_dir_all(&out).unwrap();
    let t = std::time::Instant::now();
    let r = d.save_pictures(&all, &names, 64, 0, &out, "paper").unwrap();
    check("newspaper pictures", r.saved > 0, format!("saved {} small {} repeats {} in {} ms", r.saved, r.small, r.repeats, t.elapsed().as_millis()));
    engine.close(info.doc_id);

    // An area of a text page.
    let info = engine.open(&format!("{dir}/text-1000.pdf")).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    let area = [50.0, 40.0, 300.0, 200.0];
    let text = d.area_text(0, area).unwrap();
    check("area text", text.contains("Page 1") || text.len() > 50, text.chars().take(60).collect());
    let png = d.area_data(0, area, 0, "png", 144.0).unwrap();
    let (w, h) = (u32::from_be_bytes(png[16..20].try_into().unwrap()), u32::from_be_bytes(png[20..24].try_into().unwrap()));
    check("area PNG at 144 dpi", png.starts_with(b"\x89PNG") && w == 500 && h == 320, format!("{w} x {h}"));
    let png = d.area_data(0, area, 90, "png", 72.0).unwrap();
    let (w, h) = (u32::from_be_bytes(png[16..20].try_into().unwrap()), u32::from_be_bytes(png[20..24].try_into().unwrap()));
    check("area PNG turned", w == 160 && h == 250, format!("{w} x {h}"));
    let svg = String::from_utf8(d.area_data(0, area, 0, "svg", 72.0).unwrap()).unwrap_or_default();
    check("area SVG", svg.contains("<svg") && svg.contains("<path"), format!("{} KB", svg.len() / 1024));
    let pdf = d.area_data(0, area, 0, "pdf", 72.0).unwrap();
    let one = format!("{tmp}/area.pdf");
    std::fs::write(&one, &pdf).unwrap();
    let o = engine.open(&one).unwrap();
    let size = engine.page_sizes(o.doc_id).unwrap()[0];
    let od = engine.doc(o.doc_id).unwrap();
    let inside = od.area_text(0, [0.0, 0.0, size[0], size[1]]).unwrap();
    check("area PDF (vector, text kept)", o.page_count == 1 && (size[0] - 250.0).abs() < 1.0 && (size[1] - 160.0).abs() < 1.0 && inside.lines().take(3).eq(text.lines().take(3)), format!("{size:?} {} KB {:?} vs {:?}", pdf.len() / 1024, inside.chars().take(80).collect::<String>(), text.chars().take(80).collect::<String>()));
    engine.close(o.doc_id);
    engine.close(info.doc_id);

    // Attachments: one on the document, one pinned to the page.
    let p = format!("{tmp}/files.pdf");
    std::fs::write(&p, WITH_FILES).unwrap();
    let info = engine.open(&p).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    let list = d.attachments().unwrap();
    check(
        "attachments listed",
        list.len() == 2 && list[0].name == "notes.txt" && list[0].page == -1 && list[0].desc == "Some notes" && list[1].name == "run.exe" && list[1].page == 0,
        format!("{:?}", list.iter().map(|a| (&a.name, a.page, a.size)).collect::<Vec<_>>()),
    );
    check("attachment contents", d.attachment_data(0).unwrap() == b"hello world", String::new());

    // Attaching and removing.
    d.forms_init().unwrap();
    let names = |d: &paginaut_lib::engine::Doc| d.attachments().unwrap().iter().map(|a| a.name.clone()).collect::<Vec<_>>();
    d.attachment_add(&format!("{dir}/photo.png")).unwrap();
    d.attachment_add(&format!("{dir}/photo.png")).unwrap();
    let n = names(&d);
    check("attached twice", n.len() == 4 && n.iter().filter(|x| *x == "photo.png").count() == 2, format!("{n:?}"));
    let photo = std::fs::read(format!("{dir}/photo.png")).unwrap();
    let i = d.attachments().unwrap().iter().position(|a| a.name == "photo.png").unwrap();
    check("attached bytes", d.attachment_data(i).unwrap() == photo, String::new());
    d.attachment_remove(0).unwrap();
    let n = names(&d);
    check("document attachment removed", !n.contains(&"notes.txt".to_string()) && n.len() == 3, format!("{n:?}"));
    let pinned = d.attachments().unwrap().iter().position(|a| a.page == 0).unwrap();
    d.attachment_remove(pinned).unwrap();
    let n = names(&d);
    check("page attachment removed", !n.contains(&"run.exe".to_string()) && n.len() == 2, format!("{n:?}"));
    d.undo(false).unwrap();
    check("undo brings it back", names(&d).contains(&"run.exe".to_string()), String::new());
    d.undo(true).unwrap();
    let saved = engine.save(info.doc_id, vec![0], vec![0], None).unwrap();
    let n = names(&engine.doc(saved.doc_id).unwrap());
    check("after saving", n == ["photo.png", "photo.png"], format!("{n:?}"));
    engine.close(saved.doc_id);
    let info = engine.open(&format!("{dir}/text-1000.pdf")).unwrap();
    check("no attachments", engine.doc(info.doc_id).unwrap().attachments().unwrap().is_empty(), String::new());

    // Tables in a prospectus: found with their cells, saved as CSV and Excel.
    let info = engine.open(&format!("{dir}/nse rhp.pdf")).unwrap();
    let d = engine.doc(info.doc_id).unwrap();
    let t = std::time::Instant::now();
    let mut best: Option<(usize, paginaut_lib::engine::table::Table)> = None;
    let mut found = 0;
    for i in 0..info.page_count.min(60) {
        for tb in d.tables(i).unwrap_or_default() {
            found += 1;
            if best.as_ref().map_or(true, |(_, b)| tb.cells.len() > b.cells.len()) {
                best = Some((i, tb));
            }
        }
    }
    println!("  {found} tables on the first 60 pages in {} ms", t.elapsed().as_millis());
    if let Some((page, tb)) = &best {
        let grid = tb.grid();
        println!("  page {}: {} x {}; first rows: {:?}", page + 1, tb.rows(), tb.cols(), grid.iter().take(4).map(|r| r.iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>()).collect::<Vec<_>>());
        std::fs::write(format!("{tmp}/table.csv"), tb.csv()).unwrap();
        std::fs::write(format!("{tmp}/table.xlsx"), tb.xlsx()).unwrap();
        // Reading the same area again, and with the column lines given.
        let again = d.table_in(*page, tb.r, &[], &[]).unwrap();
        check("table read from its area", again.as_ref().is_some_and(|a| a.cols() == tb.cols()), format!("{:?}", again.as_ref().map(|a| (a.rows(), a.cols()))));
        let mid = (tb.xs[0] + tb.xs[tb.xs.len() - 1]) / 2.0;
        let two = d.table_in(*page, tb.r, &[tb.xs[0], mid, tb.xs[tb.xs.len() - 1]], &[]).unwrap();
        check("column lines as given", two.as_ref().is_some_and(|a| a.cols() == 2), format!("{:?}", two.as_ref().map(|a| (a.rows(), a.cols()))));
        // Row lines too: the first three rows joined into one, the rest as found.
        let mut rows = vec![tb.ys[0]];
        rows.extend_from_slice(&tb.ys[3..]);
        let fewer = d.table_in(*page, tb.r, &tb.xs, &rows).unwrap();
        check(
            "row lines as given",
            fewer.as_ref().is_some_and(|a| a.rows() == tb.rows() - 2 && a.ys == rows),
            format!("{:?} of {}", fewer.as_ref().map(|a| (a.rows(), a.cols())), tb.rows()),
        );
    }
    check("tables found", found > 0 && best.is_some(), format!("{found}"));
    engine.close(info.doc_id);

    println!("{}", if failures == 0 { "ALL PASSED".to_string() } else { format!("{failures} FAILED") });
    std::process::exit(failures);
}
