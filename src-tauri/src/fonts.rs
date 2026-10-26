//! Fonts for themes: the families installed in Windows (or known to fontconfig on Linux), and font files the user added (kept in Paginaut's data
//! folder, read by the page as bytes).

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct UserFont {
    /// Shown name (the file's name without its extension).
    pub name: String,
    /// File name inside the fonts folder.
    pub file: String,
}

const EXTS: [&str; 4] = ["ttf", "otf", "woff", "woff2"];

/// Families installed for everyone and for this user, sorted, styles folded in ("Georgia Bold Italic" → "Georgia").
pub fn installed() -> Vec<String> {
    let mut names = Vec::new();
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            names.extend(reg_values(root, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts"));
        }
    }
    // Linux: fontconfig's list (every desktop has it); a family's first name, the others being translations.
    #[cfg(not(windows))]
    if let Ok(out) = std::process::Command::new("fc-list").args([":", "family"]).output() {
        names.extend(String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.split(',').next()).map(|f| f.trim().replace('\\', "")).filter(|f| !f.is_empty()));
    }
    let mut out: Vec<String> = names.iter().flat_map(|n| families(n)).collect();
    out.sort_by_key(|s| s.to_lowercase());
    out.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    out
}

/// "Cambria & Cambria Math (TrueType)" → ["Cambria", "Cambria Math"]; style words at the end dropped.
fn families(value: &str) -> Vec<String> {
    const STYLES: [&str; 18] = [
        "Regular", "Bold", "Italic", "Oblique", "Light", "Semilight", "SemiLight", "Semibold", "SemiBold", "Black", "Thin", "Medium", "Heavy",
        "ExtraBold", "ExtraLight", "Demibold", "Book", "Condensed",
    ];
    let base = value.split(" (").next().unwrap_or(value);
    base.split(" & ")
        .filter_map(|f| {
            let mut words: Vec<&str> = f.split_whitespace().collect();
            while words.len() > 1 && STYLES.contains(words.last().unwrap()) {
                words.pop();
            }
            let name = words.join(" ");
            // Symbol and system-only fonts aren't useful for labels.
            let skip = name.is_empty() || name.starts_with('@') || ["Marlett", "Symbol", "Webdings", "Wingdings", "MT Extra", "Segoe MDL2 Assets", "Segoe Fluent Icons", "HoloLens MDL2 Assets"].contains(&name.as_str());
            (!skip).then_some(name)
        })
        .collect()
}

#[cfg(windows)]
fn reg_values(root: windows::Win32::System::Registry::HKEY, key: &str) -> Vec<String> {
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::System::Registry::{RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, KEY_READ};
    let mut out = Vec::new();
    let mut h = HKEY::default();
    unsafe {
        if RegOpenKeyExW(root, &HSTRING::from(key), None, KEY_READ, &mut h).is_err() {
            return out;
        }
        for i in 0.. {
            let mut name = [0u16; 512];
            let mut len = name.len() as u32;
            if RegEnumValueW(h, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None).is_err() {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
        }
        let _ = RegCloseKey(h);
    }
    out
}

/// The installed TrueType file of the font a PDF names ("Arial-BoldMT", "TimesNewRomanPS-ItalicMT", "Calibri,Bold"),
/// in that style; `exact` false when only a stand-in was found (Linux: fontconfig's choice of another family).
pub struct Found {
    pub path: PathBuf,
    pub family: String,
    pub exact: bool,
}

/// "TimesNewRomanPS-BoldItalicMT" → ("timesnewroman", "bolditalic"); styles from `bold`/`italic` when the name has none.
fn pdf_font_key(name: &str, bold: bool, italic: bool) -> (String, String) {
    let name = match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 => rest,
        _ => name,
    };
    let (family, style) = name.split_once([',', '-']).unwrap_or((name, ""));
    let norm = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
    let trim = |mut s: String| {
        for suffix in ["psmt", "mt", "ps"] {
            if s.len() > suffix.len() + 2 && s.ends_with(suffix) {
                s.truncate(s.len() - suffix.len());
                break;
            }
        }
        s
    };
    let family = trim(norm(family));
    let mut style = trim(norm(style)).replace("oblique", "italic");
    if style.is_empty() || style == "regular" || style == "roman" {
        style = match (bold, italic) {
            (true, true) => "bolditalic",
            (true, false) => "bold",
            (false, true) => "italic",
            _ => "",
        }
        .into();
    }
    (family, style)
}

/// A TrueType file its licence lets be embedded in documents: OS/2 fsType not "restricted" (0x0002) and not
/// "bitmaps only" (0x0200). Fonts without an OS/2 table are taken as installable.
fn embeddable_truetype(p: &Path) -> bool {
    let Ok(data) = std::fs::read(p) else { return false };
    if data.len() < 12 || !(data[..4] == [0, 1, 0, 0] || &data[..4] == b"true") {
        return false;
    }
    let u16_at = |at: usize| data.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
    let u32_at = |at: usize| data.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let tables = u16_at(4).unwrap_or(0) as usize;
    for i in 0..tables {
        let entry = 12 + i * 16;
        if data.get(entry..entry + 4) == Some(b"OS/2") {
            let fs_type = u32_at(entry + 8).and_then(|offset| u16_at(offset + 8)).unwrap_or(0);
            return fs_type & 0x000f != 0x0002 && fs_type & 0x0200 == 0;
        }
    }
    true
}

