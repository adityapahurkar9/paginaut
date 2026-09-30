//! Paginaut's own Word (.docx) writer, for when Microsoft Word isn't here: the pages' text as paragraphs (size,
//! bold, italic, colour, alignment, font), tables found on them as Word tables, and their pictures, in reading
//! order, a page break between pages. The flow of the text, not the page layout: easy to edit.
//! (MuPDF's own .docx writer makes files Word refuses to open; LibreOffice's loses text under shading.)

use crate::engine::table::{xml, zip, Table};
use crate::engine::{Doc, Paragraph, Picture};

enum Block {
    Para(Paragraph),
    Table(Table),
    /// Picture bytes, their extension, and size in points.
    Picture(Vec<u8>, &'static str, f32, f32),
}

/// What's on page `index`, in reading order: paragraphs as MuPDF reads them, each table where its first
/// paragraph was (the paragraphs inside it left out), each picture before the first paragraph below its top.
fn blocks(d: &Doc, index: usize) -> Result<Vec<Block>, String> {
    let b = d.bounds(index)?;
    let height = b[3] - b[1];
    // Page numbers at the top or bottom of the page mean nothing in a flowing document.
    let page_number = |p: &Paragraph| {
        let t = p.t.trim().to_lowercase();
        let t = t.strip_prefix("page").unwrap_or(&t).trim();
        let numeric = !t.is_empty() && t.split(['/', ' ']).filter(|w| *w != "of" && !w.is_empty()).all(|w| w.chars().all(|c| c.is_ascii_digit() || "ivxlc".contains(c)));
        numeric && (p.r[1] > height * 0.88 || p.r[3] < height * 0.12)
    };
    let paras: Vec<Paragraph> = d.paragraphs(index)?.into_iter().filter(|p| !page_number(p)).collect();
    let mut tables: Vec<Option<Table>> = d.tables(index)?.into_iter().map(Some).collect();
    let rects: Vec<[f32; 4]> = tables.iter().flatten().map(|t| t.r).collect();
    let inside = |r: &[f32; 4], t: &[f32; 4]| {
        let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
        cx >= t[0] && cx <= t[2] && cy >= t[1] && cy <= t[3]
    };
    let mut pictures: Vec<(f32, Block)> = Vec::new();
    // Numbered as picture_data counts them (the page with its comments' pictures).
    for (n, p) in d.pictures(index, false)?.into_iter().enumerate() {
        let Picture { r, w, h, .. } = p;
        // Rules, dots and bullets drawn as pictures aren't worth keeping.
        if w < 24 || h < 24 || (r[2] - r[0]) < 12.0 || (r[3] - r[1]) < 12.0 {
            continue;
        }
        let (mut data, mut ext) = d.picture_data(index, n, 0)?;
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
            (data, ext) = d.picture_data(index, n, 1)?; // as PNG
        }
        let ext = if ext == "png" { "png" } else { "jpeg" };
        pictures.push((r[1], Block::Picture(data, ext, r[2] - r[0], r[3] - r[1])));
    }
    pictures.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut pictures = pictures.into_iter().peekable();
    let mut out = Vec::new();
    for p in paras {
        while pictures.peek().is_some_and(|(top, _)| *top <= p.r[1]) {
            out.push(pictures.next().unwrap().1);
        }
        match rects.iter().position(|t| inside(&p.r, t)) {
            Some(i) => {
                if let Some(t) = tables[i].take() {
                    out.push(Block::Table(t));
                }
            }
            None => out.push(Block::Para(p)),
        }
    }
    out.extend(pictures.map(|p| p.1));
    // Tables with no paragraph found inside them.
    out.extend(tables.into_iter().flatten().map(Block::Table));
    Ok(out)
}

/// "ABCDEF+Aptos-Bold" → "Aptos".
fn family(font: &str) -> String {
    let f = font.split_once('+').map_or(font, |(tag, rest)| if tag.len() == 6 { rest } else { font });
    let f = f.split([',', '-']).next().unwrap_or(f);
    let f = f.trim_end_matches("MT").trim_end_matches("PS");
    // "TimesNewRoman" → "Times New Roman".
    let mut s = String::new();
    for (i, ch) in f.chars().enumerate() {
        if i > 0 && ch.is_ascii_uppercase() && s.chars().last().is_some_and(|p: char| p.is_ascii_lowercase()) {
            s.push(' ');
        }
        s.push(ch);
    }
    s
}

