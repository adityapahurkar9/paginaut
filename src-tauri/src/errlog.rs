//! The error log: what went wrong, kept on this computer or phone so someone can hand it over when they report a
//! problem ("Report a problem" on the desktop, Settings › About on Android). Nothing here is ever sent anywhere.
//!
//! One text file, `paginaut.log`, in the app's data folder; at `CAP` it becomes `paginaut.1.log` (the one before is
//! dropped), so the log never takes more than twice `CAP`. Each line is `time [kind] message`, in UTC.
//!
//! Every message is cleaned before it's written (`scrub`): the home folder becomes `~`, and a document's path or
//! name becomes `<file>.pdf` (its extension kept, as that says what kind of file failed). A message repeated within
//! a minute is written once, with a count.

use parking_lot::Mutex;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Size at which the log rolls over.
pub const CAP: u64 = 256 * 1024;
/// The longest message kept (a long stack is cut).
const MAX_MESSAGE: usize = 2048;
/// A message repeated within this long is counted rather than written again.
const REPEAT: Duration = Duration::from_secs(60);
const FILE: &str = "paginaut.log";
const OLD: &str = "paginaut.1.log";
/// An Android crash, written raw by the Kotlin side as it happened (MainActivity): cleaned into the log at the next
/// start, so there's one way of cleaning. Crashes in it are apart by `PENDING_SEP`.
const PENDING: &str = "crash-pending.txt";
const PENDING_SEP: &str = "\n\u{1e}\n";
/// Present while the app runs (desktop): found at start, the last session ended without closing.
const RUNNING: &str = "running";

struct Log {
    dir: PathBuf,
    home: Option<String>,
    /// The last message written, when, and how many times it came again since.
    last: Option<(String, Instant, u32)>,
}

static LOG: OnceLock<Mutex<Log>> = OnceLock::new();
/// Set at start when the last session ended in a crash (or without closing, on the desktop): the page offers a report
/// once (`take_crash_notice`).
static CRASHED: AtomicBool = AtomicBool::new(false);
/// Lines `write_limited` takes a minute, per kind; the rest are counted.
const BURST: u32 = 20;
static BURSTS: Mutex<Vec<(String, Instant, u32, u32)>> = Mutex::new(Vec::new());

/// Starts the log in `dir` (made if missing) and catches Rust panics into it. Returns true when the last session
/// ended without closing (only when `watch_exit`: the desktop, where the app closing is something it sees).
pub fn init(dir: &Path, watch_exit: bool) -> bool {
    let _ = fs::create_dir_all(dir);
    let home = home_dir();
    if LOG.set(Mutex::new(Log { dir: dir.to_path_buf(), home, last: None })).is_err() {
        return false;
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let at = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        write("panic", &format!("{what}{at} (thread {thread})"));
        prev(info);
    }));
    if let Ok(raw) = fs::read_to_string(dir.join(PENDING)) {
        let _ = fs::remove_file(dir.join(PENDING));
        for crash in raw.split(PENDING_SEP).filter(|c| !c.trim().is_empty()) {
            write("android", crash.trim());
            CRASHED.store(true, Ordering::Relaxed);
        }
    }
    if !watch_exit {
        return false;
    }
    let marker = dir.join(RUNNING);
    let unclean = marker.exists();
    if unclean {
        CRASHED.store(true, Ordering::Relaxed);
        write("start", "The last session ended without closing (a crash, or the app was stopped).");
    }
    let _ = fs::write(&marker, std::process::id().to_string());
    unclean
}

/// True once, when the last session crashed or ended without closing: the page then offers Report a problem.
pub fn take_crash_notice() -> bool {
    CRASHED.swap(false, Ordering::Relaxed)
}

