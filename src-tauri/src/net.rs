//! HTTPS downloads and file hashes. On Windows through Windows itself: WinHTTP (the system's proxy settings
//! and certificate store apply, and no HTTP library ships with the app) and BCrypt; on Linux through ureq
//! and RustCrypto (see `other`).

use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy)]
pub enum Alg {
    Sha1,
    Sha256,
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Where a download goes, and what it's checked against as it arrives.
pub struct Get<'a> {
    pub host: &'a str,
    pub path: &'a str,
    /// Stop early when set.
    pub cancel: &'a AtomicBool,
    /// Bytes received, bytes expected (the server's length when `expect` is None).
    pub progress: &'a mut dyn FnMut(u64, u64),
    /// The exact size, when known beforehand; otherwise the server's own length is required.
    pub expect: Option<u64>,
    /// Refuse anything larger.
    pub max: u64,
    /// Bytes already downloaded: ask for the rest (a server that can't send part of a file answers "RESTART").
    pub from: u64,
    /// Give up ("SLOW") when fewer bytes than this arrive per second, measured over half a minute.
    pub min_rate: u64,
}

/// Why `fetch` stopped when the caller can do something about it.
const RESTART: &str = "RESTART";
const SLOW: &str = "SLOW";

#[cfg(windows)]
pub use win::{fetch, Hasher};

#[cfg(windows)]
mod win {
    use super::{Alg, Get, RESTART, SLOW};
    use std::sync::atomic::Ordering;
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Networking::WinHttp::*;
    use windows::Win32::Security::Cryptography::{
        BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash, BCryptHashData, BCRYPT_HASH_HANDLE, BCRYPT_SHA1_ALG_HANDLE, BCRYPT_SHA256_ALG_HANDLE,
    };

    pub struct Hasher(BCRYPT_HASH_HANDLE, usize);

    impl Hasher {
        pub fn new(alg: Alg) -> Result<Self, String> {
            let (handle, len) = match alg {
                Alg::Sha1 => (BCRYPT_SHA1_ALG_HANDLE, 20),
                Alg::Sha256 => (BCRYPT_SHA256_ALG_HANDLE, 32),
            };
            let mut h = BCRYPT_HASH_HANDLE::default();
            unsafe { BCryptCreateHash(handle, &mut h, None, None, 0).ok().map_err(|e| e.to_string())? };
            Ok(Hasher(h, len))
        }
        pub fn update(&mut self, data: &[u8]) -> Result<(), String> {
            unsafe { BCryptHashData(self.0, data, 0).ok().map_err(|e| e.to_string()) }
        }
        pub fn finish(self) -> Result<Vec<u8>, String> {
            let mut out = vec![0u8; self.1];
            unsafe { BCryptFinishHash(self.0, &mut out, 0).ok().map_err(|e| e.to_string())? };
            Ok(out)
        }
    }

    impl Drop for Hasher {
        fn drop(&mut self) {
            unsafe {
                let _ = BCryptDestroyHash(self.0);
            }
        }
    }

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

