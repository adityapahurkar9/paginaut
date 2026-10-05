//! Paginaut as a choice for its file types in Windows ("Open with" and Settings › Default apps), never taking
//! any over: Windows 10 and 11 let only the user choose a default, so the app registers itself and then opens
//! Default apps at Paginaut for the user to confirm. Everything lives under HKEY_CURRENT_USER; the installer
//! runs `paginaut --register-types` after installing and `--unregister-types` before uninstalling. Only an
//! installed copy offers itself (a development build or a copied folder would take the installed copy's place).
//!
//! Windows resets a default whose ProgID disappears, so ProgIDs are never deleted while Paginaut is installed,
//! and an upgrade that uninstalls the old version first (the installer's recommended choice) is bridged: the
//! uninstall leaves a note of what was offered, and the install that follows offers it again and recreates any
//! ProgID a default still names.

use serde::Serialize;

/// A kind of file Paginaut can be offered for: one ProgID ("Paginaut.<id>") for all its extensions.
pub struct Kind {
    pub id: &'static str,
    pub name: &'static str,
    pub exts: &'static [&'static str],
    /// Offered on a first install.
    pub core: bool,
    /// Files other apps own (Office files, pictures): shown apart, and offered only when picked.
    pub others: bool,
}

pub const KINDS: &[Kind] = &[
    Kind { id: "pdf", name: "PDF document", exts: &["pdf"], core: true, others: false },
    Kind { id: "xps", name: "XPS document", exts: &["xps", "oxps"], core: true, others: false },
    Kind { id: "ebook", name: "E-book", exts: &["epub", "mobi", "fb2", "fbz"], core: true, others: false },
    Kind { id: "comic", name: "Comic book", exts: &["cbz", "cbr"], core: true, others: false },
    Kind { id: "markdown", name: "Markdown document", exts: &["md", "markdown"], core: false, others: false },
    Kind {
        id: "picture",
        name: "Picture",
        exts: &["png", "jpg", "jpeg", "jfif", "tif", "tiff", "bmp", "gif", "jp2", "jpx", "psd", "pnm", "pbm", "pgm", "ppm", "pam", "jxr"],
        core: false,
        others: true,
    },
    Kind { id: "word", name: "Word document", exts: &["docx", "doc", "docm", "dotx", "dot", "dotm", "rtf", "odt", "ott"], core: false, others: true },
    Kind { id: "excel", name: "Spreadsheet", exts: &["xlsx", "xls", "xlsm", "xlsb", "xltx", "xlt", "ods", "ots"], core: false, others: true },
    Kind { id: "powerpoint", name: "Presentation", exts: &["pptx", "ppt", "pptm", "ppsx", "pps", "potx", "pot", "odp", "otp"], core: false, others: true },
];

/// ProgIDs the installers of 0.7 and earlier made (Tauri's own associations), cleaned up on the way.
const LEGACY: &[(&str, &str)] = &[("pdf", "PDF document"), ("xps", "XPS document"), ("oxps", "XPS document"), ("epub", "EPUB e-book"), ("cbz", "Comic book archive")];

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KindStatus {
    pub id: &'static str,
    pub name: &'static str,
    pub exts: &'static [&'static str],
    pub core: bool,
    pub others: bool,
    /// Paginaut is offered for these files.
    pub offered: bool,
    /// How many of its extensions open in this copy of Paginaut now.
    pub default_for: usize,
}

/// This copy was installed (the installer puts its uninstaller next to it), so it may offer itself in Windows.
pub fn installed(exe: &std::path::Path) -> bool {
    exe.parent().is_some_and(|d| d.join("uninstall.exe").is_file())
}

/// The program a ProgID's command runs (`"C:\...\paginaut.exe" "%1"` or unquoted).
fn command_exe(command: &str) -> std::path::PathBuf {
    let c = command.trim();
    let path = match c.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or(""),
        None => c.to_ascii_lowercase().find(".exe").map_or(c, |i| &c[..i + 4]),
    };
    std::path::PathBuf::from(path)
}