/// `write`, but at most `BURST` lines of this kind a minute (MuPDF's errors: a damaged file can give hundreds); what's
/// left out is counted in one line when the next minute begins.
pub fn write_limited(kind: &str, message: &str) {
    let dropped = {
        let mut b = BURSTS.lock();
        let i = match b.iter().position(|e| e.0 == kind) {
            Some(i) => i,
            None => {
                b.push((kind.to_string(), Instant::now(), 0, 0));
                b.len() - 1
            }
        };
        let e = &mut b[i];
        let mut dropped = 0;
        if e.1.elapsed() >= REPEAT {
            dropped = e.3;
            *e = (kind.to_string(), Instant::now(), 0, 0);
        }
        if e.2 >= BURST {
            e.3 += 1;
            return;
        }
        e.2 += 1;
        dropped
    };
    if dropped > 0 {
        write(kind, &format!("({dropped} more lines of this kind in the minute before were left out)"));
    }
    write(kind, message);
}

/// The app is closing normally.
pub fn closed() {
    if let Some(log) = LOG.get() {
        let mut l = log.lock();
        flush_repeats(&mut l);
        let _ = fs::remove_file(l.dir.join(RUNNING));
    }
}

/// Writes one line to the log (cleaned of paths and names). Does nothing before `init`.
pub fn write(kind: &str, message: &str) {
    let Some(log) = LOG.get() else { return };
    // A panic while the log is held (writing failed in some odd way) mustn't deadlock the hook.
    let Some(mut l) = log.try_lock_for(Duration::from_millis(200)) else { return };
    // A stack's lines are indented under the line they belong to.
    let clean = scrub(message.trim_end(), l.home.as_deref()).replace('\n', "\n    ");
    let key = format!("[{kind}] {clean}");
    if let Some((last, when, n)) = l.last.as_mut() {
        if *last == key && when.elapsed() < REPEAT {
            *n += 1;
            return;
        }
    }
    flush_repeats(&mut l);
    let line = format!("{} {key}\n", timestamp(SystemTime::now()));
    append(&l.dir, &line, CAP);
    l.last = Some((key, Instant::now(), 0));
}

fn flush_repeats(l: &mut Log) {
    if let Some((key, _, n)) = l.last.take() {
        if n > 0 {
            let first = key.split_once("] ").map(|(k, _)| format!("{k}]")).unwrap_or_default();
            let line = format!("{} {first} (the line before came {n} more time{} within a minute)\n", timestamp(SystemTime::now()), if n == 1 { "" } else { "s" });
            append(&l.dir, &line, CAP);
        }
    }
}

