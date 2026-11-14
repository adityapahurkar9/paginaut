//! Tables read from a page, written as CSV or Excel (.xlsx).

use serde::{Deserialize, Serialize};

/// A table: where it is (relative to the page origin), its column and row lines, and its
/// cells as (row, column, rows spanned, columns spanned, text).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Table {
    pub r: [f32; 4],
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
    pub cells: Vec<(usize, usize, usize, usize, String)>,
}

impl Table {
    pub fn rows(&self) -> usize {
        self.ys.len().saturating_sub(1)
    }

    pub fn cols(&self) -> usize {
        self.xs.len().saturating_sub(1)
    }

    /// Every cell's text in a grid; a spanned cell's text sits in its first (top-left) slot.
    pub fn grid(&self) -> Vec<Vec<String>> {
        let (h, w) = (self.rows(), self.cols());
        let mut g = vec![vec![String::new(); w]; h];
        for (r, c, _, _, t) in &self.cells {
            if *r < h && *c < w {
                if !g[*r][*c].is_empty() {
                    g[*r][*c].push(' ');
                }
                g[*r][*c].push_str(t);
            }
        }
        // Rows with nothing in them add nothing.
        g.retain(|row| row.iter().any(|s| !s.trim().is_empty()));
        g
    }

    /// CSV (UTF-8 with a byte order mark, so Excel reads Indian scripts right).
    pub fn csv(&self) -> Vec<u8> {
        let mut out = String::from("\u{feff}");
        for row in self.grid() {
            let line: Vec<String> = row
                .iter()
                .map(|s| if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.clone() })
                .collect();
            out.push_str(&line.join(","));
            out.push_str("\r\n");
        }
        out.into_bytes()
    }

    /// An Excel workbook with one sheet: numbers as numbers, spanned cells merged, columns as wide as on the page.
    pub fn xlsx(&self) -> Vec<u8> {
        workbook(&[("Table".to_string(), self)])
    }

    /// The table as a worksheet.
    fn sheet(&self) -> String {
        let (h, w) = (self.rows(), self.cols());
        // Row numbers after empty rows are left out.
        let full = self.grid();
        let mut keep = vec![None; h];
        {
            let mut n = 0;
            let mut g = vec![vec![String::new(); w]; h];
            for (r, c, _, _, t) in &self.cells {
                if *r < h && *c < w {
                    g[*r][*c].push_str(t);
                }
            }
            for (i, row) in g.iter().enumerate() {
                if row.iter().any(|s| !s.trim().is_empty()) {
                    keep[i] = Some(n);
                    n += 1;
                }
            }
        }
        let mut sheet = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cols>"#);
        for c in 0..w {
            let width = ((self.xs[c + 1] - self.xs[c]) / 5.25).clamp(4.0, 80.0);
            sheet.push_str(&format!(r#"<col min="{0}" max="{0}" width="{width:.1}" customWidth="1"/>"#, c + 1));
        }
        sheet.push_str("</cols><sheetData>");
        for (r, row) in full.iter().enumerate() {
            sheet.push_str(&format!(r#"<row r="{}">"#, r + 1));
            for (c, text) in row.iter().enumerate() {
                if text.is_empty() {
                    continue;
                }
                let at = format!("{}{}", col_name(c), r + 1);
                match (number(text), percent(text)) {
                    (Some(n), _) => sheet.push_str(&format!(r#"<c r="{at}"><v>{n}</v></c>"#)),
                    // Styles 1 and 2: 0% and 0.00% (see STYLES).
                    (None, Some((v, decimals))) => sheet.push_str(&format!(r#"<c r="{at}" s="{}"><v>{v}</v></c>"#, if decimals { 2 } else { 1 })),
                    _ => sheet.push_str(&format!(r#"<c r="{at}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#, xml(text))),
                }
            }
            sheet.push_str("</row>");
        }
        sheet.push_str("</sheetData>");
        let merges: Vec<String> = self
            .cells
            .iter()
            .filter(|(r, c, rs, cs, _)| (*rs > 1 || *cs > 1) && *r < h && *c < w)
            .filter_map(|(r, c, rs, cs, _)| {
                // Rows kept around the span: from its first kept row to its last.
                let rows: Vec<usize> = (*r..(*r + *rs).min(h)).filter_map(|i| keep[i]).collect();
                let (top, bottom) = (*rows.first()?, *rows.last()?);
                let right = (*c + *cs).min(w) - 1;
                (bottom > top || right > *c).then(|| format!(r#"<mergeCell ref="{}{}:{}{}"/>"#, col_name(*c), top + 1, col_name(right), bottom + 1))
            })
            .collect();
        if !merges.is_empty() {
            sheet.push_str(&format!(r#"<mergeCells count="{}">{}</mergeCells>"#, merges.len(), merges.concat()));
        }
        sheet.push_str("</worksheet>");
        sheet
    }
}

/// Cell styles: 0 plain, 1 a percentage (12%), 2 one with decimals (12.50%).
const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="3"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="9" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="10" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#;

/// An Excel workbook with a sheet per table (names up to 31 characters, as Excel allows).
pub fn workbook(tables: &[(String, &Table)]) -> Vec<u8> {
    let n = tables.len().max(1);
    let mut types = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#);
    let mut sheets = String::new();
    let mut rels = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
    let mut files: Vec<(String, String)> = Vec::new();
    for i in 1..=n {
        types.push_str(&format!(r#"<Override PartName="/xl/worksheets/sheet{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#));
        let name: String = tables.get(i - 1).map_or("Sheet1", |t| t.0.as_str()).chars().filter(|c| !"[]:*?/\\".contains(*c)).take(31).collect();
        sheets.push_str(&format!(r#"<sheet name="{}" sheetId="{i}" r:id="rId{i}"/>"#, xml(&name)));
        rels.push_str(&format!(r#"<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i}.xml"/>"#));
        let body = tables.get(i - 1).map_or_else(
            || r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#.to_string(),
            |t| t.1.sheet(),
        );
        files.push((format!("xl/worksheets/sheet{i}.xml"), body));
    }
    types.push_str(r#"<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#);
    rels.push_str(r#"<Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#);
    let mut all: Vec<(String, String)> = vec![
        ("[Content_Types].xml".into(), types),
        ("_rels/.rels".into(), r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.into()),
        ("xl/workbook.xml".into(), format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{sheets}</sheets></workbook>"#)),
        ("xl/_rels/workbook.xml.rels".into(), rels),
        ("xl/styles.xml".into(), STYLES.into()),
    ];
    all.extend(files);
    zip(&all)
}

/// Excel's column name: 0 → A, 25 → Z, 26 → AA.
fn col_name(mut c: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (c % 26) as u8);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

/// A plain number ("1,23,456.50", "-12", "(1,234)" for a negative), for Excel to add up; not codes like "007".
fn number(s: &str) -> Option<String> {
    let t = s.trim();
    let (neg, t) = match t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        Some(inner) => (true, inner.trim()),
        None => (false, t),
    };
    // A comma is a thousands mark only where groups of digits follow it as such marks are written: threes
    // ("1,234,567"), or twos before a last three ("1,23,456"). Anything else ("12,5", "1.234,56": a decimal comma)
    // isn't a number read this way, and the cell stays text rather than become a wrong number.
    if t.contains(',') {
        let whole = t.strip_prefix('-').unwrap_or(t);
        let whole = whole.split_once('.').map_or(whole, |(int, frac)| if frac.contains(',') { "," } else { int });
        let groups: Vec<&str> = whole.split(',').collect();
        let last = groups.len() - 1;
        let grouped = groups.iter().enumerate().all(|(i, g)| {
            g.chars().all(|c| c.is_ascii_digit())
                && match i {
                    0 => (1..=3).contains(&g.len()),
                    i if i == last => g.len() == 3,
                    _ => g.len() == 2 || g.len() == 3,
                }
        });
        if !grouped {
            return None;
        }
    }
    let digits: String = t.chars().filter(|&ch| ch != ',').collect();
    let body = digits.strip_prefix('-').unwrap_or(&digits);
    let valid = !body.is_empty()
        && body.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
        && body.matches('.').count() <= 1
        && !body.starts_with('.')
        && !(body.len() > 1 && body.starts_with('0') && !body.starts_with("0."))
        && body.len() <= 15;
    if !valid {
        return None;
    }
    let v: f64 = digits.parse().ok()?;
    Some(format!("{}", if neg { -v } else { v }))
}

/// A percentage ("12.5%", "-4 %", "(3%)" for a negative) as its value (0.125), and whether it has decimals.
fn percent(s: &str) -> Option<(String, bool)> {
    let t = s.trim();
    let (neg, t) = match t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        Some(inner) => (true, inner.trim()),
        None => (false, t),
    };
    let n = number(t.strip_suffix('%')?.trim_end())?;
    let (minus, digits) = match n.strip_prefix('-') {
        Some(d) => (!neg, d),
        None => (neg, n.as_str()),
    };
    // Two places left, done on the digits (7.1 / 100 isn't exactly 0.071 in binary).
    let (int, frac) = digits.split_once('.').unwrap_or((digits, ""));
    let int = format!("{int:0>3}");
    let (whole, moved) = int.split_at(int.len() - 2);
    let whole = whole.trim_start_matches('0');
    let frac = format!("{moved}{frac}");
    let frac = frac.trim_end_matches('0');
    let v = match (whole.is_empty(), frac.is_empty()) {
        (true, true) => "0".to_string(),
        (_, true) => whole.to_string(),
        (true, false) => format!("0.{frac}"),
        (false, false) => format!("{whole}.{frac}"),
    };
    let v = if minus && v != "0" { format!("-{v}") } else { v };
    Some((v, t.contains('.')))
}

pub(crate) fn xml(s: &str) -> String {
    s.chars()
        .filter(|&c| c == '\t' || c == '\n' || c >= ' ')
        .fold(String::with_capacity(s.len()), |mut o, c| {
            match c {
                '&' => o.push_str("&amp;"),
                '<' => o.push_str("&lt;"),
                '>' => o.push_str("&gt;"),
                '"' => o.push_str("&quot;"),
                _ => o.push(c),
            }
            o
        })
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, e) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *e = c;
    }
    !data.iter().fold(!0u32, |c, &b| table[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8))
}

/// A zip archive with the files stored as they are (Office files need no compression to open).
pub(crate) fn zip<N: AsRef<str>, B: AsRef<[u8]>>(files: &[(N, B)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, body) in files {
        let (name, body) = (name.as_ref().as_bytes(), body.as_ref());
        let (crc, len, at) = (crc32(body), body.len() as u32, out.len() as u32);
        let head = |sig: u32, central: bool| {
            let mut h = Vec::new();
            h.extend_from_slice(&sig.to_le_bytes());
            if central {
                h.extend_from_slice(&20u16.to_le_bytes());
            }
            h.extend_from_slice(&20u16.to_le_bytes()); // version needed
            h.extend_from_slice(&0x0800u16.to_le_bytes()); // names in UTF-8
            h.extend_from_slice(&0u16.to_le_bytes()); // stored
            h.extend_from_slice(&0u32.to_le_bytes()); // time, date
            h.extend_from_slice(&crc.to_le_bytes());
            h.extend_from_slice(&len.to_le_bytes());
            h.extend_from_slice(&len.to_le_bytes());
            h.extend_from_slice(&(name.len() as u16).to_le_bytes());
            h.extend_from_slice(&0u16.to_le_bytes()); // extra
            if central {
                h.extend_from_slice(&[0; 6]); // comment length, disk, internal attributes
                h.extend_from_slice(&0u32.to_le_bytes()); // external attributes
                h.extend_from_slice(&at.to_le_bytes());
            }
            h.extend_from_slice(name);
            h
        };
        out.extend(head(0x0403_4b50, false));
        out.extend_from_slice(body);
        central.extend(head(0x0201_4b50, true));
    }
    let (start, size) = (out.len() as u32, central.len() as u32);
    out.extend(central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a workbook with numbers and percentages to PAGINAUT_XLSX_OUT, to open in Excel.
    #[test]
    fn workbook_with_percentages() {
        let t = Table {
            r: [0.0; 4],
            xs: vec![0.0, 100.0, 200.0],
            ys: vec![0.0, 20.0, 40.0, 60.0],
            cells: vec![
                (0, 0, 1, 1, "Share".into()),
                (0, 1, 1, 1, "Growth".into()),
                (1, 0, 1, 1, "12.5%".into()),
                (1, 1, 1, 1, "(3%)".into()),
                (2, 0, 1, 1, "1,23,456.50".into()),
                (2, 1, 1, 1, "7.1 %".into()),
            ],
        };
        let book = t.xlsx();
        let text = String::from_utf8_lossy(&book);
        assert!(text.contains(r#"<c r="A2" s="2"><v>0.125</v></c>"#), "{text}");
        assert!(text.contains(r#"<c r="B2" s="1"><v>-0.03</v></c>"#));
        assert!(text.contains("xl/styles.xml"));
        if let Ok(out) = std::env::var("PAGINAUT_XLSX_OUT") {
            std::fs::write(out, book).unwrap();
        }
    }

    #[test]
    fn numbers() {
        assert_eq!(number("1,23,456.50").as_deref(), Some("123456.5"));
        assert_eq!(number("(1,234)").as_deref(), Some("-1234"));
        assert_eq!(number("007"), None);
        assert_eq!(number("0.5").as_deref(), Some("0.5"));
        assert_eq!(number("12a"), None);
        // A decimal comma isn't a thousands mark: text, not a wrong number.
        assert_eq!(number("12,5"), None);
        assert_eq!(number("1.234,56"), None);
        assert_eq!(number("12,50"), None);
        assert_eq!(number("1,2345"), None);
        assert_eq!(number("1,234,567.25").as_deref(), Some("1234567.25"));
        assert_eq!(number("-1,234").as_deref(), Some("-1234"));
        assert_eq!(percent("12,5%"), None);
        assert_eq!(percent("12.5%"), Some(("0.125".into(), true)));
        assert_eq!(percent("7.1 %"), Some(("0.071".into(), true)));
        assert_eq!(percent("(3%)"), Some(("-0.03".into(), false)));
        assert_eq!(percent("-150%"), Some(("-1.5".into(), false)));
        assert_eq!(percent("1,200%"), Some(("12".into(), false)));
        assert_eq!(percent("0%"), Some(("0".into(), false)));
        assert_eq!(percent("12.0%"), Some(("0.12".into(), true)));
        assert_eq!(percent("12"), None);
        assert_eq!(percent("abc%"), None);
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(27), "AB");
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }
}