/// Where the keys go: Paginaut's own names, or a test's.
pub struct Names {
    pub app: &'static str,
    pub exe_name: &'static str,
}

pub const PAGINAUT: Names = Names { app: "Paginaut", exe_name: "paginaut.exe" };

#[cfg(windows)]
mod reg {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Registry::{
        RegDeleteKeyValueW, RegDeleteKeyW, RegDeleteTreeW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_NONE, REG_SZ, RRF_RT_REG_SZ,
    };

    pub fn set(key: &str, name: Option<&str>, value: &str) -> Result<(), String> {
        let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let name = name.map(HSTRING::from);
        // SAFETY: the key and value name are NUL-terminated wide strings that outlive the call; `data` is
        // NUL-terminated and its byte length is passed.
        let r = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(key),
                name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr())),
                REG_SZ.0,
                Some(data.as_ptr().cast()),
                (data.len() * 2) as u32,
            )
        };
        r.ok().map_err(|e| format!("Couldn't write {key}: {e}"))
    }

    /// An empty value, as Windows' lists of ProgIDs and types use.
    pub fn set_none(key: &str, name: &str) -> Result<(), String> {
        // SAFETY: as in `set`, with no data.
        let r = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name), REG_NONE.0, None, 0) };
        r.ok().map_err(|e| format!("Couldn't write {key}: {e}"))
    }

    pub fn get(key: &str, name: Option<&str>) -> Option<String> {
        let name = name.map(HSTRING::from);
        let name = name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr()));
        let mut buf = vec![0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        // SAFETY: `buf` is writable for `len` bytes; the key and value name are NUL-terminated.
        let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(key), name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len)) };
        r.ok().ok()?;
        let chars = (len as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buf[..chars.min(buf.len())]))
    }

    #[cfg(test)]
    pub fn exists(key: &str) -> bool {
        use windows::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW, HKEY, KEY_READ};
        let mut h = HKEY::default();
        // SAFETY: the key name is NUL-terminated; an opened key is closed.
        unsafe {
            let ok = RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(key), None, KEY_READ, &mut h).is_ok();
            if ok {
                let _ = RegCloseKey(h);
            }
            ok
        }
    }

    pub fn delete_tree(key: &str) {
        // SAFETY: the key name is a NUL-terminated wide string.
        let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, &HSTRING::from(key)) };
        // RegDeleteTreeW empties the key; the key itself goes with a second call.
        // SAFETY: as above.
        let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(key)) };
    }

    /// Deletes a key only if it has no keys under it (Windows refuses otherwise).
    pub fn delete_if_empty(key: &str) {
        // SAFETY: the key name is a NUL-terminated wide string.
        let _ = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, &HSTRING::from(key)) };
    }

    pub fn delete_value(key: &str, name: &str) {
        // SAFETY: both names are NUL-terminated wide strings.
        let _ = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from(name)) };
    }
}

const CLASSES: &str = "Software\\Classes";

fn prog_id(n: &Names, k: &Kind) -> String {
    format!("{}.{}", n.app, k.id)
}

