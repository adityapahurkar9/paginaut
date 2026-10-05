//! LibreOffice as a converter: an installed copy, or Paginaut's own pack (the Document Foundation's
//! installer, downloaded on request, checked, and unpacked into the app's folder: no system install, no
//! admin rights). Conversions run `soffice --headless --convert-to pdf` with a profile of our own, one at a
//! time, in a job object so a stuck LibreOffice can be stopped along with everything it started.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// One LibreOffice: where soffice.exe is, and what it is.
#[derive(Clone, Debug)]
pub struct Lo {
    pub soffice: PathBuf,
    pub version: String,
    /// Paginaut's own pack (true), or one installed on the system.
    pub pack: bool,
}

/// What the pack records about itself.
#[derive(Serialize, Deserialize)]
struct PackInfo {
    version: String,
    sha256: String,
}

const PACK_INFO: &str = "paginaut-pack.json";
/// Where the Document Foundation publishes releases.
const HOST: &str = "download.documentfoundation.org";

/// Paginaut's pack in `dir`, if it's complete.
pub fn pack(dir: &Path) -> Option<Lo> {
    let soffice = dir.join("program").join("soffice.exe");
    let info: PackInfo = serde_json::from_slice(&std::fs::read(dir.join(PACK_INFO)).ok()?).ok()?;
    soffice.is_file().then_some(Lo { soffice, version: info.version, pack: true })
}

/// What to do on Linux, where LibreOffice isn't fetched by Paginaut.
#[cfg(not(windows))]
pub const NOT_HERE: &str = "Install LibreOffice from your software centre (or in a terminal: sudo apt install libreoffice), then try again.";

/// Linux: LibreOffice from the distribution (apt), its own .deb packages (/opt), a snap or a flatpak, in that
/// order. The version is left out: finding it means starting LibreOffice.
#[cfg(not(windows))]
pub fn installed() -> Option<Lo> {
    let mut found: Vec<PathBuf> = vec!["/usr/lib/libreoffice/program/soffice".into(), "/usr/bin/soffice".into(), "/usr/bin/libreoffice".into()];
    if let Ok(list) = std::fs::read_dir("/opt") {
        let mut opt: Vec<PathBuf> = list.flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("libreoffice"))).collect();
        opt.sort();
        found.extend(opt.into_iter().rev().map(|p| p.join("program").join("soffice")));
    }
    found.push("/snap/bin/libreoffice".into());
    found.push("/var/lib/flatpak/exports/bin/org.libreoffice.LibreOffice".into());
    if let Some(home) = std::env::var_os("HOME") {
        found.push(PathBuf::from(home).join(".local/share/flatpak/exports/bin/org.libreoffice.LibreOffice"));
    }
    let soffice = found.into_iter().find(|p| p.is_file())?;
    Some(Lo { soffice, version: String::new(), pack: false })
}

/// A LibreOffice installed on the system (its registry entry, or the usual folder).
#[cfg(windows)]
pub fn installed() -> Option<Lo> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    #[cfg(windows)]
    for root in [reg::HKLM, reg::HKCU] {
        if let Some(p) = reg::string(root, r"SOFTWARE\LibreOffice\UNO\InstallPath", "") {
            dirs.push(PathBuf::from(p));
        }
    }
    for base in ["ProgramFiles", "ProgramW6432"] {
        if let Some(pf) = std::env::var_os(base) {
            dirs.push(PathBuf::from(pf).join("LibreOffice").join("program"));
        }
    }
    let program = dirs.into_iter().find(|d| d.join("soffice.exe").is_file())?;
    // The version: from the registry's product key when there is one (LibreOffice\LibreOffice\<version>).
    #[cfg(windows)]
    let version = [reg::HKLM, reg::HKCU].iter().find_map(|&r| reg::subkeys(r, r"SOFTWARE\LibreOffice\LibreOffice").into_iter().max()).unwrap_or_default();
    #[cfg(not(windows))]
    let version = String::new();
    Some(Lo { soffice: program.join("soffice.exe"), version, pack: false })
}

