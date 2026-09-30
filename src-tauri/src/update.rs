//! Updates, from Paginaut's releases on GitHub, without a second HTTP stack: WinHTTP (net.rs) downloads, and
//! everything is checked against the minisign key built in here before it's used. `latest.json` has the format
//! Tauri's own updater reads (version, notes, date, and per platform the installer's URL and signature), and the
//! installer runs as Tauri's updater would run it: passive, `/UPDATE`, restarting Paginaut afterwards.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// The public half of the key releases are signed with (minisign, as `tauri signer generate` makes it: base64 of
/// the .pub file). The private half never enters the repository.
const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDM5OTk3ODdGRTQ1N0YwRTMKUldUajhGZmtmM2laT1FjT2xnUG1hYmV2NWlMQ0xYRExRc3hab3hqUkJHY1NIM2Q1T2l4YzliSU4K";

/// Where releases are published: a public repository (the app can't read a private one's releases).
pub const RELEASES: &str = "adityapahurkar9/paginaut-releases";
pub const HOST: &str = "github.com";

/// A file of the newest release.
pub fn latest_path(file: &str) -> String {
    format!("/{RELEASES}/releases/latest/download/{file}")
}

/// Checks `data` against a signature made with the release key (`sig`: the base64 .sig file `tauri signer` writes).
pub fn verify(data: &[u8], sig: &str) -> Result<(), String> {
    let bad = || "The download's signature didn't match Paginaut's, so it wasn't used.".to_string();
    let text = |b64: &str| crate::base64::decode(b64.trim()).and_then(|b| String::from_utf8(b).ok());
    let pk = minisign_verify::PublicKey::decode(&text(PUBKEY).ok_or_else(bad)?).map_err(|_| bad())?;
    let sig = minisign_verify::Signature::decode(&text(sig).ok_or_else(bad)?).map_err(|_| bad())?;
    pk.verify(data, &sig, true).map_err(|_| bad())
}

/// A small file of the newest release (None when nothing has been published yet).
pub fn latest_text(file: &str, cancel: &AtomicBool) -> Result<Option<String>, String> {
    match crate::net::text(HOST, &latest_path(file), cancel) {
        Ok(t) => Ok(Some(t)),
        Err(e) if e.contains("answered 404") => Ok(None),
        Err(e) => Err(e),
    }
}

#[derive(Deserialize)]
struct Latest {
    version: String,
    notes: Option<String>,
    pub_date: Option<String>,
    platforms: HashMap<String, Platform>,
}

#[derive(Deserialize, Clone)]
struct Platform {
    signature: String,
    url: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Available {
    pub version: String,
    pub notes: String,
    pub date: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Checked {
    pub current: &'static str,
    /// A newer version, if there is one.
    pub available: Option<Available>,
    /// No release has been published yet.
    pub none_published: bool,
}

/// The installer of the newer version found by the last check.
static FOUND: Mutex<Option<(String, Platform)>> = Mutex::new(None);

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// `a` is a later version than `b` (numbers compared part by part; anything after a "-" is ignored).
pub fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| v.trim().trim_start_matches('v').split('-').next().unwrap_or("").split('.').map(|p| p.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    let (x, y) = (parts(a), parts(b));
    let n = x.len().max(y.len());
    let at = |v: &Vec<u64>, i: usize| v.get(i).copied().unwrap_or(0);
    (0..n).map(|i| at(&x, i).cmp(&at(&y, i))).find(|o| o.is_ne()) == Some(std::cmp::Ordering::Greater)
}

pub fn check(cancel: &AtomicBool) -> Result<Checked, String> {
    let Some(text) = latest_text("latest.json", cancel)? else {
        return Ok(Checked { current: CURRENT, available: None, none_published: true });
    };
    let latest: Latest = serde_json::from_str(&text).map_err(|_| "The update information couldn't be read.".to_string())?;
    let platform = ["windows-x86_64-nsis", "windows-x86_64"].iter().find_map(|k| latest.platforms.get(*k)).cloned();
    let available = match platform {
        Some(p) if newer(&latest.version, CURRENT) => {
            *FOUND.lock() = Some((latest.version.clone(), p));
            Some(Available {
                version: latest.version.trim_start_matches('v').to_string(),
                notes: latest.notes.unwrap_or_default(),
                date: latest.pub_date.unwrap_or_default(),
            })
        }
        _ => None,
    };
    Ok(Checked { current: CURRENT, available, none_published: false })
}

/// The installer's address, if it is one of Paginaut's releases: host and path.
fn release_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_at(rest.find('/')?);
    let file = path.rsplit('/').next()?;
    let ok = host == HOST
        && path.starts_with(&format!("/{RELEASES}/releases/download/"))
        && file.ends_with(".exe")
        && file.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c));
    ok.then_some((host, path))
}