    unsafe fn header(req: &Handle, which: u32) -> Option<String> {
        let mut buf = [0u16; 64];
        let mut len = (buf.len() * 2) as u32;
        WinHttpQueryHeaders(req.0, which, PCWSTR::null(), Some(buf.as_mut_ptr().cast()), &mut len, std::ptr::null_mut()).ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize / 2]))
    }

    /// Downloads over HTTPS (following redirects to other HTTPS servers), handing each piece to `sink`.
    /// Returns the size.
    pub fn fetch(g: Get, sink: &mut dyn FnMut(&[u8]) -> Result<(), String>) -> Result<u64, String> {
        let offline = |e: windows::core::Error| format!("Couldn't download (are you online?): {}", e.message());
        unsafe {
            let session = open(WinHttpOpen(w!("Paginaut"), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0), "Couldn't start downloading")?;
            let _ = WinHttpSetTimeouts(session.0, 15_000, 15_000, 30_000, 60_000);
            let conn = open(WinHttpConnect(session.0, &HSTRING::from(g.host), INTERNET_DEFAULT_HTTPS_PORT, 0), "Couldn't connect")?;
            let req = open(
                WinHttpOpenRequest(conn.0, w!("GET"), &HSTRING::from(g.path), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE),
                "Couldn't connect",
            )?;
            let range: Vec<u16> = format!("Range: bytes={}-", g.from).encode_utf16().collect();
            WinHttpSendRequest(req.0, (g.from > 0).then_some(&range[..]), None, 0, 0, 0).map_err(offline)?;
            WinHttpReceiveResponse(req.0, std::ptr::null_mut()).map_err(offline)?;
            let status = header(&req, WINHTTP_QUERY_STATUS_CODE).and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
            match status {
                200 if g.from > 0 => return Err(RESTART.into()),
                200 | 206 => {}
                _ => return Err(format!("The download server answered {status}.")),
            }
            // A partial answer's length is what's left.
            let length = header(&req, WINHTTP_QUERY_CONTENT_LENGTH).and_then(|s| s.trim().parse::<u64>().ok()).map(|l| l + g.from);
            let expect = match (g.expect, length) {
                (Some(e), Some(l)) if e != l => return Err("The download server offered a different file than expected.".into()),
                (Some(e), _) => e,
                (None, Some(l)) => l,
                (None, None) => g.max,
            };
            if expect > g.max {
                return Err("The download was larger than expected.".into());
            }
            let mut buf = vec![0u8; 256 << 10];
            let mut total = g.from;
            let mut window = (std::time::Instant::now(), total);
            loop {
                if g.cancel.load(Ordering::Acquire) {
                    return Err("stopped".into());
                }
                let mut read = 0u32;
                WinHttpReadData(req.0, buf.as_mut_ptr().cast(), buf.len() as u32, &mut read).map_err(offline)?;
                if read == 0 {
                    break;
                }
                total += read as u64;
                let secs = window.0.elapsed().as_secs();
                if secs >= 30 {
                    if g.min_rate > 0 && (total - window.1) / secs < g.min_rate {
                        return Err(SLOW.into());
                    }
                    window = (std::time::Instant::now(), total);
                }
                if total > expect {
                    return Err("The download was larger than expected.".into());
                }
                sink(&buf[..read as usize])?;
                (g.progress)(total, expect);
            }
            if (g.expect.is_some() || length.is_some()) && total != expect {
                return Err("The download was cut short; try again.".into());
            }
            Ok(total)
        }
    }
}

#[cfg(not(windows))]
pub use other::{fetch, Hasher};

/// Linux: ureq over rustls, trusting the system's certificates (as WinHTTP does on Windows) and its
/// proxy settings (https_proxy, all_proxy); hashes with RustCrypto's SHA-1 and SHA-256.
#[cfg(not(windows))]
mod other {
    use super::{Alg, Get, RESTART, SLOW};
    use sha1::Digest;
    use std::io::Read;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    pub enum Hasher {
        Sha1(sha1::Sha1),
        Sha256(sha2::Sha256),
    }

    impl Hasher {
        pub fn new(alg: Alg) -> Result<Self, String> {
            Ok(match alg {
                Alg::Sha1 => Hasher::Sha1(sha1::Sha1::new()),
                Alg::Sha256 => Hasher::Sha256(sha2::Sha256::new()),
            })
        }
        pub fn update(&mut self, data: &[u8]) -> Result<(), String> {
            match self {
                Hasher::Sha1(h) => h.update(data),
                Hasher::Sha256(h) => h.update(data),
            }
            Ok(())
        }
        pub fn finish(self) -> Result<Vec<u8>, String> {
            Ok(match self {
                Hasher::Sha1(h) => h.finalize().to_vec(),
                Hasher::Sha256(h) => h.finalize().to_vec(),
            })
        }
    }