/// `file:///C:/...` for LibreOffice's -env: settings.
/// LibreOffice can't read a relative one: it stops with a "bootstrap.ini is corrupt" message box.
fn file_url(p: &Path) -> String {
    let p = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let s = p.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file:///");
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/:-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// LibreOffice converts one file at a time (a second instance with the same profile would hand its work to
/// the first and return early).
pub static BUSY: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
static RUN: AtomicU64 = AtomicU64::new(0);

/// Runs soffice with `args`, stopping it (and what it started) after `timeout`. Returns what it printed.
fn run(soffice: &Path, args: &[String], timeout: Duration) -> Result<String, String> {
    let mut cmd = Command::new(soffice);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start LibreOffice: {e}"))?;
    #[cfg(windows)]
    let job = job::Job::new().and_then(|j| j.add(&child).map(|_| j)).ok();
    let mut out = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = out.as_mut() {
            let _ = std::io::Read::read_to_string(o, &mut s);
        }
        s
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > timeout => {
                #[cfg(windows)]
                if let Some(j) = &job {
                    j.stop();
                }
                let _ = child.kill();
                let _ = child.wait();
                return Err("LibreOffice took too long and was stopped.".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(reader.join().unwrap_or_default())
}

/// Converts `input` to the PDF `output`. `profile` is LibreOffice's settings folder for us.
pub fn convert(lo: &Lo, profile: &Path, input: &str, output: &Path, timeout: Duration) -> Result<(), String> {
    convert_as(lo, profile, input, output, None, "pdf", timeout)
}

/// Converts `input` into `output` of type `to` ("pdf", "pptx"…), reading it with the import `filter` if given
/// (e.g. "impress_pdf_import" to read a PDF as a presentation).
pub fn convert_as(lo: &Lo, profile: &Path, input: &str, output: &Path, filter: Option<&str>, to: &str, timeout: Duration) -> Result<(), String> {
    let _one = BUSY.lock();
    if lo.pack {
        if let Some(dir) = lo.soffice.parent().and_then(|p| p.parent()) {
            sync_cloud_fonts(dir);
        }
    }
    let work = output.with_file_name(format!("lo-{}-{}", std::process::id(), RUN.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let mut args = vec![
        format!("-env:UserInstallation={}", file_url(profile)),
        "--headless".into(),
        "--norestore".into(),
        "--nologo".into(),
        "--nodefault".into(),
        "--nolockcheck".into(),
    ];
    if let Some(f) = filter {
        args.push(format!("--infilter={f}"));
    }
    args.extend([
        "--convert-to".into(),
        to.into(),
        "--outdir".into(),
        work.to_string_lossy().into_owned(),
        std::path::absolute(input).map_or_else(|_| input.to_string(), |p| p.to_string_lossy().into_owned()),
    ]);
    let res = run(&lo.soffice, &args, timeout).and_then(|printed| {
        let made = std::fs::read_dir(&work).ok().and_then(|mut d| d.find_map(|e| e.ok().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(to)))));
        match made {
            Some(made) => {
                let _ = std::fs::remove_file(output);
                std::fs::rename(&made, output).or_else(|_| std::fs::copy(&made, output).map(drop)).map_err(|e| e.to_string())
            }
            None if printed.contains("could not be loaded") || printed.to_lowercase().contains("password") => {
                Err("LibreOffice couldn't open the file (damaged, or protected with a password).".into())
            }
            None => Err("LibreOffice couldn't convert the file.".into()),
        }
    });
    let _ = std::fs::remove_dir_all(&work);
    res
}

/// Fonts Microsoft Office downloads on demand (Aptos, its default since 2023, and others) live in a folder
/// only Office reads. The pack loads fonts from its own folder too, so copies go there; without them
/// LibreOffice draws those documents in a stand-in font and lines break differently.
fn sync_cloud_fonts(pack: &Path) {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else { return };
    let cache = PathBuf::from(local).join("Microsoft").join("FontCache");
    let dest = pack.join("share").join("fonts").join("truetype");
    let Ok(versions) = std::fs::read_dir(&cache) else { return };
    for v in versions.flatten() {
        let Ok(families) = std::fs::read_dir(v.path().join("CloudFonts")) else { continue };
        for fam in families.flatten() {
            let Ok(files) = std::fs::read_dir(fam.path()) else { continue };
            let family: String = fam.file_name().to_string_lossy().chars().filter(|c| c.is_ascii_alphanumeric()).collect();
            for f in files.flatten() {
                let name = f.file_name().to_string_lossy().into_owned();
                if !(name.ends_with(".ttf") || name.ends_with(".otf") || name.ends_with(".ttc")) {
                    continue;
                }
                let to = dest.join(format!("office-{family}-{name}"));
                if !to.exists() {
                    let _ = std::fs::copy(f.path(), &to);
                }
            }
        }
    }
}

// ---------------------------------------------------------------- the pack

/// Steps of setting up the pack, for the progress display.
#[derive(Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    Finding,
    Downloading,
    Checking,
    Unpacking,
    Preparing,
}

/// The newest release in the stable folder (its listing names one folder per version).
fn latest_version(cancel: &AtomicBool) -> Result<String, String> {
    let listing = crate::net::text(HOST, "/libreoffice/stable/", cancel)?;
    let mut best: Option<(Vec<u32>, String)> = None;
    for part in listing.split("href=\"").skip(1) {
        let v = part.split('/').next().unwrap_or("");
        let nums: Vec<u32> = v.split('.').map_while(|n| n.parse().ok()).collect();
        if nums.len() == 3 && v.split('.').count() == 3 && best.as_ref().is_none_or(|(b, _)| nums > *b) {
            best = Some((nums, v.to_string()));
        }
    }
    best.map(|b| b.1).ok_or_else(|| "Couldn't find LibreOffice's latest version.".into())
}

/// Free space on the drive holding `dir`, in bytes.
#[cfg(windows)]
fn free_space(dir: &Path) -> Option<u64> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut free = 0u64;
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(dir.as_os_str()), Some(&mut free), None, None) }.ok()?;
    Some(free)
}