/// Adds `line` to the log in `dir`, rolling it over first when it would pass `cap`.
fn append(dir: &Path, line: &str, cap: u64) {
    let path = dir.join(FILE);
    let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if size > 0 && size + line.len() as u64 > cap {
        let _ = fs::rename(&path, dir.join(OLD));
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// The whole log, oldest first (both files).
pub fn read() -> String {
    let Some(log) = LOG.get() else { return String::new() };
    let mut l = log.lock();
    flush_repeats(&mut l);
    read_dir(&l.dir)
}

fn read_dir(dir: &Path) -> String {
    let mut out = fs::read(dir.join(OLD)).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    out.push_str(&fs::read(dir.join(FILE)).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default());
    out
}

/// Empties the log (both files).
pub fn clear() {
    if let Some(log) = LOG.get() {
        let mut l = log.lock();
        l.last = None;
        let _ = fs::remove_file(l.dir.join(OLD));
        let _ = fs::remove_file(l.dir.join(FILE));
    }
}

fn home_dir() -> Option<String> {
    let h = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).ok()?;
    let h = h.trim_end_matches(['\\', '/']).to_string();
    // A home of "/" (some services) would turn every path into "~".
    (h.len() > 3).then_some(h)
}

// ---------------------------------------------------------------- cleaning

/// Source files, whose names are kept (a panic's place in the code, a script in a stack): never a document.
const CODE: &[&str] = &["rs", "kt", "java", "ts", "tsx", "js", "mjs", "c", "h", "cpp"];

/// What a document's name could end in, for names written without a folder.
const DOCUMENTS: &[&str] = &[
    "pdf", "epub", "mobi", "fb2", "fbz", "xps", "oxps", "cbz", "cbr", "cbt", "cb7", "md", "markdown", "txt", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt",
    "ods", "odp", "rtf", "png", "jpg", "jpeg", "tif", "tiff", "bmp", "gif", "webp", "svg", "jxr", "pfx", "p12", "html", "htm", "zip",
];

/// Takes the home folder, paths and document names out of `s`: a path becomes `<file>.ext` (or `<folder>`), a
/// name with a document's extension becomes `<file>.ext`. Web addresses and source-code file names stay.
pub fn scrub(s: &str, home: Option<&str>) -> String {
    let mut s = s.to_string();
    if let Some(h) = home {
        s = replace_home(&s, h);
    }
    let s = scrub_paths(&s);
    let s = scrub_names(&s);
    cut(&s, MAX_MESSAGE)
}

fn replace_home(s: &str, home: &str) -> String {
    // Windows paths may come with either slash, and in any case.
    let variants = [home.to_string(), home.replace('\\', "/"), home.replace('/', "\\")];
    let mut out = s.to_string();
    for h in variants {
        if cfg!(windows) {
            // ASCII lowering keeps every byte where it was.
            let lower = out.to_ascii_lowercase();
            let hl = h.to_ascii_lowercase();
            let mut res = String::new();
            let mut i = 0;
            while let Some(p) = lower[i..].find(&hl) {
                res.push_str(&out[i..i + p]);
                res.push('~');
                i += p + hl.len();
            }
            res.push_str(&out[i..]);
            out = res;
        } else {
            out = out.replace(&h, "~");
        }
    }
    out
}

fn cut(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… (cut)", &s[..end])
}

/// Whether a path starts at byte `i` of `s`.
fn path_start(s: &str, i: usize) -> bool {
    let b = s.as_bytes();
    let before = if i == 0 { None } else { s[..i].chars().next_back() };
    let edge = matches!(before, None | Some(' ' | '\t' | '\n' | '"' | '\'' | '(' | '[' | '=' | ':' | ',' | '`' | '“' | '‘' | '<'));
    let rest = &s[i..];
    // C:\ or C:/ (a drive), \\server (a share), ~/ or ~\ (home), /something (Unix), file:// and content:// (Android).
    if b[i].is_ascii_alphabetic() && rest.len() > 2 && b[i + 1] == b':' && (b[i + 2] == b'\\' || b[i + 2] == b'/') {
        return before.map_or(true, |c| !c.is_alphanumeric());
    }
    if rest.starts_with("\\\\") || rest.starts_with("file://") || rest.starts_with("content://") {
        return edge;
    }
    if rest.starts_with("~/") || rest.starts_with("~\\") {
        return edge || before == Some('~');
    }
    if b[i] == b'/' && edge && before != Some(':') {
        // Not a lone slash, nor // (a comment, or a URL's).
        return rest.len() > 1 && (rest.as_bytes()[1].is_ascii_alphanumeric() || rest.as_bytes()[1] == b'.' || rest.as_bytes()[1] == b'_');
    }
    false
}

/// The end of a path that starts at `i`. Names have spaces, so it isn't the first space: it's the first extension in
/// the last part (after the last slash) that has one, or, with no extension anywhere, that part's first word. The
/// path ends sooner at a quote, a line's end, or another path starting.
fn path_end(s: &str, i: usize) -> usize {
    let mut stop = s[i..].find(['\n', '"', '\'', '`', '”', '’', '<', '>', '|', '*', '?', '\t']).map_or(s.len(), |p| i + p);
    if let Some((j, _)) = s[i..stop].char_indices().skip(1).find(|&(j, _)| s.as_bytes()[i + j - 1] == b' ' && path_start(s, i + j)) {
        stop = i + j - 1;
    }
    let span = &s[i..stop];
    let seps: Vec<usize> = span.match_indices(['/', '\\']).map(|(p, _)| p).collect();
    for &p in seps.iter().rev() {
        let start = p + 1;
        let end = span[start..].find(['/', '\\']).map_or(span.len(), |q| start + q);
        if let Some(e) = ext_end(&span[start..end]) {
            return i + start + e;
        }
    }
    let last = seps.last().map_or(0, |p| p + 1);
    i + last + span[last..].find([' ', ',', ')', ']', ';']).unwrap_or(span.len() - last)
}

/// Where the first file extension in `part` ends (".pdf" followed by the end, a space or punctuation).
fn ext_end(part: &str) -> Option<usize> {
    let b = part.as_bytes();
    for (p, _) in part.match_indices('.') {
        let n = b[p + 1..].iter().take_while(|c| c.is_ascii_alphanumeric()).count();
        if p == 0 || !(1..=8).contains(&n) {
            continue;
        }
        let a = p + 1 + n;
        let ends = a == b.len()
            || matches!(b[a], b' ' | b':' | b',' | b')' | b']' | b';')
            || (b[a] == b'.' && (a + 1 == b.len() || b[a + 1] == b' '));
        if ends {
            return Some(a);
        }
    }
    None
}

fn scrub_paths(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s.is_char_boundary(i) && !web_at(s, i) && path_start(s, i) {
            let end = path_end(s, i).max(i + 1);
            out.push_str(&replacement(&s[i..end]));
            i = end;
            continue;
        }
        if web_at(s, i) {
            // A web address stays as it is (the app's own scripts in a stack, a download's address).
            let end = s[i..].find(char::is_whitespace).map_or(s.len(), |p| i + p);
            out.push_str(&s[i..end]);
            i = end;
            continue;
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn web_at(s: &str, i: usize) -> bool {
    s.is_char_boundary(i) && (s[i..].starts_with("http://") || s[i..].starts_with("https://") || s[i..].starts_with("asset://") || s[i..].starts_with("tauri://") || s[i..].starts_with("ipc://"))
}

/// What a path is replaced with: `~/` kept when it was there, then the file's kind.
fn replacement(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let lead = if path.starts_with('~') { "~/…/" } else { "" };
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && (1..=8).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) => {
            if CODE.contains(&ext.to_ascii_lowercase().as_str()) {
                format!("{lead}{name}")
            } else {
                format!("{lead}<file>.{}", ext.to_ascii_lowercase())
            }
        }
        _ if path.starts_with("content://") => "<content uri>".into(),
        _ => format!("{lead}<folder>"),
    }
}

/// Names with a document's extension and no folder ("Couldn't save Report.pdf"), and anything quoted that ends in
/// one ("“My Report.pdf”").
fn scrub_names(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    'outer: while !rest.is_empty() {
        // A quoted name, spaces and all.
        for (open, close) in [('"', '"'), ('\'', '\''), ('“', '”'), ('‘', '’'), ('«', '»')] {
            if rest.starts_with(open) {
                if let Some(len) = rest[open.len_utf8()..].find(close) {
                    let inner = &rest[open.len_utf8()..open.len_utf8() + len];
                    if let Some(ext) = document_ext(inner) {
                        out.push(open);
                        out.push_str(&format!("<file>.{ext}"));
                        out.push(close);
                        rest = &rest[open.len_utf8() + len + close.len_utf8()..];
                        continue 'outer;
                    }
                }
            }
        }
        // A word.
        let word_len = rest.find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '“' | '”' | '‘' | '’' | '(' | ')' | ',' | ';' | '«' | '»')).unwrap_or(rest.len());
        if word_len == 0 {
            let ch = rest.chars().next().unwrap();
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
            continue;
        }
        let word = &rest[..word_len];
        let trimmed = word.trim_end_matches(['.', ':', '!', '?']);
        match document_ext(trimmed) {
            Some(ext) if !trimmed.starts_with('<') => {
                out.push_str(&format!("<file>.{ext}"));
                out.push_str(&word[trimmed.len()..]);
            }
            _ => out.push_str(word),
        }
        rest = &rest[word_len..];
    }
    out
}

