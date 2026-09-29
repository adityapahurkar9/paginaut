//! Your saved signatures and initials. Kept in the app's data folder, encrypted
//! with Windows' own per-user encryption (DPAPI): the file is useless if copied
//! to another account or PC.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SigEntry {
    pub id: String,
    /// "signature" or "initials".
    pub kind: String,
    pub name: String,
    /// Seconds since 1970.
    pub created: u64,
    /// The picture: base64 PNG with a transparent background.
    pub png: String,
}

fn store_path(dir: &PathBuf) -> PathBuf {
    dir.join("signatures.dat")
}

pub fn load(dir: &PathBuf) -> Result<Vec<SigEntry>, String> {
    let path = store_path(dir);
    let Ok(bytes) = std::fs::read(&path) else { return Ok(Vec::new()) };
    let plain = dpapi::unprotect(&bytes)?;
    serde_json::from_slice(&plain).map_err(|e| e.to_string())
}

pub fn save(dir: &PathBuf, list: &[SigEntry]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let plain = serde_json::to_vec(list).map_err(|e| e.to_string())?;
    let sealed = dpapi::protect(&plain)?;
    let tmp = store_path(dir).with_extension("tmp");
    std::fs::write(&tmp, sealed).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, store_path(dir)).map_err(|e| e.to_string())
}

#[cfg(windows)]
mod dpapi {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
    }

    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        unsafe {
            let _ = LocalFree(Some(HLOCAL(out.pbData as _)));
        }
        v
    }

    pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
        let input = blob(data);
        let mut out = CRYPT_INTEGER_BLOB::default();
        unsafe { CryptProtectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out) }.map_err(|e| e.to_string())?;
        Ok(take(out))
    }

    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
        let input = blob(data);
        let mut out = CRYPT_INTEGER_BLOB::default();
        unsafe { CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out) }
            .map_err(|_| "Your saved signatures belong to another Windows account and can't be read here.".to_string())?;
        Ok(take(out))
    }
}

#[cfg(not(windows))]
mod dpapi {
    pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
        Ok(data.to_vec())
    }
    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
        Ok(data.to_vec())
    }
}