/// Fonts of symbols (bullets, ticks): their letters aren't letters, so a paragraph that starts with one keeps
/// Word's usual font.
fn symbol_font(f: &str) -> bool {
    let f = f.to_ascii_lowercase();
    ["symbol", "wingdings", "webdings", "zapf", "dingbats", "marlett"].iter().any(|s| f.starts_with(s))
}

/// Bullets drawn with symbol fonts arrive as private-use characters (U+F0B7…): a plain bullet instead.
fn clean(text: &str) -> String {
    text.chars().map(|c| if ('\u{f020}'..='\u{f0ff}').contains(&c) { '•' } else { c }).collect()
}

fn run_props(p: &Paragraph) -> String {
    let mut r = String::from("<w:rPr>");
    let f = family(&p.font);
    if !f.is_empty() && !symbol_font(&f) {
        let f = xml(&f);
        r.push_str(&format!(r#"<w:rFonts w:ascii="{f}" w:hAnsi="{f}" w:cs="{f}"/>"#));
    }
    if p.b != 0 {
        r.push_str("<w:b/>");
    }
    if p.i != 0 {
        r.push_str("<w:i/>");
    }
    if p.c.len() == 7 && p.c.starts_with('#') && p.c != "#000000" && p.c[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        r.push_str(&format!(r#"<w:color w:val="{}"/>"#, &p.c[1..]));
    }
    let half = (p.s * 2.0).round().clamp(2.0, 3276.0) as i32;
    r.push_str(&format!(r#"<w:sz w:val="{half}"/><w:szCs w:val="{half}"/></w:rPr>"#));
    r
}

/// The start of a PDF page starts a Word page too (on the paragraph itself: a paragraph holding only a
/// break would be left alone on a page after a full-page picture).
fn page_start(new_page: bool) -> &'static str {
    if new_page {
        "<w:pageBreakBefore/>"
    } else {
        ""
    }
}

fn paragraph(p: &Paragraph, new_page: bool) -> String {
    let jc = match p.a.as_str() {
        "c" => "center",
        "r" => "right",
        "j" => "both",
        _ => "left",
    };
    let text = clean(&p.t.split('\n').map(str::trim_end).collect::<Vec<_>>().join(" "));
    format!(
        r#"<w:p><w:pPr>{}<w:spacing w:after="120"/><w:jc w:val="{jc}"/></w:pPr><w:r>{}<w:t xml:space="preserve">{}</w:t></w:r></w:p>"#,
        page_start(new_page),
        run_props(p),
        xml(text.trim())
    )
}

fn table(t: &Table) -> String {
    let widths: Vec<i32> = t.xs.windows(2).map(|w| ((w[1] - w[0]) * 20.0).round().max(200.0) as i32).collect();
    let border = r#"w:val="single" w:sz="4" w:space="0" w:color="999999""#;
    let mut s = format!(
        r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/><w:tblBorders><w:top {border}/><w:left {border}/><w:bottom {border}/><w:right {border}/><w:insideH {border}/><w:insideV {border}/></w:tblBorders><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid>"#
    );
    for w in &widths {
        s.push_str(&format!(r#"<w:gridCol w:w="{w}"/>"#));
    }
    s.push_str("</w:tblGrid>");
    for row in t.grid() {
        s.push_str("<w:tr>");
        for (c, cell) in row.iter().enumerate() {
            let w = widths.get(c).copied().unwrap_or(1000);
            s.push_str(&format!(r#"<w:tc><w:tcPr><w:tcW w:w="{w}" w:type="dxa"/></w:tcPr><w:p><w:r><w:t xml:space="preserve">{}</w:t></w:r></w:p></w:tc>"#, xml(cell.trim())));
        }
        s.push_str("</w:tr>");
    }
    // A table can't end the document or touch the next table: a paragraph after it.
    s.push_str("</w:tbl><w:p/>");
    s
}

fn picture(n: usize, w: f32, h: f32, max_w: f32, max_h: f32, new_page: bool) -> String {
    // Fits between the margins, with room for a line of text on its page.
    let z = (max_w / w).min((max_h - 30.0).max(36.0) / h).min(1.0);
    let (cx, cy) = ((w * z * 12700.0) as i64, (h * z * 12700.0) as i64);
    format!(
        r#"<w:p><w:pPr>{}</w:pPr><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{n}" name="Picture {n}"/><a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:nvPicPr><pic:cNvPr id="{n}" name="Picture {n}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="img{n}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#,
        page_start(new_page)
    )
}

/// Writes the pages `order` of `d` as a Word document at `path`. Calls `progress` per page; stops when it says so.
pub fn write(d: &Doc, order: &[usize], path: &str, progress: &mut dyn FnMut(usize, usize) -> bool) -> Result<(), String> {
    let first = d.bounds(*order.first().ok_or("no pages")?)?;
    let (pw, ph) = (first[2] - first[0], first[3] - first[1]);
    let margin = 54.0; // ¾ inch
    let (max_w, max_h) = ((pw - 2.0 * margin).max(72.0), (ph - 2.0 * margin).max(72.0));
    let mut body = String::new();
    let mut media: Vec<(String, Vec<u8>)> = Vec::new();
    let mut rels = String::new();
    for (k, &page) in order.iter().enumerate() {
        if !progress(k, order.len()) {
            return Err("stopped".into());
        }
        let mut new_page = k > 0;
        for b in blocks(d, page)? {
            match b {
                Block::Para(p) if !p.t.trim().is_empty() => body.push_str(&paragraph(&p, new_page)),
                Block::Para(_) => continue,
                Block::Table(t) => {
                    if new_page {
                        body.push_str(r#"<w:p><w:pPr><w:pageBreakBefore/><w:spacing w:after="0"/></w:pPr></w:p>"#);
                    }
                    body.push_str(&table(&t));
                }
                Block::Picture(data, ext, w, h) => {
                    let n = media.len() + 1;
                    let name = format!("image{n}.{}", if ext == "png" { "png" } else { "jpeg" });
                    rels.push_str(&format!(r#"<Relationship Id="img{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/{name}"/>"#));
                    body.push_str(&picture(n, w, h, max_w, max_h, new_page));
                    media.push((format!("word/media/{name}"), data));
                }
            }
            new_page = false;
        }
    }
    let (tw, th, tm) = ((pw * 20.0).round() as i32, (ph * 20.0).round() as i32, (margin * 20.0) as i32);
    let orient = if pw > ph { r#" w:orient="landscape""# } else { "" };
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><w:body>{body}<w:sectPr><w:pgSz w:w="{tw}" w:h="{th}"{orient}/><w:pgMar w:top="{tm}" w:right="{tm}" w:bottom="{tm}" w:left="{tm}" w:header="360" w:footer="360" w:gutter="0"/></w:sectPr></w:body></w:document>"#
    );
    let mut files: Vec<(String, Vec<u8>)> = vec![
        (
            "[Content_Types].xml".into(),
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="jpeg" ContentType="image/jpeg"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_vec(),
        ),
        (
            "_rels/.rels".into(),
            br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec(),
        ),
        ("word/document.xml".into(), document.into_bytes()),
        (
            "word/_rels/document.xml.rels".into(),
            format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rels}</Relationships>"#).into_bytes(),
        ),
    ];
    files.extend(media);
    std::fs::write(path, zip(&files)).map_err(|e| format!("Couldn't write {path}: {e}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn font_family_names() {
        assert_eq!(super::family("ABCDEF+Aptos-Bold"), "Aptos");
        assert_eq!(super::family("TimesNewRomanPSMT"), "Times New Roman");
        assert_eq!(super::family("Arial,Bold"), "Arial");
        assert_eq!(super::family("Calibri"), "Calibri");
        assert!(super::symbol_font("Symbol") && super::symbol_font("Wingdings 2") && !super::symbol_font("Aptos"));
        assert_eq!(super::clean("\u{f0b7} New office"), "• New office");
    }
}
