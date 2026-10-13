//! Files for the isolated engine (Android, engine_link.rs): it can't open or make any file, so the app passes it
//! handles. A path "fd:N/name" names such a handle (N) and the file's name (its kind, for MuPDF). Temporary files
//! (books drawn again, covers, unpacked comics) come from the app through `Scratch`. Elsewhere (the desktop, and
//! anything not given a handle) paths are ordinary paths and these helpers do what std::fs would.

#[cfg(unix)]
use parking_lot::Mutex;
#[cfg(unix)]
use std::collections::HashMap;
#[cfg(unix)]
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd, RawFd};
#[cfg(unix)]
use std::sync::OnceLock;

/// Temporary files from the app, for an engine that can't make its own.
#[cfg(unix)]
pub trait Scratch: Send + Sync {
    /// A nameless read-write file (gone once closed).
    fn anon(&self) -> Result<OwnedFd, String>;
    /// A file kept by name (`commit`), if there is one.
    fn open(&self, name: &str) -> Option<OwnedFd>;
    /// A new read-write file, to be kept as `name` once written (`commit`).
    fn create(&self, name: &str) -> Result<OwnedFd, String>;
    fn commit(&self, name: &str) -> Result<(), String>;
}

#[cfg(unix)]
static SCRATCH: OnceLock<Box<dyn Scratch>> = OnceLock::new();

/// Handles behind "fd:" paths made here (books drawn again, previews), open while they're used.
#[cfg(unix)]
static HELD: Mutex<Option<HashMap<String, OwnedFd>>> = Mutex::new(None);

/// This process is the isolated engine: temporary files come from `s`.
#[cfg(unix)]
pub fn set_scratch(s: Box<dyn Scratch>) {
    if SCRATCH.set(s).is_ok() {
        unsafe { super::ffi::po_set_scratch(Some(c_scratch)) };
    }
}

#[cfg(unix)]
extern "C" fn c_scratch() -> std::os::raw::c_int {
    use std::os::fd::IntoRawFd;
    SCRATCH.get().and_then(|s| s.anon().ok()).map_or(-1, |fd| fd.into_raw_fd())
}

#[cfg(unix)]
pub fn scratch() -> Option<&'static dyn Scratch> {
    SCRATCH.get().map(|b| b.as_ref())
}

/// The path naming handle `fd` (a file called `name`).
#[cfg(unix)]
pub fn fd_path(fd: RawFd, name: &str) -> String {
    format!("fd:{fd}/{name}")
}

/// Keeps `fd` open until `release`, and names it.
#[cfg(unix)]
pub fn hold(fd: OwnedFd, name: &str) -> String {
    let path = fd_path(fd.as_raw_fd(), name);
    HELD.lock().get_or_insert_with(HashMap::new).insert(path.clone(), fd);
    path
}

/// Lets go of a temporary file: a handle held here, or a file on disk.
pub fn release(path: &str) {
    #[cfg(unix)]
    if handle(path).is_some() {
        HELD.lock().get_or_insert_with(HashMap::new).remove(path);
        return;
    }
    let _ = std::fs::remove_file(path);
}

/// The handle an "fd:" path names.
#[cfg(unix)]
pub fn handle(path: &str) -> Option<RawFd> {
    let rest = path.strip_prefix("fd:")?;
    rest.split('/').next()?.parse().ok()
}

#[cfg(not(unix))]
pub fn handle(_path: &str) -> Option<i32> {
    None
}

/// The file's name: after the handle for "fd:" paths, else the path itself.
pub fn file_name(path: &str) -> &str {
    if handle(path).is_some() {
        path.split_once('/').map_or(path, |(_, n)| n)
    } else {
        path
    }
}

/// A file of its own reading the same file as `path`'s handle.
#[cfg(unix)]
fn reopen(fd: RawFd) -> std::io::Result<std::fs::File> {
    // SAFETY: the handle is held open by whoever made the path for as long as the path is used.
    let owned = unsafe { BorrowedFd::borrow_raw(fd) }.try_clone_to_owned()?;
    Ok(std::fs::File::from(owned))
}

/// Size and time of last change (seconds since 1970).
pub fn stat(path: &str) -> Option<(u64, u64)> {
    let meta = {
        #[cfg(unix)]
        match handle(path) {
            Some(fd) => reopen(fd).ok()?.metadata().ok()?,
            None => std::fs::metadata(path).ok()?,
        }
        #[cfg(not(unix))]
        std::fs::metadata(path).ok()?
    };
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    Some((meta.len(), mtime))
}

/// Writes `bytes` as the file `target`: through its handle, or beside it first and then in its place
/// (saving over the file being read is allowed).
pub fn write_target(target: &str, bytes: &[u8]) -> Result<(), String> {
    #[cfg(unix)]
    if let Some(fd) = handle(target) {
        use std::os::unix::fs::FileExt;
        let f = reopen(fd).map_err(|e| e.to_string())?;
        f.set_len(0).map_err(|e| e.to_string())?;
        return f.write_all_at(bytes, 0).map_err(|e| e.to_string());
    }
    if let Some(dir) = std::path::Path::new(target).parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let part = format!("{target}.part");
    std::fs::write(&part, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&part, target).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        e.to_string()
    })
}

/// Copies the file `from` to `target` (either may be a handle).
pub fn copy_file(from: &str, target: &str) -> Result<(), String> {
    #[cfg(unix)]
    if handle(from).is_some() || handle(target).is_some() {
        use std::os::unix::fs::FileExt;
        let src = match handle(from) {
            Some(fd) => reopen(fd),
            None => std::fs::File::open(from),
        }
        .map_err(|e| e.to_string())?;
        let dst = match handle(target) {
            Some(fd) => reopen(fd),
            None => std::fs::File::create(target),
        }
        .map_err(|e| e.to_string())?;
        dst.set_len(0).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 1 << 16];
        let mut at = 0u64;
        loop {
            let n = src.read_at(&mut buf, at).map_err(|e| e.to_string())?;
            if n == 0 {
                return Ok(());
            }
            dst.write_all_at(&buf[..n], at).map_err(|e| e.to_string())?;
            at += n as u64;
        }
    }
    if let Some(dir) = std::path::Path::new(target).parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let part = format!("{target}.part");
    std::fs::copy(from, &part).map_err(|e| e.to_string())?;
    std::fs::rename(&part, target).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        e.to_string()
    })
}