fn quoted_exe(exe: &std::path::Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Offers the app for exactly the kinds in `ids` (and withdraws it from the rest), pointing at `exe`.
#[cfg(windows)]
pub fn register(n: &Names, exe: &std::path::Path, ids: &[&str]) -> Result<(), String> {
    let cmd = format!("{} \"%1\"", quoted_exe(exe));
    let icon = format!("{},0", quoted_exe(exe));
    let caps = format!("Software\\{}\\Capabilities", n.app);
    let app_key = format!("{CLASSES}\\Applications\\{}", n.exe_name);
    // Kinds that were unticked go. The ticked ones are only overwritten, never deleted first: a ProgID can be the
    // user's default, and Windows resets a default whose ProgID disappears, even for a moment.
    let (offered, withdrawn): (Vec<&Kind>, Vec<&Kind>) = KINDS.iter().partition(|k| ids.contains(&k.id));
    remove_kinds(n, &withdrawn);
    reg::delete_tree(&format!("{caps}\\FileAssociations"));
    reg::delete_tree(&format!("{app_key}\\SupportedTypes"));
    if offered.is_empty() {
        unregister_keys(n);
        notify();
        return Ok(());
    }
    reg::set(&caps, Some("ApplicationName"), n.app)?;
    reg::set(&caps, Some("ApplicationDescription"), "Read, edit, sign and convert PDFs and other documents.")?;
    reg::set(&caps, Some("ApplicationIcon"), &icon)?;
    reg::set(&app_key, Some("FriendlyAppName"), n.app)?;
    reg::set(&format!("{app_key}\\DefaultIcon"), None, &icon)?;
    reg::set(&format!("{app_key}\\shell\\open\\command"), None, &cmd)?;
    for k in offered {
        let pid = prog_id(n, k);
        let key = format!("{CLASSES}\\{pid}");
        reg::set(&key, None, k.name)?;
        reg::set(&key, Some("FriendlyTypeName"), k.name)?;
        reg::set(&format!("{key}\\DefaultIcon"), None, &icon)?;
        reg::set(&format!("{key}\\shell\\open\\command"), None, &cmd)?;
        for ext in k.exts {
            reg::set_none(&format!("{CLASSES}\\.{ext}\\OpenWithProgids"), &pid)?;
            reg::set_none(&format!("{app_key}\\SupportedTypes"), &format!(".{ext}"))?;
            reg::set(&format!("{caps}\\FileAssociations"), Some(&format!(".{ext}")), &pid)?;
        }
    }
    reg::set("Software\\RegisteredApplications", Some(n.app), &caps)?;
    refresh_legacy(&cmd, &icon);
    notify();
    Ok(())
}

/// Withdraws the app from every file type.
#[cfg(windows)]
pub fn unregister(n: &Names, exe: &std::path::Path) {
    unregister_keys(n);
    remove_legacy(exe);
    notify();
}

/// The note an uninstall leaves for an install that follows at once (an upgrade that uninstalls first).
fn carryover(n: &Names) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("{} file types.txt", n.app))
}

/// The installer's `--unregister-types`: withdraws everything, noting first what was offered.
#[cfg(windows)]
pub fn unregister_for_uninstall(n: &Names, exe: &std::path::Path) {
    let offered: Vec<&str> = status(n, exe).into_iter().filter(|k| k.offered).map(|k| k.id).collect();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let _ = std::fs::write(carryover(n), format!("{now}\n{}", offered.join(",")));
    unregister(n, exe);
}

/// What the note says, if it was left in the last hour (it's removed either way).
fn take_carryover(n: &Names) -> Vec<&'static str> {
    let path = carryover(n);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let _ = std::fs::remove_file(&path);
    let mut lines = text.lines();
    let at: u64 = lines.next().and_then(|l| l.trim().parse().ok()).unwrap_or(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    if now.saturating_sub(at) > 3600 {
        return Vec::new();
    }
    let ids = lines.next().unwrap_or("");
    KINDS.iter().filter(|k| ids.split(',').any(|i| i.trim() == k.id)).map(|k| k.id).collect()
}

/// The ProgIDs the user's defaults name for an extension (Windows keeps the choice in UserChoice, and in
/// UserChoiceLatest on newer versions).
#[cfg(windows)]
fn chosen(ext: &str) -> Vec<String> {
    let base = format!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\.{ext}");
    [reg::get(&format!("{base}\\UserChoice"), Some("ProgId")), reg::get(&format!("{base}\\UserChoiceLatest\\ProgId"), Some("ProgId"))]
        .into_iter()
        .flatten()
        .collect()
}

#[cfg(windows)]
fn remove_kinds(n: &Names, kinds: &[&Kind]) {
    for k in kinds {
        let pid = prog_id(n, k);
        reg::delete_tree(&format!("{CLASSES}\\{pid}"));
        for ext in k.exts {
            reg::delete_value(&format!("{CLASSES}\\.{ext}\\OpenWithProgids"), &pid);
        }
    }
}

