//! Your saved signatures and initials. Kept in the app's data folder, encrypted
//! with Windows' own per-user encryption (DPAPI) or, on Linux, a key in the desktop's
//! keyring: the file is useless if copied to another account or PC.
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
    #[cfg(not(windows))]
    dpapi::set_dir(dir);
    let path = store_path(dir);
    let Ok(bytes) = std::fs::read(&path) else { return Ok(Vec::new()) };
    let plain = dpapi::unprotect(&bytes)?;
    serde_json::from_slice(&plain).map_err(|e| e.to_string())
}

pub fn save(dir: &PathBuf, list: &[SigEntry]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    dpapi::set_dir(dir);
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

/// Linux: sealed with ChaCha20-Poly1305 under a random key kept in the desktop's keyring (GNOME Keyring,
/// KWallet: the Secret Service), so the file alone is useless. Without a keyring (a bare window manager),
/// the key is kept in a file only this account can read, next to the signatures.
#[cfg(not(windows))]
mod dpapi {
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
    use std::sync::OnceLock;

    const MAGIC: &[u8; 4] = b"PGS1";
    static DIR: OnceLock<std::path::PathBuf> = OnceLock::new();

    /// Where the key file goes when there's no keyring (the signatures' folder).
    pub fn set_dir(dir: &std::path::Path) {
        let _ = DIR.set(dir.to_path_buf());
    }

    fn key() -> Result<[u8; 32], String> {
        let fresh = || {
            let mut k = [0u8; 32];
            getrandom::fill(&mut k).map_err(|e| e.to_string())?;
            Ok::<_, String>(k)
        };
        if let Ok(entry) = keyring::Entry::new("Paginaut", "saved signatures") {
            match entry.get_secret() {
                Ok(k) if k.len() == 32 => return Ok(k.try_into().unwrap_or([0; 32])),
                Err(keyring::Error::NoEntry) => {
                    let k = fresh()?;
                    if entry.set_secret(&k).is_ok() {
                        return Ok(k);
                    }
                }
                _ => {}
            }
        }
        let file = DIR.get().ok_or("no folder for the key")?.join("signatures.key");
        if let Ok(k) = std::fs::read(&file) {
            if k.len() == 32 {
                return Ok(k.try_into().unwrap_or([0; 32]));
            }
        }
        let k = fresh()?;
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&file).map_err(|e| e.to_string())?;
            f.write_all(&k).map_err(|e| e.to_string())?;
        }
        Ok(k)
    }

    pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key()?));
        let mut nonce = [0u8; 12];
        getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
        let sealed = cipher.encrypt(Nonce::from_slice(&nonce), data).map_err(|_| "couldn't encrypt the signatures".to_string())?;
        Ok([&MAGIC[..], &nonce, &sealed].concat())
    }

    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
        let Some(rest) = data.strip_prefix(&MAGIC[..]) else { return Ok(data.to_vec()) };
        if rest.len() < 12 {
            return Err("The saved signatures are damaged.".into());
        }
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key()?));
        cipher.decrypt(Nonce::from_slice(&rest[..12]), &rest[12..]).map_err(|_| "The saved signatures can't be read with this account's key.".into())
    }
}
