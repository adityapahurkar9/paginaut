//! The text of Word, PowerPoint and Excel files (and their OpenDocument cousins), read straight from the file
//! for Compare: no two programs lay an Office file out the same way, but its words are its own.

use crate::engine::mupdf::zip_read;
use crate::xml::{attr, Ev, Reader};
use serde::Serialize;
use std::collections::HashMap;

/// A paragraph, heading, slide or sheet title, or a row of cells.
#[derive(Serialize, Clone, Debug)]
pub struct Block {
    /// "h" heading, "p" paragraph, "slide" or "sheet" (a title of its own), "row" (cells).
    pub k: &'static str,
    /// The text; a row's cells.
    pub t: Vec<String>,
    /// A row's cells' names ("B7").
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    /// A row's cells' values as numbers, as shown (percentages ×100); dates and text have none.
    #[serde(skip)]
    pub nums: Vec<Option<f64>>,
}

impl Block {
    fn text(k: &'static str, t: String) -> Self {
        Block { k, t: vec![t], refs: Vec::new(), nums: Vec::new() }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Word,
    Slides,
    Sheets,
    OdfText,
    OdfSlides,
    OdfSheets,
}

/// Files whose text is read directly (others are compared through the PDF they're shown as).
pub fn kind(path: &str) -> Option<Kind> {
    let ext = std::path::Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "docx" | "docm" | "dotx" | "dotm" => Kind::Word,
        "pptx" | "pptm" | "ppsx" | "ppsm" | "potx" | "potm" => Kind::Slides,
        "xlsx" | "xlsm" | "xltx" | "xltm" => Kind::Sheets,
        "odt" | "ott" => Kind::OdfText,
        "odp" | "otp" => Kind::OdfSlides,
        "ods" | "ots" => Kind::OdfSheets,
        _ => return None,
    })
}

