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

    /// The file's first bytes say where its key is: the keyring, or the key file. Only `protect` makes a key:
    /// reading never does, so a keyring that's locked or not started yet can't lead to a second key.
    const IN_KEYRING: &[u8; 4] = b"PGS1";
    const IN_FILE: &[u8; 4] = b"PGS2";
    static DIR: OnceLock<std::path::PathBuf> = OnceLock::new();

    /// Where the key file goes when there's no keyring (the signatures' folder).
    pub fn set_dir(dir: &std::path::Path) {
        let _ = DIR.set(dir.to_path_buf());
    }

    fn fresh() -> Result<[u8; 32], String> {
        let mut k = [0u8; 32];
        getrandom::fill(&mut k).map_err(|e| e.to_string())?;
        Ok(k)
    }

    /// The key in the desktop's keyring (made when `make` and there is none).
    fn keyring_key(make: bool) -> Result<[u8; 32], String> {
        // Connected on first use, and again when the connection fails: a keyring that wasn't up yet (just after
        // logging in) or was restarted is found on the next try, not only after Paginaut restarts. A connection
        // kept from before is made again at once.
        let (entry, got) = loop {
            let kept = keyring_core::get_default_store().is_some();
            if !kept {
                let store = dbus_secret_service_keyring_store::Store::new().map_err(|e| format!("The desktop's keyring isn't available ({e})."))?;
                keyring_core::set_default_store(store);
            }
            let entry = keyring_core::Entry::new("Paginaut", "saved signatures").map_err(|e| format!("The desktop's keyring isn't available ({e})."))?;
            let got = entry.get_secret();
            if matches!(got, Err(keyring_core::Error::PlatformFailure(_) | keyring_core::Error::NoStorageAccess(_))) {
                keyring_core::unset_default_store();
                if kept {
                    continue;
                }
            }
            break (entry, got);
        };
        match got {
            Ok(k) => k.try_into().map_err(|_| "The key for your saved signatures in the desktop's keyring is damaged.".to_string()),
            Err(keyring_core::Error::NoEntry) if make => {
                let k = fresh()?;
                entry.set_secret(&k).map_err(|e| format!("The desktop's keyring didn't keep the key ({e})."))?;
                Ok(k)
            }
            Err(keyring_core::Error::NoEntry) => Err("The key for your saved signatures isn't in the desktop's keyring any more.".into()),
            Err(e) => Err(format!("The desktop's keyring couldn't be opened ({e}): unlock it, then try again.")),
        }
    }

    /// The key kept in a file only this account can read (made when `make` and there is none).
    fn file_key(make: bool) -> Result<[u8; 32], String> {
        let file = DIR.get().ok_or("no folder for the key")?.join("signatures.key");
        if let Ok(k) = std::fs::read(&file) {
            return k.try_into().map_err(|_| "The key file for your saved signatures is damaged.".to_string());
        }
        if !make {
            return Err("The key file for your saved signatures is missing.".into());
        }
        let k = fresh()?;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&file).map_err(|e| e.to_string())?;
        f.write_all(&k).map_err(|e| e.to_string())?;
        Ok(k)
    }

    pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
        // The keyring when it answers; otherwise (a bare window manager) the key file.
        let (magic, key) = match keyring_key(true) {
            Ok(k) => (IN_KEYRING, k),
            Err(_) => (IN_FILE, file_key(true)?),
        };
        let cipher = ChaCha20Poly1305::new(&Key::from(key));
        let mut nonce = [0u8; 12];
        getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
        let sealed = cipher.encrypt(&Nonce::from(nonce), data).map_err(|_| "couldn't encrypt the signatures".to_string())?;
        Ok([&magic[..], &nonce, &sealed].concat())
    }

    pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
        let (keys, rest) = if let Some(rest) = data.strip_prefix(&IN_FILE[..]) {
            (vec![file_key(false)], rest)
        } else if let Some(rest) = data.strip_prefix(&IN_KEYRING[..]) {
            // (0.8.11 marked key-file ones PGS1 too: that key is tried next.)
            (vec![keyring_key(false), file_key(false)], rest)
        } else {
            return Ok(data.to_vec());
        };
        let Some((nonce, sealed)) = rest.split_first_chunk::<12>() else {
            return Err("The saved signatures are damaged.".into());
        };
        let mut why = String::new();
        for key in keys {
            match key {
                Ok(k) => {
                    if let Ok(plain) = ChaCha20Poly1305::new(&Key::from(k)).decrypt(&Nonce::from(*nonce), sealed) {
                        return Ok(plain);
                    }
                }
                Err(e) if why.is_empty() => why = e,
                Err(_) => {}
            }
        }
        Err(if why.is_empty() { "The saved signatures can't be read with this account's key.".into() } else { why })
    }
}

/// Linux: sealed and opened again, whichever key is at hand (in WSL, without a keyring, the key file).
#[cfg(all(test, not(windows)))]
mod tests {
    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("paginaut-sigstore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        super::dpapi::set_dir(&dir);
        let sealed = super::dpapi::protect(b"[1,2,3]").unwrap();
        assert!(sealed.starts_with(b"PGS"));
        assert_eq!(super::dpapi::unprotect(&sealed).unwrap(), b"[1,2,3]");
        // Plain JSON (never sealed) reads as it is.
        assert_eq!(super::dpapi::unprotect(b"[]").unwrap(), b"[]");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