/// Downloads LibreOffice's Windows installer (the newest stable release), checks it against the SHA-256 the
/// Document Foundation publishes and its code signature, and unpacks it into `dir` without installing
/// anything. Parts a converter doesn't need (help, other languages' menus and dictionaries, icon themes)
/// are left out: about 600 MB instead of 1.6 GB. `work` holds the download meanwhile.
#[cfg_attr(not(windows), allow(unreachable_code))]
pub fn install(dir: &Path, work: &Path, profile: &Path, cancel: &AtomicBool, progress: &mut dyn FnMut(Stage, u64, u64)) -> Result<Lo, String> {
    // Linux: programs come from the distribution, which keeps them up to date.
    #[cfg(not(windows))]
    {
        let _ = (dir, work, profile, cancel, progress);
        return Err(NOT_HERE.into());
    }
    #[cfg(windows)]
    std::fs::create_dir_all(work).map_err(|e| format!("Couldn't create {}: {e}", work.display()))?;
    #[cfg(windows)]
    if free_space(work).is_some_and(|f| f < 2_300 << 20) {
        return Err("Setting up LibreOffice needs about 2.3 GB of free space while it unpacks (600 MB afterwards). Free some space and try again.".into());
    }
    progress(Stage::Finding, 0, 0);
    // The add-on list may name a release Paginaut is known to work with (and its checksum); otherwise the newest.
    let pin = crate::addons::libreoffice();
    let version = match &pin {
        Some(p) => p.version.clone(),
        None => latest_version(cancel)?,
    };
    let folder = format!("/libreoffice/stable/{version}/win/x86_64/");
    let name = format!("LibreOffice_{version}_Win_x86-64.msi");
    let want = match pin {
        Some(p) => p.sha256.to_ascii_lowercase(),
        None => {
            let sums = crate::net::text(HOST, &format!("{folder}{name}.sha256"), cancel)?;
            sums.split_whitespace().next().filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit())).ok_or("Couldn't read LibreOffice's published checksum.")?.to_ascii_lowercase()
        }
    };
    let msi = work.join(&name);
    let part = work.join(format!("{name}.part"));
    let path = format!("{folder}{name}");
    let mut on_bytes = |done: u64, total: u64| progress(Stage::Downloading, done, total);
    let got = crate::net::to_file_resuming(HOST, &path, &part, cancel, &mut on_bytes, 1 << 30, crate::net::Alg::Sha256).inspect_err(|_| {
        let _ = std::fs::remove_file(&part);
    })?;
    progress(Stage::Checking, 0, 0);
    if crate::net::hex(&got) != want {
        let _ = std::fs::remove_file(&part);
        return Err("The download didn't match the checksum the Document Foundation publishes; nothing was installed. Try again.".into());
    }
    let _ = std::fs::remove_file(&msi);
    std::fs::rename(&part, &msi).map_err(|e| e.to_string())?;
    if cancel.load(Ordering::Acquire) {
        let _ = std::fs::remove_file(&msi);
        return Err("stopped".into());
    }
    let r = set_up(&msi, &version, &want, dir, profile, progress);
    let _ = std::fs::remove_file(&msi);
    r
}

