//! Office files (Word, Excel, PowerPoint, OpenDocument) as PDFs. Converters, best first:
//! - Microsoft Office, when installed: the documents look exactly as their authors saw them;
//! - LibreOffice: an installed copy, or Paginaut's own pack (downloaded on request, ~360 MB);
//! - MuPDF's built-in reader (always there): the text only, for .docx/.xlsx/.pptx.
//! The Office quality test (tools/office) measured the difference. PDFs made are kept in a cache folder, so
//! reopening a file (or taking pages from it) doesn't convert it again.

pub mod com;
pub mod libre;

use crate::engine::Converted;
use parking_lot::RwLock;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a conversion may take before Paginaut stops waiting.
const TIMEOUT: Duration = Duration::from_secs(180);

/// Which Office program a file belongs to (by its extension).
pub fn kind(path: &str) -> Option<com::App> {
    let ext = path.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "doc" | "docx" | "docm" | "dot" | "dotx" | "dotm" | "rtf" | "odt" | "ott" => Some(com::App::Word),
        "xls" | "xlsx" | "xlsm" | "xlsb" | "xlt" | "xltx" | "ods" | "ots" => Some(com::App::Excel),
        "ppt" | "pptx" | "pptm" | "pps" | "ppsx" | "pot" | "potx" | "odp" | "otp" => Some(com::App::PowerPoint),
        _ => None,
    }
}

/// MuPDF reads these itself (text only).
fn builtin_reads(path: &str) -> bool {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "docx" | "xlsx" | "pptx")
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MsOffice {
    pub word: bool,
    pub excel: bool,
    pub powerpoint: bool,
    pub version: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LoInfo {
    pub version: String,
    pub path: String,
    /// Disk space used (the pack only).
    pub size: u64,
}

/// What can convert Office files on this PC.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Engines {
    pub msoffice: Option<MsOffice>,
    pub installed: Option<LoInfo>,
    pub pack: Option<LoInfo>,
    /// "auto", "msoffice", "libreoffice" or "builtin".
    pub preference: String,
}

pub struct Office {
    /// Paginaut's LibreOffice pack.
    pack_dir: PathBuf,
    /// LibreOffice profiles (one per LibreOffice) and the download while setting up.
    data: PathBuf,
    /// PDFs made from Office files.
    cache: PathBuf,
    preference: RwLock<String>,
    /// Detected once (asking Windows is quick, but not free); refreshed after installing or removing.
    found: RwLock<Option<(Option<MsOffice>, Option<libre::Lo>)>>,
}

impl Office {
    pub fn new(local_data: &Path) -> Office {
        let cache = local_data.join("converted");
        clean_cache(&cache);
        Office {
            pack_dir: local_data.join("libreoffice"),
            data: local_data.to_path_buf(),
            cache,
            preference: RwLock::new("auto".into()),
            found: RwLock::new(None),
        }
    }

    pub fn set_preference(&self, p: &str) {
        if matches!(p, "auto" | "msoffice" | "libreoffice" | "builtin") {
            *self.preference.write() = p.into();
        }
    }

    fn detect(&self) -> (Option<MsOffice>, Option<libre::Lo>) {
        if let Some(f) = self.found.read().clone() {
            return f;
        }
        let (word, excel, powerpoint) = (com::installed(com::App::Word), com::installed(com::App::Excel), com::installed(com::App::PowerPoint));
        let ms = (word || excel || powerpoint).then(|| MsOffice { word, excel, powerpoint, version: office_version() });
        let f = (ms, libre::installed());
        *self.found.write() = Some(f.clone());
        f
    }

    pub fn forget(&self) {
        *self.found.write() = None;
    }

    pub fn engines(&self) -> Engines {
        let (ms, lo) = self.detect();
        let pack = libre::pack(&self.pack_dir).map(|p| LoInfo { version: p.version, path: self.pack_dir.to_string_lossy().into_owned(), size: libre::size_of(&self.pack_dir) });
        let installed = lo.map(|l| LoInfo { version: l.version, path: l.soffice.parent().and_then(|p| p.parent()).unwrap_or(&l.soffice).to_string_lossy().into_owned(), size: 0 });
        Engines { msoffice: ms, installed, pack, preference: self.preference.read().clone() }
    }

