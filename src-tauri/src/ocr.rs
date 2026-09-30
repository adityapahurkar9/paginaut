//! OCR language models: which are available (built in, or downloaded into the
//! app's local data folder), resolving a choice of languages to model files, and
//! downloading more from Tesseract's own repositories, checked against the sizes
//! and hashes in `ocr_langs` before they are used.

use crate::engine::ocr::Models;
use crate::ocr_langs::{Lang, LANGS, TAG};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quality {
    /// Tesseract's "fast" models: small and quick, what ships with Paginaut.
    Fast,
    /// "best" models: most accurate, several times larger and slower.
    Best,
}

impl Quality {
    pub fn parse(s: &str) -> Quality {
        if s == "best" {
            Quality::Best
        } else {
            Quality::Fast
        }
    }
    fn dir(self) -> &'static str {
        match self {
            Quality::Fast => "fast",
            Quality::Best => "best",
        }
    }
    fn repo(self) -> &'static str {
        match self {
            Quality::Fast => "tessdata_fast",
            Quality::Best => "tessdata_best",
        }
    }
}

/// Where models live: `builtin` ships with the app (fast models only), `user`
/// holds downloads in fast/ and best/ subfolders.
#[derive(Clone)]
pub struct ModelDirs {
    pub builtin: PathBuf,
    pub user: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LangInfo {
    code: &'static str,
    name: &'static str,
    fast_size: u64,
    best_size: u64,
    /// The fast model ships with Paginaut.
    builtin: bool,
    /// Downloaded models.
    fast: bool,
    best: bool,
}

impl ModelDirs {
    fn downloaded(&self, code: &str, q: Quality) -> PathBuf {
        self.user.join(q.dir()).join(format!("{code}.traineddata"))
    }

    fn model(&self, code: &str, q: Quality) -> Option<PathBuf> {
        let builtin = self.builtin.join(format!("{code}.traineddata"));
        if q == Quality::Fast && builtin.is_file() {
            return Some(builtin);
        }
        let p = self.downloaded(code, q);
        p.is_file().then_some(p)
    }

    pub fn languages(&self) -> Vec<LangInfo> {
        LANGS
            .iter()
            .map(|l| LangInfo {
                code: l.code,
                name: l.name,
                fast_size: l.fast.0,
                best_size: l.best.0,
                builtin: self.builtin.join(format!("{}.traineddata", l.code)).is_file(),
                fast: self.downloaded(l.code, Quality::Fast).is_file(),
                best: self.downloaded(l.code, Quality::Best).is_file(),
            })
            .collect()
    }

    /// The model files for `langs` at quality `q`; Err names what must be downloaded first.
    pub fn models(&self, langs: &[String], q: Quality) -> Result<Models, String> {
        if langs.is_empty() {
            return Err("Choose at least one language.".into());
        }
        let mut files = Vec::new();
        let mut missing = Vec::new();
        for code in langs {
            let lang = find(code)?;
            match self.model(code, q) {
                Some(p) => files.push((code.clone(), p.to_string_lossy().into_owned())),
                None => missing.push(lang.name),
            }
        }
        if !missing.is_empty() {
            return Err(format!("Download these languages first: {}", missing.join(", ")));
        }
        Ok(Models { langs: langs.join("+"), files })
    }

    pub fn delete(&self, code: &str, q: Quality) -> Result<(), String> {
        find(code)?;
        let p = self.downloaded(code, q);
        if p.is_file() {
            std::fs::remove_file(&p).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Downloads a model (into a temporary file, renamed once its size and hash match).
    pub fn download(&self, code: &str, q: Quality, cancel: &AtomicBool, mut progress: impl FnMut(u64, u64)) -> Result<(), String> {
        let lang = find(code)?;
        let (size, sha) = match q {
            Quality::Fast => lang.fast,
            Quality::Best => lang.best,
        };
        let dest = self.downloaded(code, q);
        let dir = dest.parent().unwrap();
        std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
        let part = dest.with_extension("part");
        let path = format!("/tesseract-ocr/{}/{TAG}/{code}.traineddata", q.repo());
        let get = crate::net::Get { host: "raw.githubusercontent.com", path: &path, cancel, progress: &mut progress, expect: Some(size), max: size, from: 0, min_rate: 0 };
        let res = crate::net::to_file(get, &part, None).and_then(|_| {
            let got = git_blob_sha1(&part)?;
            if got != sha {
                return Err(format!("The downloaded {} model didn't match the expected one; try again.", lang.name));
            }
            std::fs::rename(&part, &dest).map_err(|e| e.to_string())
        });
        if res.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        res
    }
}

fn find(code: &str) -> Result<&'static Lang, String> {
    LANGS.iter().find(|l| l.code == code).ok_or_else(|| format!("unknown language {code}"))
}

/// Downloads in progress, so they can be stopped: "code/quality" → stop flag.
pub static DOWNLOADS: once_cell::sync::Lazy<Mutex<HashMap<String, Arc<AtomicBool>>>> = once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));

pub fn stop_download(code: &str, q: Quality) {
    if let Some(flag) = DOWNLOADS.lock().get(&format!("{code}/{}", q.dir())) {
        flag.store(true, Ordering::Release);
    }
}

pub fn download_key(code: &str, q: Quality) -> String {
    format!("{code}/{}", q.dir())
}

/// Git's hash of a file ("blob <size>\0" + contents), as GitHub lists it.
fn git_blob_sha1(path: &Path) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut h = crate::net::Hasher::new(crate::net::Alg::Sha1)?;
    h.update(format!("blob {}\0", data.len()).as_bytes())?;
    h.update(&data)?;
    Ok(crate::net::hex(&h.finish()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Network test: cargo test --lib ocr::tests -- --ignored
    #[test]
    #[ignore]
    fn downloads_and_checks_a_model() {
        let dir = std::env::temp_dir().join("paginaut-ocr-test");
        let _ = std::fs::remove_dir_all(&dir);
        let dirs = ModelDirs { builtin: dir.join("builtin"), user: dir.join("user") };
        let stop = AtomicBool::new(false);
        let mut calls = 0;
        dirs.download("tir", Quality::Fast, &stop, |_, _| calls += 1).unwrap();
        assert!(calls > 0);
        let m = dirs.models(&["tir".into()], Quality::Fast).unwrap();
        assert_eq!(m.langs, "tir");
        assert!(dirs.models(&["tir".into()], Quality::Best).is_err());
        // A changed file doesn't pass the check.
        let p = dirs.downloaded("tir", Quality::Fast);
        let mut data = std::fs::read(&p).unwrap();
        data[100] ^= 1;
        std::fs::write(&p, data).unwrap();
        assert_ne!(git_blob_sha1(&p).unwrap(), find("tir").unwrap().fast.1);
        dirs.delete("tir", Quality::Fast).unwrap();
        assert!(!p.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