/// Sets the pack up from LibreOffice's installer `msi` (already checked against `sha256`): checks its code
/// signature, unpacks it, leaves out what isn't needed, and builds LibreOffice's profile.
pub fn set_up(msi: &Path, version: &str, sha256: &str, dir: &Path, profile: &Path, progress: &mut dyn FnMut(Stage, u64, u64)) -> Result<Lo, String> {
    #[cfg(windows)]
    trust::signed_by(msi, "The Document Foundation")?;
    progress(Stage::Unpacking, 0, 0);
    let staging = dir.with_extension("part");
    let _ = std::fs::remove_dir_all(&staging);
    if let Err(e) = unpack(msi, &staging).and_then(|()| trim(&staging)) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    let info = PackInfo { version: version.to_string(), sha256: sha256.to_string() };
    std::fs::write(staging.join(PACK_INFO), serde_json::to_vec(&info).unwrap()).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(dir);
    std::fs::rename(&staging, dir).map_err(|e| format!("Couldn't finish setting up LibreOffice: {e}"))?;

    // The first start builds LibreOffice's profile (several seconds): do it now rather than on the first file.
    progress(Stage::Preparing, 0, 0);
    let lo = pack(dir).ok_or("LibreOffice didn't unpack completely.")?;
    let _one = BUSY.lock();
    sync_cloud_fonts(dir);
    let _ = run(&lo.soffice, &[format!("-env:UserInstallation={}", file_url(profile)), "--headless".into(), "--norestore".into(), "--terminate_after_init".into()], Duration::from_secs(120));
    Ok(lo)
}

/// An administrative install: Windows Installer copies the files out of the package into `to`, and
/// installs nothing.
#[cfg_attr(not(windows), allow(unused_variables))]
fn unpack(msi: &Path, to: &Path) -> Result<(), String> {
    let full = |p: &Path| std::path::absolute(p).map_err(|e| e.to_string());
    let (msi, to) = (full(msi)?, full(to)?);
    let mut cmd = Command::new("msiexec.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // msiexec reads its own command line: the paths are quoted its way.
        cmd.raw_arg(format!("/a \"{}\" /qn TARGETDIR=\"{}\"", msi.display(), to.display()));
    }
    let status = cmd.status().map_err(|e| format!("Couldn't start Windows Installer: {e}"))?;
    match status.code() {
        Some(0) => Ok(()),
        Some(1618) => Err("Another installation is running. Try again when it's finished.".into()),
        Some(c) => Err(format!("Windows Installer couldn't unpack LibreOffice (error {c}).")),
        None => Err("Windows Installer stopped.".into()),
    }
}

/// Leaves out what converting doesn't use, and puts the rest where LibreOffice looks for it: its fonts in
/// its own font folder (an install would put them in Windows'), the C++ runtime next to the program.
fn trim(root: &Path) -> Result<(), String> {
    use std::fs;
    let program = root.join("program");
    if !program.join("soffice.exe").is_file() {
        return Err("LibreOffice didn't unpack completely.".into());
    }
    if let Ok(dlls) = fs::read_dir(root.join("System64")) {
        for f in dlls.flatten() {
            let to = program.join(f.file_name());
            if !to.exists() {
                let _ = fs::rename(f.path(), to);
            }
        }
    }
    let fonts = root.join("share").join("fonts").join("truetype");
    fs::create_dir_all(&fonts).map_err(|e| e.to_string())?;
    if let Ok(list) = fs::read_dir(root.join("Fonts")) {
        for f in list.flatten() {
            let _ = fs::rename(f.path(), fonts.join(f.file_name()));
        }
    }
    for d in ["Fonts", "System", "System64", "help", "readmes", "share/gallery", "share/xpdfimport"] {
        let _ = fs::remove_dir_all(root.join(d));
    }
    // Keep only what's needed in folders with one entry per language or theme.
    let keep_only = |dir: PathBuf, keep: &dyn Fn(&str) -> bool| {
        if let Ok(list) = fs::read_dir(&dir) {
            for e in list.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if !keep(&name) {
                    let p = e.path();
                    let _ = if p.is_dir() { fs::remove_dir_all(p) } else { fs::remove_file(p) };
                }
            }
        }
    };
    keep_only(program.join("resource"), &|n| n == "common");
    keep_only(root.join("share").join("extensions"), &|n| n == "dict-en");
    keep_only(root.join("share").join("registry").join("res"), &|n| n.contains("en-US"));
    keep_only(root.join("share").join("config"), &|n| !n.starts_with("images_") || n == "images_colibre.zip");
    keep_only(root.to_path_buf(), &|n| !n.to_ascii_lowercase().ends_with(".msi"));
    // No crash-report prompts from a converter.
    let ini = program.join("soffice.ini");
    if let Ok(s) = fs::read_to_string(&ini) {
        let _ = fs::write(&ini, s.replace("CrashDumpEnable=true", "CrashDumpEnable=false"));
    }
    Ok(())
}