#[cfg(windows)]
fn unregister_keys(n: &Names) {
    remove_kinds(n, &KINDS.iter().collect::<Vec<_>>());
    reg::delete_tree(&format!("{CLASSES}\\Applications\\{}", n.exe_name));
    reg::delete_tree(&format!("Software\\{}\\Capabilities", n.app));
    reg::delete_if_empty(&format!("Software\\{}", n.app));
    reg::delete_value("Software\\RegisteredApplications", n.app);
}

/// The ProgIDs earlier installers made are kept, because one may be the user's default (Windows resets a default
/// whose ProgID disappears). One whose program is gone (moved, or uninstalled before this install) is pointed
/// at this copy; one a default still names but that an old uninstaller removed is made again. A ProgID that
/// opens another copy that's still there is left alone. Only uninstalling removes them.
#[cfg(windows)]
fn refresh_legacy(cmd: &str, icon: &str) {
    let mut seen = Vec::new();
    for (_, class) in LEGACY {
        if seen.contains(class) {
            continue;
        }
        seen.push(class);
        let key = format!("{CLASSES}\\{class}");
        let command = reg::get(&format!("{key}\\shell\\open\\command"), None);
        let point_here = match &command {
            Some(c) => is_paginaut(c) && !command_exe(c).is_file(),
            // Gone: made again if a default names it (an old uninstaller removed it before this install).
            None => LEGACY.iter().filter(|(_, c)| c == class).any(|(e, _)| chosen(e).iter().any(|p| p == class)),
        };
        if point_here {
            let _ = reg::set(&key, None, class);
            let _ = reg::set(&format!("{key}\\shell\\open\\command"), None, cmd);
            let _ = reg::set(&format!("{key}\\DefaultIcon"), None, icon);
        }
    }
}

/// A ProgID command that runs a Paginaut.
fn is_paginaut(command: &str) -> bool {
    command_exe(command).file_name().is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case("paginaut.exe"))
}

/// The ProgIDs earlier installers made, if they open this copy (or a Paginaut that's gone): removed, and the
/// type's own class put back (uninstalling only).
#[cfg(windows)]
fn remove_legacy(exe: &std::path::Path) {
    let ours = |c: &str| is_paginaut(c) && (same_file(&command_exe(c).to_string_lossy(), exe) || !command_exe(c).is_file());
    for (ext, class) in LEGACY {
        let command = reg::get(&format!("{CLASSES}\\{class}\\shell\\open\\command"), None);
        if !command.is_some_and(|c| ours(&c)) {
            continue;
        }
        let key = format!("{CLASSES}\\.{ext}");
        let backup_name = format!("{class}_backup");
        if reg::get(&key, None).as_deref() == Some(class) {
            match reg::get(&key, Some(&backup_name)) {
                Some(b) if !b.is_empty() && b != *class => reg::set(&key, None, &b).unwrap_or(()),
                _ => reg::delete_value(&key, ""),
            }
        }
        reg::delete_value(&key, &backup_name);
    }
    for (_, class) in LEGACY {
        let command = reg::get(&format!("{CLASSES}\\{class}\\shell\\open\\command"), None);
        if command.is_some_and(|c| ours(&c)) {
            reg::delete_tree(&format!("{CLASSES}\\{class}"));
        }
    }
}