    /// The LibreOffice to use: the pack (it has Office's cloud fonts), else an installed one.
    fn libreoffice(&self) -> Option<libre::Lo> {
        libre::pack(&self.pack_dir).or_else(|| self.detect().1)
    }

    /// Converts with `engine` ("msoffice", "libreoffice"), or the preferred available one. None: only the
    /// built-in text preview is left (or wanted).
    fn choose(&self, path: &str, engine: Option<&str>) -> Result<Option<&'static str>, String> {
        let app = kind(path).ok_or("not an Office file")?;
        let has_ms = self.detect().0.is_some_and(|m| match app {
            com::App::Word => m.word,
            com::App::Excel => m.excel,
            com::App::PowerPoint => m.powerpoint,
        });
        let has_lo = self.libreoffice().is_some();
        let pref = engine.map(str::to_string).unwrap_or_else(|| self.preference.read().clone());
        let pick = match pref.as_str() {
            "msoffice" if has_ms => Some("msoffice"),
            "libreoffice" if has_lo => Some("libreoffice"),
            "builtin" if builtin_reads(path) => None,
            // Asked for by name for this file, but not there.
            _ if engine.is_some() && pref != "auto" => return Err(format!("{} isn't available.", engine_name(&pref, app))),
            _ if has_ms => Some("msoffice"),
            _ if has_lo => Some("libreoffice"),
            _ => None,
        };
        if pick.is_none() && !builtin_reads(path) {
            return Err(format!(
                "Opening {} files needs Microsoft Office or LibreOffice. Get LibreOffice (free) in Tools › Convert › Office suite choices.",
                path.rsplit('.').next().unwrap_or("these").to_ascii_uppercase()
            ));
        }
        Ok(pick)
    }

    /// The engine hook: a PDF made from an Office file (from the cache when it's still current).
    pub fn convert(&self, path: &str, engine: Option<&str>) -> Result<Option<Converted>, String> {
        let Some(app) = kind(path) else { return Ok(None) };
        let Some(which) = self.choose(path, engine)? else { return Ok(None) };
        let meta = std::fs::metadata(path).map_err(|e| format!("Couldn't open {}: {e}", file_name(path)))?;
        let stamp = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis());
        std::fs::create_dir_all(&self.cache).map_err(|e| e.to_string())?;
        let key = fnv(&format!("{}|{}|{stamp}|{which}", path.to_lowercase(), meta.len()));
        let pdf = self.cache.join(format!("{key:016x}.pdf"));
        let name = engine_name(which, app);
        if !pdf.is_file() {
            // Its own temporary name: the same file may be opening in two places at once.
            static RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let part = self.cache.join(format!("{key:016x}-{}.part.pdf", RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
            let res = match which {
                "msoffice" => com::convert(app, path, &part.to_string_lossy(), TIMEOUT),
                _ => {
                    let lo = self.libreoffice().ok_or("LibreOffice isn't available.")?;
                    let profile = self.data.join(if lo.pack { "lo-profile" } else { "lo-profile-installed" });
                    libre::convert(&lo, &profile, path, &part, TIMEOUT)
                }
            };
            if let Err(e) = res {
                let _ = std::fs::remove_file(&part);
                return Err(format!("{name} couldn't convert {}: {e}", file_name(path)));
            }
            // Another conversion of the same file may have finished first (and be open): keep that one.
            if pdf.is_file() {
                let _ = std::fs::remove_file(&part);
            } else {
                std::fs::rename(&part, &pdf).map_err(|e| e.to_string())?;
            }
        }
        Ok(Some(Converted { from: path.to_string(), engine: which.into(), engine_name: name, pdf: pdf.to_string_lossy().into_owned() }))
    }

    /// Converts an Office file into `out` (a PDF of the user's). Returns the converter's name.
    pub fn convert_to(&self, path: &str, out: &str, engine: Option<&str>) -> Result<String, String> {
        if kind(path).is_none() {
            return Err(format!("{} isn't an Office file.", file_name(path)));
        }
        let c = self.convert(path, engine)?.ok_or_else(|| format!("{} needs Microsoft Office or LibreOffice to become a PDF.", file_name(path)))?;
        std::fs::copy(&c.pdf, out).map_err(|e| format!("Couldn't write {}: {e}", file_name(out)))?;
        Ok(c.engine_name)
    }

    /// A PDF as an editable presentation, by LibreOffice (a slide per page, text and shapes kept).
    pub fn pdf_to_slides(&self, pdf: &str, out: &Path) -> Result<(), String> {
        let lo = self.libreoffice().ok_or("LibreOffice isn't available.")?;
        let profile = self.data.join(if lo.pack { "lo-profile" } else { "lo-profile-installed" });
        libre::convert_as(&lo, &profile, pdf, out, Some("impress_pdf_import"), "pptx", TIMEOUT).map_err(|e| format!("LibreOffice couldn't make the slides: {e}"))
    }

    pub fn pack_dir(&self) -> &Path {
        &self.pack_dir
    }

    pub fn install_pack(&self, cancel: &std::sync::atomic::AtomicBool, progress: &mut dyn FnMut(libre::Stage, u64, u64)) -> Result<(), String> {
        let _ = std::fs::remove_dir_all(self.data.join("lo-profile"));
        // The installer goes to the temp folder: Windows Installer refuses to open packages kept in the app's
        // data folder (error 1619), though it unpacks into it.
        let work = std::env::temp_dir().join("paginaut-libreoffice");
        let r = libre::install(&self.pack_dir, &work, &self.data.join("lo-profile"), cancel, progress);
        let _ = std::fs::remove_dir_all(&work);
        self.forget();
        r.map(drop)
    }

    pub fn remove_pack(&self) -> Result<(), String> {
        // Not while it's converting.
        let _one = libre::BUSY.lock();
        for d in [self.pack_dir.clone(), self.data.join("lo-profile"), self.pack_dir.with_extension("part")] {
            if d.exists() {
                std::fs::remove_dir_all(&d).map_err(|e| format!("Couldn't remove {}: {e}", d.display()))?;
            }
        }
        self.forget();
        Ok(())
    }
}