/// The extension of a document's name (lowercase), if `name` is one.
fn document_ext(name: &str) -> Option<String> {
    let (stem, ext) = name.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    (!stem.is_empty() && !stem.ends_with('>') && DOCUMENTS.contains(&ext.as_str())).then_some(ext)
}

// ---------------------------------------------------------------- time

/// `2026-11-22T10:15:03Z`.
fn timestamp(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil(days as i64);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Days since 1970-01-01 to a date (Howard Hinnant's algorithm).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(s: &str) -> String {
        scrub(s, None)
    }

    #[test]
    fn windows_paths() {
        assert_eq!(clean(r"Couldn't open C:\Users\asha\Documents\Tax 2025\Return form.pdf: damaged"), "Couldn't open <file>.pdf: damaged");
        assert_eq!(clean(r"saving D:/Work/plan.DOCX failed"), "saving <file>.docx failed");
        assert_eq!(clean(r"no access to \\server\share\x.xlsx"), "no access to <file>.xlsx");
        assert_eq!(clean(r"folder C:\Users\asha\Secret Stuff missing"), "folder <folder> Stuff missing");
    }

    #[test]
    fn unix_and_android_paths() {
        assert_eq!(clean("cannot read /home/ravi/Books/My Novel.epub"), "cannot read <file>.epub");
        assert_eq!(clean("open '/storage/emulated/0/Download/bank statement.pdf' failed"), "open '<file>.pdf' failed");
        assert_eq!(clean("from content://com.android.providers.downloads/document/raw%3A%2Fx.pdf"), "from <file>.pdf");
        assert_eq!(clean("from content://media/external/file/42"), "from <content uri>");
    }

    #[test]
    fn home_folder() {
        assert_eq!(scrub("in /home/ravi/.config/x", Some("/home/ravi")), "in ~/…/<folder>");
        if cfg!(windows) {
            assert_eq!(scrub(r"in c:\users\Asha\AppData\Local\paginaut\fonts", Some(r"C:\Users\Asha")), "in ~/…/<folder>");
        }
    }

    #[test]
    fn names_without_folders() {
        assert_eq!(clean("Couldn't save Report.pdf."), "Couldn't save <file>.pdf.");
        assert_eq!(clean("Opening “Bank Statement March.pdf” failed"), "Opening “<file>.pdf” failed");
        assert_eq!(clean("version 1.2.3 is out"), "version 1.2.3 is out");
    }

    #[test]
    fn code_and_web_stay() {
        assert_eq!(clean("index out of bounds at src/engine/mupdf.rs:120 (thread main)"), "index out of bounds at src/engine/mupdf.rs:120 (thread main)");
        assert_eq!(clean(r"at C:\Users\x\.cargo\registry\src\tauri-2.12.1\src\lib.rs:44"), "at lib.rs:44");
        assert_eq!(clean("TypeError: x is null at http://tauri.localhost/assets/index-abc.js:1:2345"), "TypeError: x is null at http://tauri.localhost/assets/index-abc.js:1:2345");
        assert_eq!(clean("ratio 1/2 and/or 3"), "ratio 1/2 and/or 3");
    }

    #[test]
    fn long_messages_are_cut() {
        let s = "é".repeat(3000);
        let c = clean(&s);
        assert!(c.len() <= MAX_MESSAGE + 10 && c.ends_with("… (cut)"));
    }

    #[test]
    fn rotation_keeps_two_files() {
        let dir = std::env::temp_dir().join(format!("paginaut-errlog-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let line = format!("{}\n", "x".repeat(99));
        for _ in 0..25 {
            append(&dir, &line, 1000);
        }
        let cur = fs::metadata(dir.join(FILE)).unwrap().len();
        let old = fs::metadata(dir.join(OLD)).unwrap().len();
        // Lines 1-10 filled the first file and 11-20 the second, which became the old one; 21-25 are in the new one.
        assert_eq!((cur, old), (500, 1000));
        // No third file.
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        // Read back oldest first.
        assert!(read_dir(&dir).len() as u64 == cur + old);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dates() {
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(1_795_342_503)), "2026-11-22T10:15:03Z");
        assert_eq!(timestamp(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29T00:00:00Z");
    }
}