/// Size of a folder, in bytes.
pub fn size_of(dir: &Path) -> u64 {
    let Ok(list) = std::fs::read_dir(dir) else { return 0 };
    list.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => size_of(&e.path()),
            _ => e.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// Processes that end together: closing the job (or Paginaut closing) ends every process in it.
    pub struct Job(HANDLE);

    impl Job {
        pub fn new() -> Result<Job, String> {
            unsafe {
                let h = CreateJobObjectW(None, None).map_err(|e| e.to_string())?;
                let job = Job(h);
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(h, JobObjectExtendedLimitInformation, (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(), std::mem::size_of_val(&info) as u32)
                    .map_err(|e| e.to_string())?;
                Ok(job)
            }
        }
        pub fn add(&self, child: &std::process::Child) -> Result<(), String> {
            unsafe { AssignProcessToJobObject(self.0, HANDLE(child.as_raw_handle())).map_err(|e| e.to_string()) }
        }
        pub fn stop(&self) {
            unsafe {
                let _ = TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

#[cfg(windows)]
mod trust {
    use std::path::Path;
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Security::Cryptography::{CertGetNameStringW, CERT_NAME_SIMPLE_DISPLAY_TYPE};
    use windows::Win32::Security::WinTrust::{
        WTHelperGetProvCertFromChain, WTHelperGetProvSignerFromChain, WTHelperProvDataFromStateData, WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2,
        WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
    };

    /// Checks the file's code signature is valid (Windows trusts it) and was made by `signer`.
    pub fn signed_by(file: &Path, signer: &str) -> Result<(), String> {
        let path = HSTRING::from(file.as_os_str());
        let mut fi = WINTRUST_FILE_INFO { cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32, pcwszFilePath: PCWSTR(path.as_ptr()), ..Default::default() };
        let mut data = WINTRUST_DATA {
            cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_NONE,
            dwUnionChoice: WTD_CHOICE_FILE,
            Anonymous: WINTRUST_DATA_0 { pFile: &mut fi },
            dwStateAction: WTD_STATEACTION_VERIFY,
            ..Default::default()
        };
        let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let status = unsafe { WinVerifyTrust(HWND::default(), &mut action, (&mut data as *mut WINTRUST_DATA).cast()) };
        let name = unsafe {
            let prov = WTHelperProvDataFromStateData(data.hWVTStateData);
            let sgnr = if prov.is_null() { std::ptr::null_mut() } else { WTHelperGetProvSignerFromChain(prov, 0, false, 0) };
            let cert = if sgnr.is_null() { std::ptr::null_mut() } else { WTHelperGetProvCertFromChain(sgnr, 0) };
            if cert.is_null() || (*cert).pCert.is_null() {
                String::new()
            } else {
                let mut buf = [0u16; 256];
                let n = CertGetNameStringW((*cert).pCert, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, Some(&mut buf));
                String::from_utf16_lossy(&buf[..(n as usize).saturating_sub(1)])
            }
        };
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        unsafe { WinVerifyTrust(HWND::default(), &mut action, (&mut data as *mut WINTRUST_DATA).cast()) };
        if status != 0 {
            return Err(format!("LibreOffice's installer isn't validly signed (0x{:08X}); nothing was installed.", status as u32));
        }
        if name != signer {
            return Err(format!("LibreOffice's installer is signed by \"{name}\", not {signer}; nothing was installed."));
        }
        Ok(())
    }
}

#[cfg(windows)]
mod reg {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ};

    pub const HKLM: HKEY = HKEY_LOCAL_MACHINE;
    pub const HKCU: HKEY = HKEY_CURRENT_USER;

    pub fn string(root: HKEY, key: &str, value: &str) -> Option<String> {
        let mut buf = [0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        unsafe { RegGetValueW(root, &HSTRING::from(key), &HSTRING::from(value), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len)) }.ok().ok()?;
        let s = String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]);
        (!s.is_empty()).then_some(s)
    }

    pub fn subkeys(root: HKEY, key: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut h = HKEY::default();
        unsafe {
            if RegOpenKeyExW(root, &HSTRING::from(key), None, KEY_READ, &mut h).is_err() {
                return out;
            }
            for i in 0.. {
                let mut name = [0u16; 256];
                let mut len = name.len() as u32;
                if RegEnumKeyExW(h, i, Some(windows::core::PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None).is_err() {
                    break;
                }
                out.push(String::from_utf16_lossy(&name[..len as usize]));
            }
            let _ = RegCloseKey(h);
        }
        out
    }
}