/// Downloads the installer found by the last check into `dir` and checks its signature. Returns its path.
pub fn download(dir: &Path, cancel: &AtomicBool, progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf, String> {
    let (version, p) = FOUND.lock().clone().ok_or("Check for updates first.")?;
    let (host, path) = release_url(&p.url).ok_or("The update's address isn't one of Paginaut's releases, so it wasn't downloaded.")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("Paginaut_{}_setup.exe", version.trim_start_matches('v').replace(|c: char| !c.is_ascii_alphanumeric() && c != '.', "")));
    let part = dest.with_extension("part");
    let get = crate::net::Get { host, path, cancel, progress, expect: None, max: 400 << 20, from: 0, min_rate: 0 };
    let res = crate::net::to_file(get, &part, None).and_then(|_| {
        let data = std::fs::read(&part).map_err(|e| e.to_string())?;
        verify(&data, &p.signature)?;
        let _ = std::fs::remove_file(&dest);
        std::fs::rename(&part, &dest).map_err(|e| e.to_string())
    });
    if res.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    res.map(|_| dest)
}

/// Starts the installer (passive, as an update, restarting Paginaut when done). The caller then quits.
pub fn run_installer(installer: &Path) -> Result<(), String> {
    let mut cmd = std::process::Command::new(installer);
    cmd.args(["/P", "/UPDATE", "/R"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Its own process group, not attached to this one: it outlives Paginaut quitting.
        const DETACHED_PROCESS: u32 = 0x8;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn().map(|_| ()).map_err(|e| format!("Couldn't start the installer: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(newer("0.8.1", "0.8.0"));
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(newer("1.0", "0.99.99"));
        assert!(!newer("0.8.0", "0.8.0"));
        assert!(!newer("0.8.0-beta", "0.8.0"));
        assert!(!newer("0.7.99", "0.8"));
    }

    #[test]
    fn only_our_releases() {
        assert!(release_url("https://github.com/adityapahurkar9/paginaut-releases/releases/download/v0.8.0/Paginaut_0.8.0_x64-setup.exe").is_some());
        assert!(release_url("https://github.com/someone/else/releases/download/v1/x.exe").is_none());
        assert!(release_url("http://github.com/adityapahurkar9/paginaut-releases/releases/download/v0.8.0/a.exe").is_none());
        assert!(release_url("https://evil.example/adityapahurkar9/paginaut-releases/releases/download/v0.8.0/a.exe").is_none());
        assert!(release_url("https://github.com/adityapahurkar9/paginaut-releases/releases/download/v0.8.0/a.msi").is_none());
    }

    #[test]
    fn rejects_bad_signatures() {
        assert!(verify(b"data", "bm90IGEgc2lnbmF0dXJl").is_err());
    }

    /// A release tools/release.ps1 staged: its installer checks out against latest.json's signature, and its add-on
    /// list against its own. `PAGINAUT_RELEASE_DIR=release/v0.8.0 cargo test --lib staged_release -- --ignored`
    #[test]
    #[ignore]
    fn staged_release() {
        let dir = std::path::PathBuf::from(std::env::var("PAGINAUT_RELEASE_DIR").expect("PAGINAUT_RELEASE_DIR"));
        let latest: Latest = serde_json::from_str(&std::fs::read_to_string(dir.join("latest.json")).unwrap()).unwrap();
        let p = &latest.platforms["windows-x86_64"];
        assert!(release_url(&p.url).is_some(), "{}", p.url);
        let file = p.url.rsplit('/').next().unwrap();
        verify(&std::fs::read(dir.join(file)).unwrap(), &p.signature).unwrap();
        let json = std::fs::read(dir.join("addons.json")).unwrap();
        verify(&json, &std::fs::read_to_string(dir.join("addons.json.sig")).unwrap()).unwrap();
    }

    /// A file signed with the release key (`tauri signer sign`) checks out; the same signature on changed data doesn't.
    #[test]
    fn release_key_signatures() {
        let data = include_bytes!("../tests/fixtures/signed.txt");
        let sig = include_str!("../tests/fixtures/signed.txt.sig");
        verify(data, sig).unwrap();
        let mut changed = data.to_vec();
        changed[0] ^= 1;
        assert!(verify(&changed, sig).is_err());
    }
}