    /// Downloads over HTTPS (following redirects to other HTTPS servers), handing each piece to `sink`.
    /// Returns the size.
    pub fn fetch(g: Get, sink: &mut dyn FnMut(&[u8]) -> Result<(), String>) -> Result<u64, String> {
        let offline = |e: ureq::Error| format!("Couldn't download (are you online?): {e}");
        let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .proxy(ureq::Proxy::try_from_env())
            .https_only(true)
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_recv_body(None)
            .user_agent("Paginaut")
            .build()
            .into();
        let mut req = agent.get(format!("https://{}{}", g.host, g.path));
        if g.from > 0 {
            req = req.header("Range", format!("bytes={}-", g.from));
        }
        let mut resp = req.call().map_err(offline)?;
        let status = resp.status().as_u16();
        match status {
            200 if g.from > 0 => return Err(RESTART.into()),
            200 | 206 => {}
            _ => return Err(format!("The download server answered {status}.")),
        }
        // A partial answer's length is what's left.
        let length = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|s| s.trim().parse::<u64>().ok()).map(|l| l + g.from);
        let expect = match (g.expect, length) {
            (Some(e), Some(l)) if e != l => return Err("The download server offered a different file than expected.".into()),
            (Some(e), _) => e,
            (None, Some(l)) => l,
            (None, None) => g.max,
        };
        if expect > g.max {
            return Err("The download was larger than expected.".into());
        }
        let mut reader = resp.body_mut().with_config().limit(g.max.saturating_add(1)).reader();
        let mut buf = vec![0u8; 256 << 10];
        let mut total = g.from;
        let mut window = (std::time::Instant::now(), total);
        loop {
            if g.cancel.load(Ordering::Acquire) {
                return Err("stopped".into());
            }
            let read = reader.read(&mut buf).map_err(|e| format!("Couldn't download (are you online?): {e}"))?;
            if read == 0 {
                break;
            }
            total += read as u64;
            let secs = window.0.elapsed().as_secs();
            if secs >= 30 {
                if g.min_rate > 0 && (total - window.1) / secs < g.min_rate {
                    return Err(SLOW.into());
                }
                window = (std::time::Instant::now(), total);
            }
            if total > expect {
                return Err("The download was larger than expected.".into());
            }
            sink(&buf[..read])?;
            (g.progress)(total, expect);
        }
        if (g.expect.is_some() || length.is_some()) && total != expect {
            return Err("The download was cut short; try again.".into());
        }
        Ok(total)
    }
}

/// Downloads into `dest` (hashing as it goes when `hash` is set). Returns the hash.
pub fn to_file(g: Get, dest: &std::path::Path, hash: Option<Alg>) -> Result<Option<Vec<u8>>, String> {
    use std::io::Write;
    let file = std::fs::File::create(dest).map_err(|e| format!("Couldn't write {}: {e}", dest.display()))?;
    let mut out = std::io::BufWriter::with_capacity(1 << 20, file);
    let mut hasher = hash.map(Hasher::new).transpose()?;
    fetch(g, &mut |chunk| {
        if let Some(h) = hasher.as_mut() {
            h.update(chunk)?;
        }
        out.write_all(chunk).map_err(|e| e.to_string())
    })?;
    out.flush().map_err(|e| e.to_string())?;
    hasher.map(|h| h.finish()).transpose()
}

/// A large download into `dest` that survives interruptions: after a dropped connection, or a server sending
/// too slowly (a download site hands out a mirror per request, and some are slow), it asks again for the rest.
/// Returns the file's hash.
pub fn to_file_resuming(host: &str, path: &str, dest: &std::path::Path, cancel: &AtomicBool, progress: &mut dyn FnMut(u64, u64), max: u64, alg: Alg) -> Result<Vec<u8>, String> {
    use std::io::{Read, Write};
    let mut file = std::fs::File::create(dest).map_err(|e| format!("Couldn't write {}: {e}", dest.display()))?;
    let (mut have, mut failures, mut slow_switches) = (0u64, 0, 0);
    loop {
        let min_rate = if slow_switches < 4 { 150 << 10 } else { 0 };
        let r = fetch(Get { host, path, cancel, progress: &mut *progress, expect: None, max, from: have, min_rate }, &mut |chunk| {
            file.write_all(chunk).map_err(|e| e.to_string())?;
            have += chunk.len() as u64;
            Ok(())
        });
        match r {
            Ok(_) => break,
            Err(e) if e == RESTART => {
                // The server can't send part of the file: start over.
                file.set_len(0).and_then(|_| std::io::Seek::rewind(&mut file)).map_err(|e| e.to_string())?;
                have = 0;
                failures += 1;
            }
            Err(e) if e == SLOW => slow_switches += 1,
            Err(e) if e == "stopped" || e.contains("larger") || e.contains("different file") => return Err(e),
            Err(e) => {
                failures += 1;
                if failures > 6 {
                    return Err(e);
                }
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
        }
        if failures > 6 {
            return Err("The download kept failing; try again later.".into());
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    let mut h = Hasher::new(alg)?;
    let mut f = std::fs::File::open(dest).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n])?;
    }
    h.finish()
}

/// A small text download (a listing, a checksum file).
pub fn text(host: &str, path: &str, cancel: &AtomicBool) -> Result<String, String> {
    let mut data = Vec::new();
    fetch(Get { host, path, cancel, progress: &mut |_, _| {}, expect: None, max: 4 << 20, from: 0, min_rate: 0 }, &mut |chunk| {
        data.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(String::from_utf8_lossy(&data).into_owned())
}
