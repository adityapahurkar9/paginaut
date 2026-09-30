//! A page's content as a flow for writers of editable documents (Word, Markdown): paragraphs, tables and
//! pictures in reading order, without what only makes sense on a printed page (page numbers).

use crate::engine::table::Table;
use crate::engine::{Doc, Paragraph, Picture};

pub enum Block {
    Para(Paragraph),
    Table(Table),
    /// Picture bytes, their kind ("png" or "jpeg"), and size in points.
    Picture(Vec<u8>, &'static str, f32, f32),
}

/// What's on page `index`, in reading order: paragraphs as MuPDF reads them, each table where its first
/// paragraph was (the paragraphs inside it left out), each picture before the first paragraph below its top.
pub fn blocks(d: &Doc, index: usize) -> Result<Vec<Block>, String> {
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

/// The pages `order` as blocks, without running headers and footers: the same text near the top or bottom of
/// most pages. Calls `progress` per page; stops when it says so.
pub fn document(d: &Doc, order: &[usize], progress: &mut dyn FnMut(usize, usize) -> bool) -> Result<Vec<Vec<Block>>, String> {
    let mut pages = Vec::with_capacity(order.len());
    let mut edges: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut heights = Vec::with_capacity(order.len());
    for (k, &page) in order.iter().enumerate() {
        if !progress(k, order.len()) {
            return Err("stopped".into());
        }
        let b = d.bounds(page)?;
        let h = b[3] - b[1];
        let blocks = blocks(d, page)?;
        let mut seen = std::collections::HashSet::new();
        for block in &blocks {
            if let Block::Para(p) = block {
                if near_edge(p, h) && seen.insert(key(&p.t)) {
                    *edges.entry(key(&p.t)).or_default() += 1;
                }
            }
        }
        heights.push(h);
        pages.push(blocks);
    }
    if order.len() >= 2 {
        let many = (order.len() / 2).max(2);
        for (blocks, h) in pages.iter_mut().zip(heights) {
            blocks.retain(|b| !matches!(b, Block::Para(p) if near_edge(p, h) && edges.get(&key(&p.t)).is_some_and(|&n| n >= many)));
        }
    }
    Ok(pages)
}

fn near_edge(p: &Paragraph, height: f32) -> bool {
    p.r[3] < height * 0.12 || p.r[1] > height * 0.88
}

/// Text compared across pages, with numbers left out ("Page 3 of 9" and "Page 4 of 9" are the same footer).
fn key(t: &str) -> String {
    t.chars().filter(|c| !c.is_ascii_digit() && !c.is_whitespace()).collect::<String>().to_lowercase()
}

/// Bullets drawn with symbol fonts arrive as private-use characters (U+F0B7…): a plain bullet instead.
pub fn clean(text: &str) -> String {
    text.chars().map(|c| if ('\u{f020}'..='\u{f0ff}').contains(&c) { '•' } else { c }).collect()
}


#[cfg(test)]
mod tests {
    #[test]
    fn symbol_bullets() {
        assert_eq!(super::clean("\u{f0b7} New office"), "• New office");
    }
}