pub fn engine_name(which: &str, app: com::App) -> String {
    match which {
        "msoffice" => app.name().into(),
        "libreoffice" => "LibreOffice".into(),
        _ => "Paginaut's text preview".into(),
    }
}

fn file_name(p: &str) -> String {
    Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string())
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

/// PDFs made more than a week ago go (they're remade when needed).
fn clean_cache(dir: &Path) {
    let Ok(list) = std::fs::read_dir(dir) else { return };
    let week = std::time::SystemTime::now() - Duration::from_secs(7 * 86400);
    for e in list.flatten() {
        if e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < week) || e.file_name().to_string_lossy().contains(".part") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// "Microsoft 365 (16.0.20430)" or similar, from Office's own registry entries.
fn office_version() -> String {
    #[cfg(windows)]
    {
        use windows::core::HSTRING;
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
        let read = |key: &str, value: &str| -> Option<String> {
            let mut buf = [0u16; 256];
            let mut len = (buf.len() * 2) as u32;
            unsafe { RegGetValueW(HKEY_LOCAL_MACHINE, &HSTRING::from(key), &HSTRING::from(value), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len)) }.ok().ok()?;
            Some(String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
        };
        let c2r = r"SOFTWARE\Microsoft\Office\ClickToRun\Configuration";
        if let Some(v) = read(c2r, "VersionToReport") {
            let product = read(c2r, "ProductReleaseIds").unwrap_or_default();
            let name = if product.contains("O365") { "Microsoft 365" } else { "Microsoft Office" };
            return format!("{name} ({v})");
        }
    }
    "Microsoft Office".into()
}
