//! The Windows shelf: every e-book and comic in the folders chosen for it. Each book is known by a
//! fingerprint of its contents, the same as Android's (SHA-1 of its size, its first and its last
//! 64 KB), so its cover stays with it when it's moved or renamed. Fingerprints are kept in
//! `shelf-index.json` and only taken again for files whose size or date changed.

use crate::app::{blocking, Res};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::Path;
use std::time::UNIX_EPOCH;
use tauri::{AppHandle, Manager};

const EBOOKS: [&str; 4] = ["epub", "mobi", "fb2", "fbz"];
const COMICS: [&str; 3] = ["cbz", "cbr", "cbt"];
const FP_PART: u64 = 64 * 1024;
/// Folders deeper than this aren't looked in.
const MAX_DEPTH: usize = 12;

fn kind(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if EBOOKS.contains(&ext.as_str()) {
        Some("ebook")
    } else if COMICS.contains(&ext.as_str()) {
        Some("comic")
    } else {
        None
    }
}

/// Folders that never hold a library: hidden ones, the recycle bin and system folders, app data,
/// and package caches.
fn skipped(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('$') || ["AppData", "node_modules", "System Volume Information", "Windows"].contains(&name)
}

fn fingerprint(path: &Path, size: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let take = || -> Result<String, String> {
        let mut f = File::open(path).map_err(|e| e.to_string())?;
        let mut h = crate::net::Hasher::new(crate::net::Alg::Sha1)?;
        h.update(size.to_string().as_bytes())?;
        let mut buf = vec![0u8; FP_PART as usize];
        let mut part = |f: &mut File, at: u64| -> Result<(), String> {
            f.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
            let mut got = 0;
            while got < buf.len() {
                let n = f.read(&mut buf[got..]).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                got += n;
            }
            h.update(&buf[..got])
        };
        part(&mut f, 0)?;
        if size > FP_PART {
            part(&mut f, FP_PART.max(size - FP_PART))?;
        }
        Ok(h.finish()?[..10].iter().map(|b| format!("{b:02x}")).collect())
    };
    take().unwrap_or_default()
}

fn walk(dir: &Path, depth: usize, found: &mut Vec<(String, u64, u64)>, seen: &mut HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let Ok(t) = e.file_type() else { continue };
        let path = e.path();
        // Links and junctions are left alone: they loop, or lead to folders looked in anyway.
        if t.is_symlink() {
            continue;
        }
        if t.is_dir() {
            if depth < MAX_DEPTH && !skipped(&e.file_name().to_string_lossy()) {
                walk(&path, depth + 1, found, seen);
            }
        } else if kind(&path).is_some() {
            let Ok(meta) = e.metadata() else { continue };
            let key = path.to_string_lossy().to_lowercase();
            // Folders inside one another: each file once.
            if !seen.insert(key) {
                continue;
            }
            let mtime = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
            found.push((path.to_string_lossy().into_owned(), meta.len(), mtime));
        }
    }
}

/// The e-books and comics in `folders`: `{ items: [{ path, name, size, mtime, fp, kind }] }`, one per
/// book (copies of the same file are shown once).
#[tauri::command]
pub async fn scan_books(app: AppHandle, folders: Vec<String>) -> Res<Value> {
    let index_file = app.path().app_data_dir().map_err(|e| e.to_string())?.join("shelf-index.json");
    blocking(move || {
        let old: HashMap<String, (u64, u64, String)> = std::fs::read_to_string(&index_file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        let mut found = Vec::new();
        let mut seen = HashSet::new();
        for f in &folders {
            walk(Path::new(f), 0, &mut found, &mut seen);
        }
        let mut index = HashMap::new();
        let mut fps = HashSet::new();
        let mut items = Vec::new();
        for (path, size, mtime) in found {
            let fp = match old.get(&path) {
                Some((s, m, fp)) if *s == size && *m == mtime && !fp.is_empty() => fp.clone(),
                _ => fingerprint(Path::new(&path), size),
            };
            index.insert(path.clone(), (size, mtime, fp.clone()));
            if !fp.is_empty() && !fps.insert(fp.clone()) {
                continue;
            }
            let p = Path::new(&path);
            items.push(json!({
                "path": path,
                "name": p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                "size": size,
                "mtime": mtime,
                "fp": fp,
                "kind": kind(p),
            }));
        }
        if let Some(dir) = index_file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&index_file, serde_json::to_string(&index).unwrap_or_default());
        Ok(json!({ "items": items }))
    })
    .await
}