#[cfg(windows)]
pub fn find(name: &str, bold: bool, italic: bool) -> Option<Found> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let (family, style) = pdf_font_key(name, bold, italic);
    let fonts_dir = PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
    let wanted: Vec<String> = if style.is_empty() { vec![family.clone(), format!("{family}regular")] } else { vec![format!("{family}{style}")] };
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for (value, data) in reg_pairs(root, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts") {
            let full = value.split(" (").next().unwrap_or(&value);
            // One file can hold several faces ("Cambria & Cambria Math"): collections aren't PDF fonts anyway.
            let key: String = full.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
            if !wanted.contains(&key) {
                continue;
            }
            let path = if data.contains(['\\', '/']) { PathBuf::from(&data) } else { fonts_dir.join(&data) };
            if embeddable_truetype(&path) {
                return Some(Found { path, family: full.to_string(), exact: true });
            }
        }
    }
    None
}

#[cfg(not(windows))]
pub fn find(name: &str, bold: bool, italic: bool) -> Option<Found> {
    let (family, style) = pdf_font_key(name, bold, italic);
    // fontconfig wants the family with its spaces: "TimesNewRoman" → "Times New Roman".
    let raw = name.split_once('+').filter(|(t, _)| t.len() == 6).map_or(name, |(_, r)| r);
    let raw = raw.split([',', '-']).next().unwrap_or(raw).trim_end_matches("PSMT").trim_end_matches("MT").trim_end_matches("PS");
    let mut spaced = String::new();
    for (i, ch) in raw.chars().enumerate() {
        if i > 0 && ch.is_ascii_uppercase() && !raw[..i].ends_with(|c: char| c.is_ascii_uppercase() || c == ' ') {
            spaced.push(' ');
        }
        spaced.push(ch);
    }
    let pattern = format!("{spaced}:style={}", match style.as_str() { "bold" => "Bold", "italic" => "Italic", "bolditalic" => "Bold Italic", _ => "Regular" });
    let out = std::process::Command::new("fc-match").args(["-f", "%{file}\n%{family}\n", &pattern]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let path = PathBuf::from(lines.next()?);
    let got = lines.next()?.split(',').next()?.to_string();
    let norm = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
    embeddable_truetype(&path).then(|| Found { exact: norm(&got) == family, path, family: got })
}

#[cfg(windows)]
fn reg_pairs(root: windows::Win32::System::Registry::HKEY, key: &str) -> Vec<(String, String)> {
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::System::Registry::{RegCloseKey, RegEnumValueW, RegOpenKeyExW, HKEY, KEY_READ};
    let mut out = Vec::new();
    let mut h = HKEY::default();
    unsafe {
        if RegOpenKeyExW(root, &HSTRING::from(key), None, KEY_READ, &mut h).is_err() {
            return out;
        }
        for i in 0.. {
            let mut name = [0u16; 512];
            let mut len = name.len() as u32;
            let mut data = [0u16; 1024];
            let mut size = (data.len() * 2) as u32;
            let mut kind = 0u32;
            if RegEnumValueW(h, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, Some(&mut kind as *mut u32), Some(data.as_mut_ptr().cast()), Some(&mut size as *mut u32)).is_err() {
                break;
            }
            let chars = (size as usize / 2).min(data.len());
            let value = String::from_utf16_lossy(&data[..chars]).trim_end_matches('\0').to_string();
            out.push((String::from_utf16_lossy(&name[..len as usize]), value));
        }
        let _ = RegCloseKey(h);
    }
    out
}

fn is_font(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

pub fn user(dir: &Path) -> Vec<UserFont> {
    let mut out: Vec<UserFont> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| is_font(p))
                .filter_map(|p| Some(UserFont { name: p.file_stem()?.to_string_lossy().into_owned(), file: p.file_name()?.to_string_lossy().into_owned() }))
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|f| f.name.to_lowercase());
    out
}

/// Copies a font file into the fonts folder (a free name if one is taken).
pub fn add(dir: &Path, from: &str) -> Result<UserFont, String> {
    let src = Path::new(from);
    if !is_font(src) {
        return Err("That isn't a font file (.ttf, .otf, .woff or .woff2).".into());
    }
    let len = std::fs::metadata(src).map_err(|e| e.to_string())?.len();
    if len > 30 << 20 {
        return Err("That font file is too large.".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Font".into());
    let ext = src.extension().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let mut to = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while to.exists() {
        to = dir.join(format!("{stem} {n}.{ext}"));
        n += 1;
    }
    std::fs::copy(src, &to).map_err(|e| format!("Couldn't copy the font: {e}"))?;
    Ok(UserFont { name: to.file_stem().unwrap().to_string_lossy().into_owned(), file: to.file_name().unwrap().to_string_lossy().into_owned() })
}

/// A file of the fonts folder by its name (nothing outside it).
pub fn file(dir: &Path, name: &str) -> Result<PathBuf, String> {
    let p = dir.join(name);
    if name.contains(['/', '\\']) || name.contains("..") || !is_font(&p) || !p.is_file() {
        return Err("No such font.".into());
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    #[test]
    fn families_from_registry_names() {
        assert_eq!(super::families("Georgia Bold Italic (TrueType)"), vec!["Georgia"]);
        assert_eq!(super::families("Cambria & Cambria Math (TrueType)"), vec!["Cambria", "Cambria Math"]);
        assert_eq!(super::families("Segoe UI Semibold (TrueType)"), vec!["Segoe UI"]);
        assert!(super::families("Wingdings (TrueType)").is_empty());
    }
}
