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
