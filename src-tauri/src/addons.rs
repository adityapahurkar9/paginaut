//! The add-on list: which OCR language models Paginaut offers (and a LibreOffice release it's known to work
//! with), published with each release as `addons.json` and signed with the release key, so they can change
//! without a new Paginaut. Until a list has been downloaded and checked, the one built in is used; a checked
//! list is kept in the app's data folder for use offline. A list can only name files from the places Paginaut
//! already downloads from (Tesseract's model repositories at a tag, LibreOffice's stable folder).

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Addons {
    pub schema: u32,
    #[serde(default)]
    pub ocr: Option<OcrList>,
    #[serde(default)]
    pub libreoffice: Option<LibrePin>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct OcrList {
    /// The tag of Tesseract's tessdata_fast and tessdata_best repositories the models come from.
    pub tag: String,
    pub langs: Vec<OcrLang>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct OcrLang {
    pub code: String,
    pub name: String,
    /// Size in bytes and git blob SHA-1 of the fast and best models.
    pub fast: (u64, String),
    pub best: (u64, String),
}

/// A LibreOffice release to set up instead of the newest (one Paginaut is known to work with), and its SHA-256.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct LibrePin {
    pub version: String,
    pub sha256: String,
}

const SCHEMA: u32 = 1;

/// The checked list in use, with its OCR languages ready to hand out.
struct Loaded {
    ocr: Option<Arc<OcrList>>,
    libreoffice: Option<LibrePin>,
}

impl From<Addons> for Loaded {
    fn from(a: Addons) -> Self {
        Loaded { ocr: a.ocr.map(Arc::new), libreoffice: a.libreoffice }
    }
}

static LIST: Lazy<RwLock<Option<Loaded>>> = Lazy::new(|| RwLock::new(None));

static BUILTIN_OCR: Lazy<Arc<OcrList>> = Lazy::new(|| {
    Arc::new(OcrList {
        tag: crate::ocr_langs::TAG.to_string(),
        langs: crate::ocr_langs::LANGS
            .iter()
            .map(|l| OcrLang { code: l.code.into(), name: l.name.into(), fast: (l.fast.0, l.fast.1.into()), best: (l.best.0, l.best.1.into()) })
            .collect(),
    })
});

/// The OCR languages on offer: the checked list's, or the built-in one.
pub fn ocr() -> Arc<OcrList> {
    LIST.read().as_ref().and_then(|a| a.ocr.clone()).unwrap_or_else(|| BUILTIN_OCR.clone())
}

/// The LibreOffice release to set up, when the list names one.
pub fn libreoffice() -> Option<LibrePin> {
    LIST.read().as_ref().and_then(|a| a.libreoffice.clone())
}

/// The list built into this version, as `addons.json` (for publishing with a release).
pub fn builtin_json() -> String {
    let a = Addons { schema: SCHEMA, ocr: Some((*BUILTIN_OCR.clone()).clone()), libreoffice: None };
    serde_json::to_string_pretty(&a).unwrap_or_default()
}

fn hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|c| c.is_ascii_hexdigit())
}

/// Only names and numbers that fit where they go (they end up in download paths).
fn valid(a: &Addons) -> bool {
    let word = |s: &str, extra: &[u8]| !s.is_empty() && s.len() < 40 && s.bytes().all(|c| c.is_ascii_alphanumeric() || extra.contains(&c));
    a.schema == SCHEMA
        && a.ocr.as_ref().is_none_or(|o| {
            word(&o.tag, b"._-") && !o.langs.is_empty() && o.langs.iter().all(|l| word(&l.code, b"_") && !l.name.is_empty() && l.name.len() < 80 && hex(&l.fast.1, 40) && hex(&l.best.1, 40) && l.fast.0 < 200 << 20 && l.best.0 < 200 << 20)
        })
        && a.libreoffice.as_ref().is_none_or(|p| {
            let nums: Vec<&str> = p.version.split('.').collect();
            nums.len() == 3 && nums.iter().all(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit())) && hex(&p.sha256, 64)
        })
}

fn parse_checked(json: &str, sig: &str) -> Result<Addons, String> {
    crate::update::verify(json.as_bytes(), sig)?;
    let a: Addons = serde_json::from_str(json).map_err(|_| "The add-on list couldn't be read.".to_string())?;
    if !valid(&a) {
        return Err("The add-on list isn't one this version can use.".into());
    }
    Ok(a)
}

/// The list kept from last time, if it's still there and still checks out.
pub fn load_cached(dir: &Path) {
    let (Ok(json), Ok(sig)) = (std::fs::read_to_string(dir.join("addons.json")), std::fs::read_to_string(dir.join("addons.json.sig"))) else { return };
    if let Ok(a) = parse_checked(&json, &sig) {
        *LIST.write() = Some(a.into());
    }
}

/// Downloads the newest release's list, checks it, and keeps it. Ok(false): nothing published yet.
pub fn refresh(dir: &Path, cancel: &AtomicBool) -> Result<bool, String> {
    let (Some(json), Some(sig)) = (crate::update::latest_text("addons.json", cancel)?, crate::update::latest_text("addons.json.sig", cancel)?) else {
        return Ok(false);
    };
    let a = parse_checked(&json, &sig)?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("addons.json"), &json).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("addons.json.sig"), &sig).map_err(|e| e.to_string())?;
    *LIST.write() = Some(a.into());
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_list_is_valid_and_round_trips() {
        let a: Addons = serde_json::from_str(&builtin_json()).unwrap();
        assert!(valid(&a));
        assert!(a.ocr.unwrap().langs.iter().any(|l| l.code == "eng"));
    }

    #[test]
    fn rejects_lists_that_reach_elsewhere() {
        let mut a: Addons = serde_json::from_str(&builtin_json()).unwrap();
        a.ocr.as_mut().unwrap().tag = "../../evil".into();
        assert!(!valid(&a));
        let mut b: Addons = serde_json::from_str(&builtin_json()).unwrap();
        b.ocr.as_mut().unwrap().langs[0].code = "eng/../../x".into();
        assert!(!valid(&b));
        let mut c: Addons = serde_json::from_str(&builtin_json()).unwrap();
        c.libreoffice = Some(LibrePin { version: "25.8.2/../..".into(), sha256: "0".repeat(64) });
        assert!(!valid(&c));
        c.libreoffice = Some(LibrePin { version: "25.8.2".into(), sha256: "0".repeat(64) });
        assert!(valid(&c));
    }

    #[test]
    fn unsigned_lists_are_refused() {
        assert!(parse_checked(&builtin_json(), "bm90IGEgc2lnbmF0dXJl").is_err());
    }
}