#[cfg(windows)]
fn notify() {
    use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
    // SAFETY: SHCNE_ASSOCCHANGED takes no items.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

/// The program a file type opens in now (Windows' own answer, whichever way the default was set).
#[cfg(windows)]
fn opens_in(ext: &str) -> Option<String> {
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::UI::Shell::{AssocQueryStringW, ASSOCF_NOTRUNCATE, ASSOCSTR_EXECUTABLE};
    let mut buf = vec![0u16; 1024];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` holds `len` characters; the association and verb are NUL-terminated.
    let r = unsafe {
        AssocQueryStringW(ASSOCF_NOTRUNCATE, ASSOCSTR_EXECUTABLE, &HSTRING::from(format!(".{ext}")), &HSTRING::from("open"), Some(PWSTR(buf.as_mut_ptr())), &mut len)
    };
    r.ok().ok()?;
    Some(String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)]))
}

fn same_file(a: &str, b: &std::path::Path) -> bool {
    let canon = |p: &std::path::Path| std::fs::canonicalize(p).ok();
    match (canon(std::path::Path::new(a)), canon(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a.eq_ignore_ascii_case(&b.to_string_lossy()),
    }
}

/// What's offered, and where this copy of the app is the default.
#[cfg(windows)]
pub fn status(n: &Names, exe: &std::path::Path) -> Vec<KindStatus> {
    KINDS
        .iter()
        .map(|k| KindStatus {
            id: k.id,
            name: k.name,
            exts: k.exts,
            core: k.core,
            others: k.others,
            offered: reg::get(&format!("{CLASSES}\\{}\\shell\\open\\command", prog_id(n, k)), None).is_some(),
            default_for: k.exts.iter().filter(|e| opens_in(e).is_some_and(|p| same_file(&p, exe))).count(),
        })
        .collect()
}

/// Settings › Default apps at this app's own page (IApplicationAssociationRegistrationUI, which Windows 10 and 11
/// send to the Settings app).
#[cfg(windows)]
pub fn open_settings(n: &Names) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{ApplicationAssociationRegistrationUI, IApplicationAssociationRegistrationUI};
    // SAFETY: COM is started on this (blocking pool) thread for the call and ended after; the interface is
    // released before CoUninitialize.
    unsafe {
        let started = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let r = (|| {
            let ui: IApplicationAssociationRegistrationUI = CoCreateInstance(&ApplicationAssociationRegistrationUI, None, CLSCTX_INPROC_SERVER)?;
            ui.LaunchAdvancedAssociationUI(&HSTRING::from(n.app))
        })();
        if started {
            CoUninitialize();
        }
        r.map_err(|e| e.to_string())
    }
}

#[cfg(not(windows))]
pub fn open_settings(_: &Names) -> Result<(), String> {
    Err("Only on Windows.".into())
}

/// The installer's `--register-types`: the kinds offered before (still registered, or noted by the uninstall of an
/// upgrade), plus any a default still names; on a first install the core ones.
#[cfg(windows)]
pub fn register_for_install(n: &Names, exe: &std::path::Path) -> Result<(), String> {
    let mut ids: Vec<&str> = status(n, exe).into_iter().filter(|s| s.offered).map(|s| s.id).collect();
    ids.extend(take_carryover(n));
    for k in KINDS {
        let pid = prog_id(n, k);
        if k.exts.iter().any(|e| chosen(e).contains(&pid)) {
            ids.push(k.id);
        }
    }
    if ids.is_empty() {
        ids = KINDS.iter().filter(|k| k.core).map(|k| k.id).collect();
    }
    ids.sort_unstable();
    ids.dedup();
    register(n, exe, &ids)
}

#[cfg(not(windows))]
pub fn status(_: &Names, _: &std::path::Path) -> Vec<KindStatus> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn register(_: &Names, _: &std::path::Path, _: &[&str]) -> Result<(), String> {
    Err("Only on Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST: Names = Names { app: "PaginautTestTypes", exe_name: "paginaut-test-types.exe" };
    // Its own names: tests run at the same time.
    const TEST_UPGRADE: Names = Names { app: "PaginautTestUpgrade", exe_name: "paginaut-test-upgrade.exe" };

    #[test]
    fn kinds_have_unique_ids_and_extensions() {
        let mut ids = std::collections::HashSet::new();
        let mut exts = std::collections::HashSet::new();
        for k in KINDS {
            assert!(ids.insert(k.id), "{}", k.id);
            for e in k.exts {
                assert!(exts.insert(*e), "{e}");
                assert_eq!(*e, e.to_ascii_lowercase());
            }
        }
    }

    /// Opens Windows' Settings on screen, so only when asked: `cargo test --lib open_settings -- --ignored`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn open_settings_at_paginaut() {
        open_settings(&PAGINAUT).unwrap();
    }

    #[test]
    fn command_paths() {
        assert_eq!(command_exe(r#""C:\Program Files\P\paginaut.exe" "%1""#), std::path::PathBuf::from(r"C:\Program Files\P\paginaut.exe"));
        assert_eq!(command_exe(r#"E:\P R\Paginaut\paginaut.exe "%1""#), std::path::PathBuf::from(r"E:\P R\Paginaut\paginaut.exe"));
        assert!(is_paginaut(r#"E:\P R\Paginaut\PAGINAUT.EXE "%1""#));
        assert!(!is_paginaut(r#""C:\Program Files\Adobe\Acrobat.exe" "%1""#));
    }

    /// An uninstall's note of what was offered is picked up once by the install that follows.
    #[cfg(windows)]
    #[test]
    fn upgrade_keeps_the_choice() {
        let exe = std::env::current_exe().unwrap();
        register(&TEST_UPGRADE, &exe, &["pdf", "markdown"]).unwrap();
        unregister_for_uninstall(&TEST_UPGRADE, &exe);
        assert!(status(&TEST_UPGRADE, &exe).iter().all(|k| !k.offered));
        register_for_install(&TEST_UPGRADE, &exe).unwrap();
        let offered: Vec<&str> = status(&TEST_UPGRADE, &exe).iter().filter(|k| k.offered).map(|k| k.id).collect();
        assert_eq!(offered, ["pdf", "markdown"]);
        assert!(!carryover(&TEST_UPGRADE).exists());
        unregister(&TEST_UPGRADE, &exe);
        assert!(status(&TEST_UPGRADE, &exe).iter().all(|k| !k.offered));
    }

    /// An install over a copy that's still registered keeps every kind it offered.
    #[cfg(windows)]
    #[test]
    fn reinstall_keeps_the_choice() {
        const N: Names = Names { app: "PaginautTestReinstall", exe_name: "paginaut-test-reinstall.exe" };
        let exe = std::env::current_exe().unwrap();
        register(&N, &exe, &["pdf", "xps", "ebook", "comic", "markdown"]).unwrap();
        register_for_install(&N, &exe).unwrap();
        let offered: Vec<&str> = status(&N, &exe).iter().filter(|k| k.offered).map(|k| k.id).collect();
        unregister(&N, &exe);
        assert_eq!(offered, ["pdf", "xps", "ebook", "comic", "markdown"]);
    }

    /// Writes and removes test keys under HKEY_CURRENT_USER (never Paginaut's own names).
    #[cfg(windows)]
    #[test]
    fn register_then_unregister() {
        let exe = std::env::current_exe().unwrap();
        register(&TEST, &exe, &["pdf", "markdown"]).unwrap();
        let s = status(&TEST, &exe);
        let offered: Vec<&str> = s.iter().filter(|k| k.offered).map(|k| k.id).collect();
        assert_eq!(offered, ["pdf", "markdown"]);
        assert_eq!(reg::get("Software\\PaginautTestTypes\\Capabilities\\FileAssociations", Some(".pdf")).as_deref(), Some("PaginautTestTypes.pdf"));
        // Unticking a kind withdraws it.
        register(&TEST, &exe, &["pdf"]).unwrap();
        assert!(!status(&TEST, &exe).iter().any(|k| k.id == "markdown" && k.offered));
        assert!(reg::get("Software\\PaginautTestTypes\\Capabilities\\FileAssociations", Some(".md")).is_none());
        unregister(&TEST, &exe);
        assert!(status(&TEST, &exe).iter().all(|k| !k.offered));
        assert!(reg::get("Software\\RegisteredApplications", Some("PaginautTestTypes")).is_none());
        assert!(reg::get(&format!("{CLASSES}\\PaginautTestTypes.pdf"), None).is_none());
        assert!(!reg::exists("Software\\PaginautTestTypes"));
    }
}
