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
        let res = net::get("raw.githubusercontent.com", &path, &part, size, cancel, &mut progress).and_then(|()| {
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
    let mut input = format!("blob {}\0", data.len()).into_bytes();
    input.extend_from_slice(&data);
    let digest = hash::sha1(&input)?;
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(windows)]
mod hash {
    use windows::Win32::Security::Cryptography::{BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData, BCRYPT_HASH_HANDLE, BCRYPT_SHA1_ALG_HANDLE};

    pub fn sha1(data: &[u8]) -> Result<[u8; 20], String> {
        let mut h = BCRYPT_HASH_HANDLE::default();
        let mut out = [0u8; 20];
        unsafe {
            BCryptCreateHash(BCRYPT_SHA1_ALG_HANDLE, &mut h, None, None, 0).ok().map_err(|e| e.to_string())?;
            let r = BCryptHashData(h, data, 0).ok().and_then(|_| BCryptFinishHash(h, &mut out, 0).ok());
            let _ = BCryptDestroyHash(h);
            r.map_err(|e| e.to_string())?;
        }
        Ok(out)
    }
}

#[cfg(not(windows))]
mod hash {
    pub fn sha1(_: &[u8]) -> Result<[u8; 20], String> {
        Err("not supported on this system".into())
    }
}

/// HTTPS downloads through Windows' own HTTP stack (WinHTTP): the system's proxy
/// settings and certificate store apply, and no HTTP library ships with the app.
#[cfg(windows)]
mod net {
    use std::io::Write;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Networking::WinHttp::*;

    struct Handle(*mut core::ffi::c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    fn open(h: *mut core::ffi::c_void, what: &str) -> Result<Handle, String> {
        if h.is_null() {
            Err(format!("{what}: {}", std::io::Error::last_os_error()))
        } else {
            Ok(Handle(h))
        }
    }

    pub fn get(host: &str, path: &str, dest: &Path, expect: u64, cancel: &AtomicBool, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
        let offline = |e: windows::core::Error| format!("Couldn't download (are you online?): {}", e.message());
        unsafe {
            let session = open(WinHttpOpen(w!("Paginaut"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0), "Couldn't start downloading")?;
            let _ = WinHttpSetTimeouts(session.0, 15_000, 15_000, 30_000, 30_000);
            let conn = open(WinHttpConnect(session.0, &HSTRING::from(host), INTERNET_DEFAULT_HTTPS_PORT, 0), "Couldn't connect")?;
            let req = open(
                WinHttpOpenRequest(conn.0, w!("GET"), &HSTRING::from(path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE),
                "Couldn't connect",
            )?;
            WinHttpSendRequest(req.0, None, None, 0, 0, 0).map_err(offline)?;
            WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(offline)?;
            let mut status = 0u32;
            let mut len = std::mem::size_of::<u32>() as u32;
            WinHttpQueryHeaders(
                req.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some((&mut status as *mut u32).cast()),
                &mut len,
                std::ptr::null_mut(),
            )
            .map_err(offline)?;
            if status != 200 {
                return Err(format!("The download server answered {status}."));
            }
            let mut file = std::fs::File::create(dest).map_err(|e| format!("Couldn't write {}: {e}", dest.display()))?;
            let mut buf = vec![0u8; 256 << 10];
            let mut total = 0u64;
            loop {
                if cancel.load(Ordering::Acquire) {
                    return Err("stopped".into());
                }
                let mut read = 0u32;
                WinHttpReadData(req.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut read).map_err(offline)?;
                if read == 0 {
                    break;
                }
                file.write_all(&buf[..read as usize]).map_err(|e| e.to_string())?;
                total += read as u64;
                if total > expect + (1 << 20) {
                    return Err("The download was larger than expected.".into());
                }
                progress(total, expect);
            }
            file.flush().map_err(|e| e.to_string())?;
            if total != expect {
                return Err("The download was cut short; try again.".into());
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod net {
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    pub fn get(_: &str, _: &str, _: &Path, _: u64, _: &AtomicBool, _: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
        Err("Downloads aren't supported on this system.".into())
    }
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
