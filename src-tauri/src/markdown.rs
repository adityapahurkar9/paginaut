//! PDF pages as Markdown: headings from larger text, bold and italic paragraphs, lists, tables (GitHub's pipe
//! tables), pictures saved in a folder next to the .md file and linked from it. (MuPDF reads Markdown but
//! doesn't write it.)

use crate::engine::{Doc, Paragraph};
use crate::flow::{clean, document, Block};
use std::path::Path;

/// The size most of the text is set in (weighted by length): the body size headings stand out from.
fn body_size(all: &[&Paragraph]) -> f32 {
    let mut sizes: Vec<(i32, usize)> = Vec::new();
    for p in all {
        let key = (p.s * 2.0).round() as i32;
        match sizes.iter_mut().find(|(k, _)| *k == key) {
            Some(e) => e.1 += p.t.len(),
            None => sizes.push((key, p.t.len())),
        }
    }
    sizes.iter().max_by_key(|e| e.1).map_or(11.0, |e| e.0 as f32 / 2.0)
}

/// Characters Markdown would read as formatting.
fn escape(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    for c in t.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '<' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A line start Markdown would take for a heading, quote, list or rule is escaped.
fn escape_start(t: &str) -> String {
    let first = t.chars().next().unwrap_or(' ');
    // Markdown takes up to nine digits and a dot or bracket for a numbered list.
    let numbered = t.find(['.', ')']).filter(|&i| i > 0 && i < 10 && t[..i].chars().all(|c| c.is_ascii_digit()) && t[i + 1..].starts_with(' '));
    if let Some(i) = numbered {
        // The dot (or bracket) is what makes the list, and what takes the backslash: Markdown only takes one
        // before punctuation, so before the number it would show.
        format!("{}\\{}", &t[..i], &t[i..])
    } else if matches!(first, '#' | '>' | '+' | '-' | '=' | '|') {
        format!("\\{t}")
    } else {
        t.to_string()
    }
}

fn paragraph(p: &Paragraph, body: f32) -> String {
    let text = clean(&p.t.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" "));
    let text = text.trim();
    // Headings: text clearly larger than the body ("1. Summary" in large type is a heading, not a list).
    let ratio = p.s / body.max(1.0);
    let short = text.chars().count() < 120 && !text.ends_with('.');
    let level = if ratio >= 1.7 {
        1
    } else if ratio >= 1.35 {
        2
    } else if ratio >= 1.15 && short {
        3
    } else {
        0
    };
    if level > 0 {
        return format!("{} {}", "#".repeat(level), escape(text));
    }
    // Lists: a bullet or a number at the start.
    for bullet in ['•', '◦', '▪', '●', '○', '■', '–', '·'] {
        if let Some(rest) = text.strip_prefix(bullet) {
            return format!("- {}", escape(rest.trim()));
        }
    }
    if let Some(i) = text.find(['.', ')']).filter(|&i| i > 0 && i < 4 && text[..i].chars().all(|c| c.is_ascii_digit())) {
        if text[i + 1..].starts_with(' ') {
            return format!("{}. {}", &text[..i], escape(text[i + 1..].trim()));
        }
    }
    let body = escape_start(&escape(text));
    match (p.b != 0, p.i != 0) {
        (true, true) => format!("***{body}***"),
        (true, false) => format!("**{body}**"),
        (false, true) => format!("*{body}*"),
        _ => body,
    }
}

fn table(t: &crate::engine::table::Table) -> String {
    let grid = t.grid();
    let cols = grid.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return String::new();
    }
    let cell = |s: &str| escape(s.trim()).replace('|', "\\|").replace('\n', " ");
    let row = |r: &Vec<String>| format!("| {} |", (0..cols).map(|c| cell(r.get(c).map_or("", |s| s.as_str()))).collect::<Vec<_>>().join(" | "));
    let mut out = vec![row(&grid[0]), format!("|{}", " --- |".repeat(cols))];
    out.extend(grid.iter().skip(1).map(row));
    out.join("\n")
}

/// Writes pages `order` of `d` as Markdown at `path`; pictures go into "<name>_files" next to it.
/// Returns how many pictures were saved. Calls `progress` per page; stops when it says so.
pub fn write(d: &Doc, order: &[usize], path: &str, progress: &mut dyn FnMut(usize, usize) -> bool) -> Result<usize, String> {
    let out = Path::new(path);
    let stem = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "document".into());
    let folder = format!("{stem}_files");
    let dir = out.with_file_name(&folder);
    let pages = document(d, order, progress)?;
    let paras: Vec<&Paragraph> = pages.iter().flatten().filter_map(|b| if let Block::Para(p) = b { Some(p) } else { None }).collect();
    let body = body_size(&paras);
    let mut md: Vec<String> = Vec::new();
    let mut pictures = 0;
    for blocks in &pages {
        for b in blocks {
            match b {
                Block::Para(p) if !p.t.trim().is_empty() => {
                    let line = paragraph(p, body);
                    // A bullet read as a paragraph of its own (Word's PDFs put it apart from its text): the item is
                    // the text after it.
                    match md.last_mut() {
                        Some(prev) if prev.as_str() == "- " && !line.starts_with(['#', '-', '|', '!']) => prev.push_str(&line),
                        _ => md.push(line),
                    }
                }
                Block::Para(_) => {}
                Block::Table(t) => md.push(table(t)),
                Block::Picture(pic, _, _) => {
                    let (data, kind) = pic.read(d)?;
                    if pictures == 0 {
                        std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
                    }
                    pictures += 1;
                    let name = format!("picture-{pictures}.{}", if kind == "png" { "png" } else { "jpg" });
                    std::fs::write(dir.join(&name), &data).map_err(|e| e.to_string())?;
                    // Angle brackets keep a name with spaces in one link.
                    md.push(format!("![Picture {pictures}](<{folder}/{name}>)"));
                }
            }
        }
    }
    // List items that follow each other stay one list.
    let mut text = String::new();
    for (i, block) in md.iter().enumerate() {
        if i > 0 {
            let list = |s: &str| s.starts_with("- ") || s.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
            text.push_str(if list(block) && list(&md[i - 1]) { "\n" } else { "\n\n" });
        }
        text.push_str(block);
    }
    text.push('\n');
    std::fs::write(out, text).map_err(|e| format!("Couldn't write {path}: {e}"))?;
    Ok(pictures)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes() {
        assert_eq!(escape("a*b_c"), "a\\*b\\_c");
        assert_eq!(escape_start("# not a heading"), "\\# not a heading");
        assert_eq!(escape_start("2024. A year"), "2024\\. A year");
        assert_eq!(escape_start("12. Twelfth"), "12\\. Twelfth");
        assert_eq!(escape_start("3) Third"), "3\\) Third");
        assert_eq!(escape_start("Plain"), "Plain");
    }
}