pub fn read(path: &str) -> Result<Vec<Block>, String> {
    let k = kind(path).ok_or("Not a Word, PowerPoint or Excel file")?;
    let res = match k {
        Kind::Word => word(path),
        Kind::Slides => slides(path),
        Kind::Sheets => sheets(path),
        Kind::OdfText | Kind::OdfSlides | Kind::OdfSheets => odf(path, k),
    };
    res.map_err(|e| format!("Couldn't read {}: {e}", std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
}

fn part(path: &str, name: &str) -> Result<Option<String>, String> {
    Ok(zip_read(path, name)?.map(|b| String::from_utf8_lossy(&b).into_owned()))
}

fn need(path: &str, name: &str) -> Result<String, String> {
    part(path, name)?.ok_or_else(|| "it isn't a complete Office file (a part is missing); it may be protected with a password".into())
}

/// Id → target of a part's relationships, the targets made full names inside the zip.
fn rels(path: &str, of: &str) -> Result<HashMap<String, String>, String> {
    let (dir, file) = of.rsplit_once('/').unwrap_or(("", of));
    let mut out = HashMap::new();
    let Some(xml) = part(path, &format!("{}{}_rels/{file}.rels", if dir.is_empty() { "" } else { dir }, if dir.is_empty() { "" } else { "/" }))? else {
        return Ok(out);
    };
    for ev in Reader::new(&xml) {
        if let Ev::Open { name: "Relationship", attrs, .. } = ev {
            if let (Some(id), Some(t)) = (attr(attrs, "Id"), attr(attrs, "Target")) {
                out.insert(id.into_owned(), resolve(dir, &t));
            }
        }
    }
    Ok(out)
}

fn resolve(dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = if dir.is_empty() { Vec::new() } else { dir.split('/').collect() };
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn push_para(out: &mut Vec<Block>, k: &'static str, text: &mut String) {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    text.clear();
    if !t.is_empty() {
        out.push(Block::text(k, t));
    }
}

// ---------------------------------------------------------------- Word

/// Paragraphs (headings by their style) and table rows, in order. Deleted text in tracked changes, field codes
/// and the second copy of text boxes (the fallback drawing) are left out; headers, footers and notes too.
fn word(path: &str) -> Result<Vec<Block>, String> {
    let xml = need(path, "word/document.xml")?;
    let mut out = Vec::new();
    let mut para = String::new();
    let mut heading = false;
    let (mut skip, mut in_t) = (0usize, false);
    // Open tables: the current row's cells, and the current cell's text.
    let mut tables: Vec<(Vec<String>, Option<String>)> = Vec::new();
    for ev in Reader::new(&xml) {
        match ev {
            Ev::Open { name, attrs, empty } => {
                if skip > 0 {
                    skip += !empty as usize;
                    continue;
                }
                match name {
                    "Fallback" if !empty => skip = 1,
                    "t" => in_t = !empty,
                    "tab" | "br" | "cr" => para.push(' '),
                    "noBreakHyphen" => para.push('-'),
                    "pStyle" => {
                        let v = attr(attrs, "w:val").unwrap_or_default().to_ascii_lowercase();
                        heading = v.starts_with("heading") || v == "title" || v == "subtitle";
                    }
                    "tbl" if !empty => tables.push((Vec::new(), None)),
                    "tr" => {
                        if let Some(t) = tables.last_mut() {
                            t.0.clear();
                        }
                    }
                    "tc" if !empty => {
                        if let Some(t) = tables.last_mut() {
                            t.1 = Some(String::new());
                        }
                    }
                    _ => {}
                }
            }
            Ev::Close(name) => {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                match name {
                    "t" => in_t = false,
                    "p" => {
                        match tables.last_mut().and_then(|t| t.1.as_mut()) {
                            Some(cell) => {
                                if !cell.is_empty() {
                                    cell.push(' ');
                                }
                                cell.push_str(&para);
                                para.clear();
                            }
                            None => push_para(&mut out, if heading { "h" } else { "p" }, &mut para),
                        }
                        heading = false;
                    }
                    "tc" => {
                        if let Some(t) = tables.last_mut() {
                            let cell = t.1.take().unwrap_or_default();
                            t.0.push(cell.split_whitespace().collect::<Vec<_>>().join(" "));
                        }
                    }
                    "tr" => {
                        if let Some(t) = tables.last_mut() {
                            if t.0.iter().any(|c| !c.is_empty()) {
                                let n = t.0.len();
                                out.push(Block { k: "row", t: std::mem::take(&mut t.0), refs: Vec::new(), nums: vec![None; n] });
                            }
                        }
                    }
                    "tbl" => {
                        tables.pop();
                    }
                    _ => {}
                }
            }
            Ev::Text(t) => {
                if in_t && skip == 0 {
                    para.push_str(&t);
                }
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- PowerPoint

/// The slides in order (hidden ones left out, as in a PDF of the deck): a "Slide n" line, then each paragraph of
/// its shapes and tables. Slide numbers and dates filled in by PowerPoint are left out.
fn slides(path: &str) -> Result<Vec<Block>, String> {
    let pres = need(path, "ppt/presentation.xml")?;
    let rel = rels(path, "ppt/presentation.xml")?;
    let ids: Vec<String> = Reader::new(&pres)
        .filter_map(|e| match e {
            Ev::Open { name: "sldId", attrs, .. } => attr(attrs, "r:id").map(|v| v.into_owned()),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for (n, id) in ids.iter().enumerate() {
        let Some(target) = rel.get(id) else { continue };
        let Some(xml) = part(path, target)? else { continue };
        let mut blocks = Vec::new();
        let mut hidden = false;
        let (mut para, mut skip, mut in_t, mut title, mut field) = (String::new(), 0usize, false, false, false);
        for ev in Reader::new(&xml) {
            match ev {
                Ev::Open { name, attrs, empty } => {
                    if skip > 0 {
                        skip += !empty as usize;
                        continue;
                    }
                    match name {
                        "sld" => hidden = attr(attrs, "show").as_deref() == Some("0"),
                        "Fallback" if !empty => skip = 1,
                        "sp" => title = false,
                        "ph" => title = matches!(attr(attrs, "type").as_deref(), Some("title" | "ctrTitle")),
                        "fld" if !empty => {
                            let t = attr(attrs, "type").unwrap_or_default();
                            field = t == "slidenum" || t.starts_with("datetime");
                        }
                        "t" => in_t = !empty,
                        "br" => para.push(' '),
                        _ => {}
                    }
                }
                Ev::Close(name) => {
                    if skip > 0 {
                        skip -= 1;
                        continue;
                    }
                    match name {
                        "t" => in_t = false,
                        "fld" => field = false,
                        "p" => push_para(&mut blocks, if title { "h" } else { "p" }, &mut para),
                        _ => {}
                    }
                }
                Ev::Text(t) => {
                    if in_t && !field && skip == 0 {
                        para.push_str(&t);
                    }
                }
            }
        }
        if !hidden {
            out.push(Block::text("slide", format!("Slide {}", n + 1)));
            out.extend(blocks);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- Excel

/// The visible sheets: a line with the sheet's name, then each row with something in it, the cells as Excel
/// shows them (number formats, dates, percentages). Hidden rows and columns are left out, as in a printout.
fn sheets(path: &str) -> Result<Vec<Block>, String> {
    let book = need(path, "xl/workbook.xml")?;
    let rel = rels(path, "xl/workbook.xml")?;
    let strings = shared_strings(path)?;
    let styles = Styles::read(path)?;
    let date1904 = Reader::new(&book).any(|e| matches!(e, Ev::Open { name: "workbookPr", attrs, .. } if matches!(attr(attrs, "date1904").as_deref(), Some("1" | "true"))));
    let mut list = Vec::new();
    for ev in Reader::new(&book) {
        if let Ev::Open { name: "sheet", attrs, .. } = ev {
            let visible = !matches!(attr(attrs, "state").as_deref(), Some("hidden" | "veryHidden"));
            if let (true, Some(name), Some(id)) = (visible, attr(attrs, "name"), attr(attrs, "r:id")) {
                list.push((name.into_owned(), id.into_owned()));
            }
        }
    }
    let mut out = Vec::new();
    for (name, id) in list {
        let Some(target) = rel.get(&id) else { continue };
        let Some(xml) = part(path, target)? else { continue }; // a chart sheet
        out.push(Block::text("sheet", name));
        let mut hidden_cols: Vec<(u32, u32)> = Vec::new();
        let mut row = Block { k: "row", t: Vec::new(), refs: Vec::new(), nums: Vec::new() };
        let mut row_hidden = false;
        // The cell being read: its name, type, style, value and inline text.
        let (mut cref, mut ctype, mut cstyle) = (String::new(), String::new(), 0usize);
        let (mut value, mut inline, mut in_v, mut in_t, mut in_rph) = (String::new(), String::new(), false, false, false);
        for ev in Reader::new(&xml) {
            match ev {
                Ev::Open { name, attrs, empty } => match name {
                    "col" => {
                        if attr(attrs, "hidden").is_some_and(|v| v == "1" || v == "true") {
                            let n = |k| attr(attrs, k).and_then(|v| v.parse().ok()).unwrap_or(0u32);
                            hidden_cols.push((n("min"), n("max")));
                        }
                    }
                    "row" => {
                        row_hidden = attr(attrs, "hidden").is_some_and(|v| v == "1" || v == "true");
                        row.t.clear();
                        row.refs.clear();
                        row.nums.clear();
                    }
                    "c" => {
                        cref = attr(attrs, "r").unwrap_or_default().into_owned();
                        ctype = attr(attrs, "t").unwrap_or_default().into_owned();
                        cstyle = attr(attrs, "s").and_then(|v| v.parse().ok()).unwrap_or(0);
                        value.clear();
                        inline.clear();
                        if empty {
                            cref.clear();
                        }
                    }
                    "v" => in_v = !empty,
                    "t" => in_t = !empty,
                    "rPh" => in_rph = !empty,
                    _ => {}
                },
                Ev::Close(name) => match name {
                    "v" => in_v = false,
                    "t" => in_t = false,
                    "rPh" => in_rph = false,
                    "c" => {
                        if cref.is_empty() || row_hidden {
                            continue;
                        }
                        let col = column_of(&cref);
                        if hidden_cols.iter().any(|&(a, b)| col >= a && col <= b) {
                            continue;
                        }
                        let (text, num) = match ctype.as_str() {
                            "s" => (value.trim().parse::<usize>().ok().and_then(|i| strings.get(i).cloned()).unwrap_or_default(), None),
                            "inlineStr" => (inline.clone(), None),
                            "str" | "e" => (value.clone(), None),
                            "b" => ((if value.trim() == "1" { "TRUE" } else { "FALSE" }).to_string(), None),
                            _ => match value.trim().parse::<f64>() {
                                Ok(v) => styles.show(v, cstyle, date1904),
                                Err(_) => (value.clone(), None),
                            },
                        };
                        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                        if !text.is_empty() {
                            row.t.push(text);
                            row.refs.push(std::mem::take(&mut cref));
                            row.nums.push(num);
                        }
                    }
                    "row" => {
                        if !row.t.is_empty() {
                            out.push(row.clone());
                        }
                    }
                    _ => {}
                },
                Ev::Text(t) => {
                    if in_v {
                        value.push_str(&t);
                    } else if in_t && !in_rph {
                        inline.push_str(&t);
                    }
                }
            }
        }
    }
    Ok(out)
}

/// "BC12" → 55 (columns counted from 1).
fn column_of(r: &str) -> u32 {
    r.bytes().take_while(|b| b.is_ascii_alphabetic()).fold(0, |n, b| n * 26 + (b.to_ascii_uppercase() - b'A' + 1) as u32)
}

fn shared_strings(path: &str) -> Result<Vec<String>, String> {
    let Some(xml) = part(path, "xl/sharedStrings.xml")? else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    let (mut cur, mut in_t, mut in_rph) = (String::new(), false, false);
    for ev in Reader::new(&xml) {
        match ev {
            Ev::Open { name: "si", .. } => cur.clear(),
            Ev::Open { name: "t", empty, .. } => in_t = !empty,
            Ev::Open { name: "rPh", empty, .. } => in_rph = !empty,
            Ev::Close("t") => in_t = false,
            Ev::Close("rPh") => in_rph = false,
            Ev::Close("si") => out.push(std::mem::take(&mut cur)),
            Ev::Text(t) if in_t && !in_rph => cur.push_str(&t),
            _ => {}
        }
    }
    Ok(out)
}

struct Styles {
    /// Per cell style: its number format code.
    formats: Vec<String>,
}

impl Styles {
    fn read(path: &str) -> Result<Self, String> {
        let mut custom: HashMap<u32, String> = HashMap::new();
        let mut ids = Vec::new();
        if let Some(xml) = part(path, "xl/styles.xml")? {
            let mut in_xfs = false;
            for ev in Reader::new(&xml) {
                match ev {
                    Ev::Open { name: "numFmt", attrs, .. } => {
                        if let (Some(id), Some(code)) = (attr(attrs, "numFmtId").and_then(|v| v.parse().ok()), attr(attrs, "formatCode")) {
                            custom.insert(id, code.into_owned());
                        }
                    }
                    Ev::Open { name: "cellXfs", empty, .. } => in_xfs = !empty,
                    Ev::Close("cellXfs") => in_xfs = false,
                    Ev::Open { name: "xf", attrs, .. } if in_xfs => ids.push(attr(attrs, "numFmtId").and_then(|v| v.parse().ok()).unwrap_or(0u32)),
                    _ => {}
                }
            }
        }
        let formats = ids.into_iter().map(|id| custom.get(&id).cloned().unwrap_or_else(|| builtin_format(id))).collect();
        Ok(Styles { formats })
    }

    /// A number as its cell shows it, and its value as shown (None for dates and times).
    fn show(&self, v: f64, style: usize, date1904: bool) -> (String, Option<f64>) {
        let code = self.formats.get(style).map(String::as_str).unwrap_or("General");
        format_value(v, code, date1904)
    }
}

fn builtin_format(id: u32) -> String {
    match id {
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        5 | 37 => "#,##0 ;(#,##0)",
        6 | 38 => "#,##0 ;[Red](#,##0)",
        7 | 39 => "#,##0.00;(#,##0.00)",
        8 | 40 => "#,##0.00;[Red](#,##0.00)",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => return short_date(),
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => return format!("{} h:mm", short_date()),
        41 => "#,##0",
        42 => "#,##0",
        43 => "#,##0.00",
        44 => "#,##0.00",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mm:ss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => "General",
    }
    .to_string()
}

/// Windows' short date ("dd-MM-yyyy"), as an Excel format: format 14 follows it.
fn short_date() -> String {
    #[cfg(windows)]
    {
        use windows::Win32::Globalization::{GetLocaleInfoEx, LOCALE_SSHORTDATE};
        let mut buf = [0u16; 80];
        let n = unsafe { GetLocaleInfoEx(windows::core::PCWSTR::null(), LOCALE_SSHORTDATE, Some(&mut buf)) };
        if n > 1 {
            return String::from_utf16_lossy(&buf[..n as usize - 1]).replace('M', "m").replace("tt", "AM/PM");
        }
    }
    "m/d/yyyy".into()
}

/// Splits a format code into its sections at ';' outside quotes and brackets.
fn sections(code: &str) -> Vec<&str> {
    let (mut out, mut start, mut quote, mut bracket) = (Vec::new(), 0, false, false);
    let b = code.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' => quote = !quote,
            b'\\' if !quote => i += 1,
            b'[' if !quote => bracket = true,
            b']' if !quote => bracket = false,
            b';' if !quote && !bracket => {
                out.push(&code[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&code[start..]);
    out
}

/// A format section's parts: literal text and codes (runs of 0 # ? . , % E+ and date letters), with [Red]-style
/// brackets dropped (a currency's symbol kept, elapsed time [h] kept as a code).
enum Item {
    Lit(String),
    Code(String),
}

fn items(sec: &str) -> Vec<Item> {
    let ch: Vec<char> = sec.chars().collect();
    let mut out = Vec::new();
    let mut code = String::new();
    fn flush(code: &mut String, out: &mut Vec<Item>) {
        if !code.is_empty() {
            out.push(Item::Code(std::mem::take(code)));
        }
    }
    let mut i = 0;
    while i < ch.len() {
        let c = ch[i];
        let ahead: String = ch[i..ch.len().min(i + 5)].iter().collect::<String>().to_ascii_uppercase();
        if ahead.starts_with("AM/PM") || ahead.starts_with("A/P") {
            flush(&mut code, &mut out);
            let n = if ahead.starts_with("AM/PM") { 5 } else { 3 };
            out.push(Item::Code(ahead[..n].to_string()));
            i += n;
            continue;
        }
        match c {
            '"' => {
                flush(&mut code, &mut out);
                let end = ch[i + 1..].iter().position(|&x| x == '"').map_or(ch.len(), |p| i + 1 + p);
                out.push(Item::Lit(ch[i + 1..end].iter().collect()));
                i = end + 1;
                continue;
            }
            '\\' => {
                flush(&mut code, &mut out);
                if let Some(n) = ch.get(i + 1) {
                    out.push(Item::Lit(n.to_string()));
                }
                i += 2;
                continue;
            }
            '_' | '*' => {
                flush(&mut code, &mut out);
                if c == '_' {
                    out.push(Item::Lit(" ".into()));
                }
                i += 2;
                continue;
            }
            '[' => {
                flush(&mut code, &mut out);
                let end = ch[i + 1..].iter().position(|&x| x == ']').map_or(ch.len(), |p| i + 1 + p);
                let inner: String = ch[i + 1..end].iter().collect();
                if let Some(cur) = inner.strip_prefix('$') {
                    let sym = cur.split('-').next().unwrap_or("");
                    if !sym.is_empty() {
                        out.push(Item::Lit(sym.to_string()));
                    }
                } else if !inner.is_empty() && inner.chars().all(|c| "hHmMsS".contains(c)) {
                    out.push(Item::Code(format!("[{}]", inner.to_ascii_lowercase())));
                }
                i = end + 1;
                continue;
            }
            'E' | 'e' if matches!(ch.get(i + 1), Some('+' | '-')) && !code.is_empty() => {
                code.push(c);
                code.push(ch[i + 1]);
                i += 2;
                continue;
            }
            '0' | '#' | '?' | '.' | ',' | '%' | 'y' | 'Y' | 'm' | 'M' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' => code.push(c),
            _ => {
                flush(&mut code, &mut out);
                out.push(Item::Lit(c.to_string()));
            }
        }
        i += 1;
    }
    flush(&mut code, &mut out);
    out
}

fn is_date(items: &[Item]) -> bool {
    let codes = || items.iter().filter_map(|i| if let Item::Code(c) = i { Some(c.as_str()) } else { None });
    codes().any(|c| c.starts_with('[') || c.chars().any(|c| "ymdhsYMDHS".contains(c)))
        && !codes().any(|c| c.contains(['#', '?']) || c.starts_with('0') || c.starts_with(','))
}

/// A value shown with an Excel number format, and its value as shown (None for dates).
pub fn format_value(v: f64, code: &str, date1904: bool) -> (String, Option<f64>) {
    let secs = sections(code);
    let (sec, x, minus) = if v < 0.0 && secs.len() >= 2 {
        (secs[1], -v, false)
    } else if v == 0.0 && secs.len() >= 3 {
        (secs[2], v, false)
    } else {
        (secs[0], v.abs(), v < 0.0)
    };
    let trimmed = sec.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("general") || trimmed == "@" {
        return (general(v), Some(v));
    }
    let its = items(sec);
    if is_date(&its) {
        return (format_date(v, &its, date1904), None);
    }
    let codes: String = its.iter().filter_map(|i| if let Item::Code(c) = i { Some(c.as_str()) } else { None }).collect();
    let percent = its.iter().any(|i| matches!(i, Item::Code(c) if c.contains('%')));
    let x = if percent { x * 100.0 } else { x };
    let int_part = codes.split('.').next().unwrap_or("");
    let decimals = codes.split_once('.').map(|(_, d)| d.chars().take_while(|c| "0#?".contains(*c)).count()).unwrap_or(0);
    let sci = codes.contains(['E', 'e']);
    let grouped = int_part.trim_end_matches(',').contains(',');
    // Commas after the last digit scale by thousands ("#,##0,").
    let scale = int_part.len() - int_part.trim_end_matches(',').len();
    let x = x / 1000f64.powi(scale as i32);
    let number = if sci {
        let s = format!("{:.*E}", decimals, x);
        let (m, e) = s.split_once('E').unwrap_or((&s, "0"));
        let e: i32 = e.parse().unwrap_or(0);
        format!("{m}E{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
    } else {
        // Halves round away from zero, as in Excel (Rust rounds them to even).
        let p = 10f64.powi(decimals as i32);
        let s = format!("{:.*}", decimals, (x * p + 1e-9 * x.signum()).round() / p);
        if grouped {
            group(&s)
        } else {
            s
        }
    };
    // Literals before, the number, literals after (and between, for codes split by a literal).
    let mut out = String::new();
    let mut placed = false;
    for i in &its {
        match i {
            Item::Lit(l) => out.push_str(l),
            Item::Code(c) if c.chars().any(|c| "0#?".contains(c)) => {
                if !placed {
                    out.push_str(&number);
                    placed = true;
                }
                if c.contains('%') {
                    out.push('%');
                }
            }
            Item::Code(c) => {
                if c.contains('%') {
                    out.push('%');
                }
            }
        }
    }
    if !placed {
        out.push_str(&number);
    }
    let shown = if minus { -x } else { x };
    (if minus { format!("-{}", out.trim()) } else { out.trim().to_string() }, Some(shown))
}

fn group(s: &str) -> String {
    let (int, frac) = s.split_once('.').map_or((s, None), |(a, b)| (a, Some(b)));
    let mut out = String::new();
    for (k, c) in int.chars().enumerate() {
        if k > 0 && (int.len() - k) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if let Some(f) = frac {
        out.push('.');
        out.push_str(f);
    }
    out
}

/// Excel's General format: whole numbers as they are, others to 10 significant digits.
fn general(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e11 {
        return format!("{}", v as i64);
    }
    if v.abs() >= 1e11 || v.abs() < 1e-9 {
        let s = format!("{:.5E}", v);
        let (m, e) = s.split_once('E').unwrap_or((&s, "0"));
        let e: i32 = e.parse().unwrap_or(0);
        let m = m.trim_end_matches('0').trim_end_matches('.');
        return format!("{m}E{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    let mag = v.abs().log10().floor() as i32;
    let dec = (9 - mag).clamp(0, 10) as usize;
    let s = format!("{:.*}", dec, v);
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/// Days since 1970-01-01 → (year, month, day) (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

fn format_date(v: f64, its: &[Item], date1904: bool) -> String {
    enum T {
        Lit(String),
        Run(char, usize),
        AmPm(bool),
        Elapsed(char),
        Frac(usize),
    }
    let mut ts = Vec::new();
    for i in its {
        match i {
            Item::Lit(l) => ts.push(T::Lit(l.clone())),
            Item::Code(c) => {
                let up = c.to_ascii_uppercase();
                if up == "AM/PM" || up == "A/P" {
                    ts.push(T::AmPm(up == "AM/PM"));
                    continue;
                }
                if let Some(inner) = c.strip_prefix('[') {
                    ts.push(T::Elapsed(inner.chars().next().unwrap_or('h')));
                    continue;
                }
                let b: Vec<char> = c.to_lowercase().chars().collect();
                let mut k = 0;
                while k < b.len() {
                    let ch = b[k];
                    if ch == '.' {
                        let n = b[k + 1..].iter().take_while(|&&x| x == '0').count();
                        ts.push(T::Frac(n));
                        k += 1 + n;
                        continue;
                    }
                    let run = b[k..].iter().take_while(|&&x| x == ch).count();
                    ts.push(T::Run(ch, run));
                    k += run;
                }
            }
        }
    }
    let whole = v.floor() as i64;
    // Days since 1970. 1904 dates count from 1 January 1904; 1900 dates have a 29 February 1900 that never was.
    let days = if date1904 {
        whole - 24_107
    } else if whole > 60 {
        whole - 25_569
    } else {
        whole - 25_568
    };
    let (y, m, d) = civil(days);
    let weekday = (days + 4).rem_euclid(7) as usize;
    let secs = ((v - v.floor()) * 86_400.0).round() as i64;
    let (hh, mm, ss) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let ampm = ts.iter().any(|t| matches!(t, T::AmPm(_)));
    let is_code = |t: &T| matches!(t, T::Run(..) | T::Elapsed(_));
    let mut out = String::new();
    for (idx, t) in ts.iter().enumerate() {
        match t {
            T::Lit(l) => out.push_str(l),
            T::AmPm(full) => out.push_str(match (hh < 12, full) {
                (true, true) => "AM",
                (false, true) => "PM",
                (true, false) => "A",
                (false, false) => "P",
            }),
            T::Elapsed(c) => out.push_str(&match c {
                'h' => ((v * 24.0).floor() as i64).to_string(),
                'm' => ((v * 1440.0).floor() as i64).to_string(),
                _ => ((v * 86_400.0).floor() as i64).to_string(),
            }),
            T::Frac(n) => {
                let frac = v * 86_400.0 - (v * 86_400.0).floor();
                out.push_str(format!("{:.*}", *n, frac).trim_start_matches('0'));
            }
            T::Run(ch, run) => {
                let run = *run;
                match ch {
                    'y' => out.push_str(&if run <= 2 { format!("{:02}", y.rem_euclid(100)) } else { y.to_string() }),
                    'd' => out.push_str(&match run {
                        1 => d.to_string(),
                        2 => format!("{d:02}"),
                        3 => DAYS[weekday][..3].to_string(),
                        _ => DAYS[weekday].to_string(),
                    }),
                    'm' => {
                        // Minutes right after hours or right before seconds; months otherwise.
                        let prev = ts[..idx].iter().rev().find(|t| is_code(t));
                        let next = ts[idx + 1..].iter().find(|t| is_code(t));
                        let minutes = run <= 2
                            && (matches!(prev, Some(T::Run('h', _)) | Some(T::Elapsed('h'))) || matches!(next, Some(T::Run('s', _)) | Some(T::Elapsed('s'))));
                        out.push_str(&if minutes {
                            if run == 1 { mm.to_string() } else { format!("{mm:02}") }
                        } else {
                            match run {
                                1 => m.to_string(),
                                2 => format!("{m:02}"),
                                3 => MONTHS[m as usize - 1][..3].to_string(),
                                5 => MONTHS[m as usize - 1][..1].to_string(),
                                _ => MONTHS[m as usize - 1].to_string(),
                            }
                        });
                    }
                    'h' => {
                        let h = if ampm { (hh + 11) % 12 + 1 } else { hh };
                        out.push_str(&if run == 1 { h.to_string() } else { format!("{h:02}") });
                    }
                    's' => out.push_str(&if run == 1 { ss.to_string() } else { format!("{ss:02}") }),
                    c => out.extend(std::iter::repeat(*c).take(run)),
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------- OpenDocument

/// OpenDocument files keep the text of each paragraph and, in spreadsheets, each cell as shown.
fn odf(path: &str, kind: Kind) -> Result<Vec<Block>, String> {
    let xml = need(path, "content.xml")?;
    let mut out = Vec::new();
    let (mut para, mut depth_p, mut heading) = (String::new(), 0usize, false);
    let mut slide = 0usize;
    // Spreadsheets: the sheet's name, the row's cells, the row and column numbers.
    let mut row = Block { k: "row", t: Vec::new(), refs: Vec::new(), nums: Vec::new() };
    let mut row_no = 0u32;
    let (mut r, mut c, mut cell, mut cell_num, mut repeat, mut in_cell, mut in_table_row) = (0u32, 0u32, String::new(), None, 1u32, false, false);
    let sheet_like = kind == Kind::OdfSheets;
    let mut skip = 0usize;
    for ev in Reader::new(&xml) {
        match ev {
            Ev::Open { name, attrs, empty } => {
                if skip > 0 {
                    skip += !empty as usize;
                    continue;
                }
                match name {
                    // Notes and annotations aren't on the page.
                    "notes" | "annotation" | "tracked-changes" if !empty => skip = 1,
                    "page" if kind == Kind::OdfSlides => {
                        slide += 1;
                        out.push(Block::text("slide", format!("Slide {slide}")));
                    }
                    "table" if sheet_like => {
                        out.push(Block::text("sheet", attr(attrs, "table:name").unwrap_or_default().into_owned()));
                        r = 0;
                    }
                    "table-row" if sheet_like => {
                        row_no = r + 1;
                        r += attr(attrs, "table:number-rows-repeated").and_then(|v| v.parse::<u32>().ok()).unwrap_or(1);
                        c = 0;
                        in_table_row = !empty;
                        row.t.clear();
                        row.refs.clear();
                        row.nums.clear();
                    }
                    "table-cell" | "covered-table-cell" if sheet_like => {
                        repeat = attr(attrs, "table:number-columns-repeated").and_then(|v| v.parse().ok()).unwrap_or(1u32).min(64);
                        cell_num = match attr(attrs, "office:value-type").as_deref() {
                            Some("float" | "currency") => attr(attrs, "office:value").and_then(|v| v.parse::<f64>().ok()),
                            Some("percentage") => attr(attrs, "office:value").and_then(|v| v.parse::<f64>().ok()).map(|v| v * 100.0),
                            _ => None,
                        };
                        cell.clear();
                        in_cell = !empty;
                        if empty {
                            c += repeat;
                        }
                    }
                    "p" | "h" => {
                        if depth_p == 0 {
                            heading = name == "h";
                        }
                        depth_p += !empty as usize;
                        if empty && !in_cell {
                            para.clear();
                        }
                    }
                    "s" => {
                        let n = attr(attrs, "text:c").and_then(|v| v.parse().ok()).unwrap_or(1usize).min(64);
                        para.push_str(&" ".repeat(n));
                    }
                    "tab" | "line-break" => para.push(' '),
                    _ => {}
                }
            }
            Ev::Close(name) => {
                if skip > 0 {
                    skip -= 1;
                    continue;
                }
                match name {
                    "p" | "h" => {
                        depth_p = depth_p.saturating_sub(1);
                        if depth_p == 0 {
                            if in_cell {
                                if !cell.is_empty() {
                                    cell.push(' ');
                                }
                                cell.push_str(&para);
                                para.clear();
                            } else {
                                push_para(&mut out, if heading { "h" } else { "p" }, &mut para);
                            }
                        }
                    }
                    "table-cell" | "covered-table-cell" if sheet_like => {
                        let text = cell.split_whitespace().collect::<Vec<_>>().join(" ");
                        if !text.is_empty() {
                            for k in 0..repeat.min(16) {
                                row.t.push(text.clone());
                                row.refs.push(format!("{}{}", column_name(c + k + 1), row_no));
                                row.nums.push(cell_num);
                            }
                        }
                        c += repeat;
                        in_cell = false;
                    }
                    "table-row" if sheet_like => {
                        if in_table_row && !row.t.is_empty() {
                            out.push(row.clone());
                        }
                        in_table_row = false;
                    }
                    _ => {}
                }
            }
            Ev::Text(t) => {
                if depth_p > 0 && skip == 0 {
                    para.push_str(&t);
                }
            }
        }
    }
    Ok(out)
}

/// 28 → "AB".
pub fn column_name(mut n: u32) -> String {
    let mut s = Vec::new();
    while n > 0 {
        let k = (n - 1) % 26;
        s.push((b'A' + k as u8) as char);
        n = (n - 1) / 26;
    }
    s.iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_numbers_like_excel() {
        assert_eq!(format_value(1234.5, "#,##0.00", false).0, "1,234.50");
        assert_eq!(format_value(-1234.5, "#,##0.00;(#,##0.00)", false).0, "(1,234.50)");
        assert_eq!(format_value(0.256, "0.0%", false), ("25.6%".to_string(), Some(25.6)));
        assert_eq!(format_value(1234.5, "\"₹\"#,##0", false).0, "₹1,235");
        assert_eq!(format_value(3.14159265358979, "General", false).0, "3.141592654");
        assert_eq!(format_value(42.0, "General", false).0, "42");
        assert_eq!(format_value(45306.0, "dd-mm-yyyy", false).0, "15-01-2024");
        assert_eq!(format_value(45306.0, "d-mmm-yy", false).0, "15-Jan-24");
        assert_eq!(format_value(45306.75, "h:mm AM/PM", false).0, "6:00 PM");
        assert_eq!(format_value(0.5, "hh:mm:ss", false).0, "12:00:00");
        assert_eq!(format_value(1234567.0, "0.00E+00", false).0, "1.23E+06");
        assert_eq!(column_name(28), "AB");
        assert_eq!(column_of("AB12"), 28);
    }
}
