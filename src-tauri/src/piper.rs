//! Piper voices: natural-sounding offline voices downloaded on request (Settings › Voices). The Piper program
//! (piper.exe with ONNX Runtime and espeak-ng, MIT and GPL respectively, run as a separate program) comes from
//! its own release on GitHub and the voices from the rhasspy/piper-voices repository at one commit; each file is
//! checked against the size and hash in `piper_voices` before it's used. Everything lives in the local data
//! folder's `voices`: the program in `piper/`, each voice as `<id>.onnx` and `<id>.onnx.json`.
//!
//! Piper gives no word timings, so the text is spoken a sentence at a time (exact sentence times) and the words
//! of each sentence are spread over its length by their size, with extra room after commas and the like.

use crate::piper_voices::{Voice, COMMIT, VOICES};
use crate::tts::Word;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Piper's release for this system: host, path, size, SHA-256.
#[cfg(windows)]
const RUNTIME: (&str, &str, u64, &str) = (
    "github.com",
    "/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip",
    22_477_236,
    "f3c58906402b24f3a96d92145f58acba6d86c9b5db896d207f78dc80811efcea",
);
#[cfg(all(not(windows), target_arch = "x86_64"))]
const RUNTIME: (&str, &str, u64, &str) = (
    "github.com",
    "/rhasspy/piper/releases/download/2023.11.14-2/piper_linux_x86_64.tar.gz",
    26_460_462,
    "a50cb45f355b7af1f6d758c1b360717877ba0a398cc8cbe6d2a7a3a26e225992",
);
#[cfg(all(not(windows), target_arch = "aarch64"))]
const RUNTIME: (&str, &str, u64, &str) = (
    "github.com",
    "/rhasspy/piper/releases/download/2023.11.14-2/piper_linux_aarch64.tar.gz",
    26_004_717,
    "fea0fd2d87c54dbc7078d0f878289f404bd4d6eea6e7444a77835d1537ab88eb",
);
/// The program inside the release's `piper/` folder.
const PIPER: &str = if cfg!(windows) { "piper.exe" } else { "piper" };
const VOICE_HOST: &str = "huggingface.co";
/// Voice ids are given to the page with this in front, to tell them from Windows' own.
pub const PREFIX: &str = "piper:";
/// Silence between sentences, in seconds.
const GAP: f64 = 0.25;

#[derive(Clone)]
pub struct Voices {
    pub dir: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceInfo {
    id: String,
    name: String,
    lang: String,
    country: String,
    /// BCP-47, e.g. "mr-IN".
    tag: String,
    quality: String,
    /// Bytes to download: the voice, and Piper itself while no voice is installed.
    size: u64,
    installed: bool,
}

/// "en_US-northern_english_male-medium" → "Northern English Male"; other than medium quality is named.
fn speaker_name(v: &Voice) -> String {
    let speaker = v.id.split('-').nth(1).unwrap_or(v.id);
    let mut name: String = speaker
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    if v.quality != "medium" {
        name += &format!(" ({})", v.quality.replace('_', " "));
    }
    name
}

/// A voice's file name: Piper reads its arguments as ANSI text, so only ASCII (pt_PT-tugão-medium has an ã).
fn stem(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect()
}

fn find(id: &str) -> Result<&'static Voice, String> {
    VOICES.iter().find(|v| v.id == id).ok_or_else(|| format!("unknown voice {id}"))
}

impl Voices {
    fn runtime(&self) -> PathBuf {
        self.dir.join("piper")
    }
    fn exe(&self) -> PathBuf {
        self.runtime().join(PIPER)
    }
    fn model(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{}.onnx", stem(id)))
    }
    fn config(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{}.onnx.json", stem(id)))
    }
    fn has(&self, id: &str) -> bool {
        self.model(id).is_file() && self.config(id).is_file()
    }

    /// Every voice on offer, with which are installed.
    pub fn list(&self) -> Vec<VoiceInfo> {
        let runtime = if self.exe().is_file() { 0 } else { RUNTIME.2 };
        VOICES
            .iter()
            .map(|v| VoiceInfo {
                id: v.id.into(),
                name: speaker_name(v),
                lang: v.lang.into(),
                country: v.country.into(),
                tag: v.tag.into(),
                quality: v.quality.into(),
                size: v.model.0 + v.config.0 + runtime,
                installed: self.has(v.id) && self.exe().is_file(),
            })
            .collect()
    }

    /// The installed voices, as read aloud lists them.
    pub fn installed(&self) -> Vec<crate::tts::Voice> {
        if !self.exe().is_file() {
            return Vec::new();
        }
        VOICES
            .iter()
            .filter(|v| self.has(v.id))
            .map(|v| crate::tts::Voice { id: format!("{PREFIX}{}", v.id), name: format!("{} (Piper)", speaker_name(v)), lang: v.tag.into(), female: false, default: false })
            .collect()
    }

    /// Downloads a voice (and Piper itself, the first time), each file checked before it's kept.
    pub fn download(&self, id: &str, cancel: &AtomicBool, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
        let v = find(id)?;
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("Couldn't create {}: {e}", self.dir.display()))?;
        let need_runtime = !self.exe().is_file();
        let total = v.model.0 + v.config.0 + if need_runtime { RUNTIME.2 } else { 0 };
        let done = AtomicU64::new(0);
        let mut step = |size: u64, f: &mut dyn FnMut(&mut dyn FnMut(u64, u64)) -> Result<(), String>| {
            let base = done.load(Ordering::Relaxed);
            let r = f(&mut |d, _| progress(base + d, total));
            done.store(base + size, Ordering::Relaxed);
            r
        };
        if need_runtime {
            step(RUNTIME.2, &mut |p| self.get_runtime(cancel, p))?;
        }
        let path = format!("/rhasspy/piper-voices/resolve/{COMMIT}/{}", v.path);
        let config = self.config(id).with_extension("json.part");
        step(v.config.0, &mut |p| {
            let mut data = Vec::new();
            let get = crate::net::Get { host: VOICE_HOST, path: &format!("{path}.onnx.json"), cancel, progress: p, expect: Some(v.config.0), max: v.config.0, from: 0, min_rate: 0 };
            crate::net::fetch(get, &mut |c| {
                data.extend_from_slice(c);
                Ok(())
            })?;
            if crate::net::git_blob_sha1(&data)? != v.config.1 {
                return Err("The downloaded voice didn't match the expected one; try again.".into());
            }
            std::fs::write(&config, usable_config(&data)?).map_err(|e| e.to_string())
        })?;
        let model = self.model(id).with_extension("onnx.part");
        let r = step(v.model.0, &mut |p| {
            let sha = crate::net::to_file_resuming(VOICE_HOST, &format!("{path}.onnx"), &model, cancel, p, v.model.0, crate::net::Alg::Sha256)?;
            if crate::net::hex(&sha) != v.model.1 {
                return Err("The downloaded voice didn't match the expected one; try again.".into());
            }
            Ok(())
        });
        let r = r.and_then(|_| {
            std::fs::rename(&model, self.model(id)).map_err(|e| e.to_string())?;
            std::fs::rename(&config, self.config(id)).map_err(|e| e.to_string())
        });
        if r.is_err() {
            let _ = std::fs::remove_file(&model);
            let _ = std::fs::remove_file(&config);
        }
        r
    }

    /// Piper itself: the release's zip, checked, its `piper/` folder unpacked into a fresh folder put in place last.
    fn get_runtime(&self, cancel: &AtomicBool, progress: &mut dyn FnMut(u64, u64)) -> Result<(), String> {
        let (host, path, size, sha) = RUNTIME;
        // One download of Piper at a time: two voices started together on a fresh install would both write
        // piper.zip.part and unpack into piper.part. The second waits (it can be stopped), then finds Piper there.
        static ONE: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
        let _one = loop {
            if let Some(g) = ONE.try_lock_for(std::time::Duration::from_millis(200)) {
                break g;
            }
            if cancel.load(std::sync::atomic::Ordering::Acquire) {
                return Err("stopped".into());
            }
        };
        if self.runtime().join(PIPER).is_file() {
            progress(size, size);
            return Ok(());
        }
        let zip = self.dir.join("piper.zip.part");
        let fresh = self.dir.join("piper.part");
        let r = (|| {
            let got = crate::net::to_file_resuming(host, path, &zip, cancel, progress, size, crate::net::Alg::Sha256)?;
            if crate::net::hex(&got) != sha {
                return Err("The downloaded Piper didn't match the expected one; try again.".into());
            }
            let _ = std::fs::remove_dir_all(&fresh);
            #[cfg(not(windows))]
            unpack_tar(&zip, &fresh)?;
            #[cfg(windows)]
            let zip_path = zip.to_string_lossy();
            #[cfg(windows)]
            for name in crate::engine::mupdf::zip_list(&zip_path)? {
                let Some(rel) = name.strip_prefix("piper/") else { continue };
                // Only plain relative names.
                if rel.is_empty() || name.ends_with('/') || rel.split('/').any(|p| p.is_empty() || p == "." || p == "..") || rel.contains(['\\', ':']) {
                    continue;
                }
                let Some(data) = crate::engine::mupdf::zip_read(&zip_path, &name)? else { continue };
                let dest = fresh.join(rel);
                std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
                std::fs::write(&dest, data).map_err(|e| e.to_string())?;
            }
            if !fresh.join(PIPER).is_file() {
                return Err(format!("The Piper download had no {PIPER}."));
            }
            let _ = std::fs::remove_dir_all(self.runtime());
            std::fs::rename(&fresh, self.runtime()).map_err(|e| e.to_string())
        })();
        let _ = std::fs::remove_file(&zip);
        if r.is_err() {
            let _ = std::fs::remove_dir_all(&fresh);
        }
        r
    }

    /// A short recording of a voice (MP3) from the voice repository, to hear it before downloading.
    pub fn sample(&self, id: &str) -> Result<Vec<u8>, String> {
        let v = find(id)?;
        let dir = v.path.rsplit_once('/').map_or("", |d| d.0);
        let path = format!("/rhasspy/piper-voices/resolve/{COMMIT}/{dir}/samples/speaker_0.mp3");
        let stop = AtomicBool::new(false);
        let mut data = Vec::new();
        let get = crate::net::Get { host: VOICE_HOST, path: &path, cancel: &stop, progress: &mut |_, _| {}, expect: None, max: 4 << 20, from: 0, min_rate: 0 };
        crate::net::fetch(get, &mut |c| {
            data.extend_from_slice(c);
            Ok(())
        })?;
        Ok(data)
    }

    /// Removes a voice; Piper itself goes with the last one.
    pub fn delete(&self, id: &str) -> Result<(), String> {
        find(id)?;
        for p in [self.model(id), self.config(id)] {
            if p.is_file() {
                std::fs::remove_file(&p).map_err(|e| e.to_string())?;
            }
        }
        if !VOICES.iter().any(|v| self.has(v.id)) {
            let _ = std::fs::remove_dir_all(self.runtime());
        }
        Ok(())
    }

    /// Speaks `text` with voice `id` at `rate` (1 = normal). Returns a WAV and its words.
    pub fn speak(&self, id: &str, text: &str, rate: f64) -> Result<(Vec<u8>, Vec<Word>), String> {
        find(id)?;
        if !self.has(id) || !self.exe().is_file() {
            return Err("That Piper voice isn't installed any more.".into());
        }
        let config: serde_json::Value = serde_json::from_slice(&std::fs::read(self.config(id)).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let rate_hz = config["audio"]["sample_rate"].as_u64().unwrap_or(22050) as u32;
        let length = config["inference"]["length_scale"].as_f64().unwrap_or(1.0) / rate.clamp(0.5, 3.0);

        let sentences = sentences(text);
        static RUN: AtomicU64 = AtomicU64::new(0);
        // Inside the voices folder, named by relative paths: Piper reads paths as ANSI text, and the user's
        // folder may have other letters in it.
        let tmp = format!("speaking-{}-{}", std::process::id(), RUN.fetch_add(1, Ordering::Relaxed));
        std::fs::create_dir_all(self.dir.join(&tmp)).map_err(|e| e.to_string())?;
        let r = self.run(id, length, &sentences.iter().map(|s| &text[s.0..s.1]).collect::<Vec<_>>(), &tmp).and_then(|clips| {
            let gap = vec![0i16; (GAP * rate_hz as f64) as usize];
            let mut pcm: Vec<i16> = Vec::new();
            let mut words = Vec::new();
            for (i, (clip, &(from, to))) in clips.iter().zip(&sentences).enumerate() {
                if i > 0 {
                    pcm.extend_from_slice(&gap);
                }
                let at = pcm.len() as f64 * 1000.0 / rate_hz as f64;
                let ms = clip.len() as f64 * 1000.0 / rate_hz as f64;
                spread(text, from, to, at, ms, &mut words);
                pcm.extend_from_slice(clip);
            }
            Ok((wav(&pcm, rate_hz), words))
        });
        let _ = std::fs::remove_dir_all(self.dir.join(&tmp));
        r
    }

    /// Runs Piper on the sentences, one WAV each; returns their samples.
    fn run(&self, id: &str, length: f64, sentences: &[&str], tmp: &str) -> Result<Vec<Vec<i16>>, String> {
        use std::io::Write;
        let mut cmd = std::process::Command::new(self.exe());
        cmd.arg("--model").arg(format!("{}.onnx", stem(id))).arg("--json-input").arg("--length_scale").arg(format!("{length:.3}")).arg("--quiet");
        cmd.current_dir(&self.dir).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
        // Linux: its own libraries (ONNX Runtime, espeak-ng) sit next to it.
        #[cfg(not(windows))]
        cmd.env("LD_LIBRARY_PATH", self.runtime()).arg("--espeak_data").arg(self.runtime().join("espeak-ng-data"));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn().map_err(|e| format!("Couldn't start Piper: {e}"))?;
        let files: Vec<String> = (0..sentences.len()).map(|i| format!("{tmp}/{i}.wav")).collect();
        {
            let mut input = child.stdin.take().unwrap();
            let mut lines = String::new();
            for (s, f) in sentences.iter().zip(&files) {
                // Line breaks inside a sentence would end it early.
                let line = serde_json::json!({ "text": s.split_whitespace().collect::<Vec<_>>().join(" "), "output_file": f });
                lines += &line.to_string();
                lines.push('\n');
            }
            input.write_all(lines.as_bytes()).map_err(|e| format!("Piper: {e}"))?;
        }
        let out = child.wait_with_output().map_err(|e| format!("Piper: {e}"))?;
        let mut clips = Vec::new();
        for f in &files {
            match std::fs::read(self.dir.join(f)).ok().and_then(|d| samples(&d)) {
                Some(s) => clips.push(s),
                None => {
                    let err = String::from_utf8_lossy(&out.stderr);
                    let line = err.lines().rev().find(|l| l.contains("error")).unwrap_or("it stopped early");
                    return Err(format!("Piper couldn't speak this: {}", line.split("] ").last().unwrap_or(line)));
                }
            }
        }
        Ok(clips)
    }
}

/// The config Piper 2023 can read: later voices name a few vowel pairs as single sounds, which it refuses
/// ("not a single codepoint"); without them it speaks each half, as espeak-ng writes them.
fn usable_config(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut c: serde_json::Value = serde_json::from_slice(data).map_err(|e| format!("voice config: {e}"))?;
    if let Some(map) = c.get_mut("phoneme_id_map").and_then(|m| m.as_object_mut()) {
        map.retain(|k, _| k.chars().count() == 1);
    }
    serde_json::to_vec(&c).map_err(|e| e.to_string())
}


/// Byte ranges of the sentences (ending at . ! ? … । ॥ and the like, plus closing quotes), blanks left out.
fn sentences(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    let mut end_seen = false;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if end_seen {
                if let Some(s) = start.take() {
                    out.push((s, i));
                }
                end_seen = false;
            }
            continue;
        }
        if start.is_none() {
            start = Some(i);
        }
        if matches!(c, '.' | '!' | '?' | '…' | '।' | '॥' | '。' | '！' | '？' | ';') {
            end_seen = true;
        } else if !matches!(c, '"' | '\'' | '”' | '’' | ')' | ']') {
            end_seen = false;
        }
    }
    if let Some(s) = start {
        out.push((s, text.len()));
    }
    // Something to say in each.
    out.retain(|&(a, b)| text[a..b].chars().any(char::is_alphanumeric));
    out
}

/// Spreads the words of text[from..to) over `ms` milliseconds from `at`, by their letters, with room for the pause
/// after punctuation. Offsets are in UTF-16 units, as the page counts them.
fn spread(text: &str, from: usize, to: usize, at: f64, ms: f64, out: &mut Vec<Word>) {
    let mut words: Vec<(usize, usize, f64)> = Vec::new();
    let mut start = None;
    for (i, c) in text[from..to].char_indices().map(|(i, c)| (i + from, c)).chain(std::iter::once((to, ' '))) {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                let w = &text[s..i];
                let letters = w.chars().filter(|c| c.is_alphanumeric()).count() as f64;
                let pause = match w.chars().last() {
                    Some(',' | '،' | '、') => 3.0,
                    Some(';' | ':' | '—' | '–') => 4.0,
                    _ => 0.0,
                };
                words.push((s, i, letters.max(1.0) + 1.0 + pause));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    let total: f64 = words.iter().map(|w| w.2).sum();
    if total <= 0.0 {
        return;
    }
    // Piper leaves a little silence at each end.
    let lead = (ms * 0.04).min(120.0);
    let span = ms - 2.0 * lead;
    let utf16 = |b: usize| text[..b].encode_utf16().count() as u32;
    let mut t = at + lead;
    for (s, e, w) in words {
        out.push(Word { start: utf16(s), end: utf16(e), at: t });
        t += span * w / total;
    }
}

/// The 16-bit samples of a WAV Piper wrote.
fn samples(wav: &[u8]) -> Option<Vec<i16>> {
    if wav.len() < 12 || &wav[..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let mut i = 12;
    while i + 8 <= wav.len() {
        let len = u32::from_le_bytes(wav[i + 4..i + 8].try_into().ok()?) as usize;
        let body = &wav[i + 8..(i + 8 + len).min(wav.len())];
        if &wav[i..i + 4] == b"data" {
            return Some(body.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect());
        }
        i += 8 + len + (len & 1);
    }
    None
}

fn wav(pcm: &[i16], rate: u32) -> Vec<u8> {
    let data = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Downloads in progress, so they can be stopped: voice id → stop flag.
pub static DOWNLOADS: once_cell::sync::Lazy<parking_lot::Mutex<std::collections::HashMap<String, std::sync::Arc<AtomicBool>>>> =
    once_cell::sync::Lazy::new(Default::default);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_sentences() {
        let t = "Hello there. “Quoted!” Next one… पान क्रमांक दोन। शेवट";
        let s: Vec<&str> = sentences(t).iter().map(|&(a, b)| &t[a..b]).collect();
        assert_eq!(s, ["Hello there.", "“Quoted!”", "Next one…", "पान क्रमांक दोन।", "शेवट"]);
        assert_eq!(sentences("  ...  "), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn spreads_words_in_order() {
        let t = "Hi, पान क्रमांक";
        let mut w = Vec::new();
        spread(t, 0, t.len(), 100.0, 1000.0, &mut w);
        let utf16: Vec<u16> = t.encode_utf16().collect();
        let got: Vec<String> = w.iter().map(|w| String::from_utf16_lossy(&utf16[w.start as usize..w.end as usize])).collect();
        assert_eq!(got, ["Hi,", "पान", "क्रमांक"]);
        assert!(w.windows(2).all(|p| p[0].at < p[1].at) && w[0].at >= 100.0 && w[2].at < 1100.0);
    }

    #[test]
    fn drops_vowel_pairs() {
        let c = usable_config(r#"{"phoneme_id_map":{"a":[1],"aɪ":[161]}}"#.as_bytes()).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&c).unwrap();
        assert_eq!(v["phoneme_id_map"].as_object().unwrap().len(), 1);
    }

    #[test]
    fn names_speakers() {
        assert_eq!(speaker_name(find("en_GB-northern_english_male-medium").unwrap()), "Northern English Male");
        assert!(VOICES.iter().any(|v| v.id == "mr_IN-google-medium"));
    }

    /// Network test (about 85 MB): cargo test --lib piper::tests -- --ignored --nocapture
    #[test]
    #[ignore]
    fn downloads_and_speaks() {
        let _engine = crate::engine::Engine::new(1, 16 << 20).unwrap(); // unpacking uses MuPDF
        let dir = std::env::temp_dir().join("paginaut-piper-test");
        let voices = Voices { dir: dir.clone() };
        let stop = AtomicBool::new(false);
        let mut last = (0, 0);
        voices.download("mr_IN-google-medium", &stop, &mut |d, t| last = (d, t)).unwrap();
        assert_eq!(last.0, last.1);
        assert_eq!(voices.installed().len(), 1);
        let text = "पान क्रमांक दोन. महाराष्ट्र टाइम्स मधील ही बातमी आहे.";
        let t0 = std::time::Instant::now();
        let (wav, words) = voices.speak("mr_IN-google-medium", text, 1.0).unwrap();
        println!("{} bytes, {} words in {:?}", wav.len(), words.len(), t0.elapsed());
        assert!(wav.len() > 44 + 22050 && words.len() == 9);
        voices.delete("mr_IN-google-medium").unwrap();
        assert!(!dir.join("piper").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Linux: the release's `piper/` folder unpacked into `fresh` with the system's tar (links and the
/// program's executable bit kept), refusing names outside it.
#[cfg(not(windows))]
fn unpack_tar(archive: &std::path::Path, fresh: &std::path::Path) -> Result<(), String> {
    let list = std::process::Command::new("tar").arg("-tzf").arg(archive).output().map_err(|e| format!("Couldn't unpack Piper: {e}"))?;
    if String::from_utf8_lossy(&list.stdout).lines().any(|n| n.starts_with('/') || n.split('/').any(|p| p == "..")) {
        return Err("The Piper download had unexpected files.".into());
    }
    let parent = fresh.with_extension("unpack");
    let _ = std::fs::remove_dir_all(&parent);
    std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let ok = std::process::Command::new("tar").arg("-xzf").arg(archive).arg("-C").arg(&parent).status().map_err(|e| format!("Couldn't unpack Piper: {e}"))?;
    let r = if ok.success() { std::fs::rename(parent.join("piper"), fresh).map_err(|e| e.to_string()) } else { Err("Couldn't unpack Piper.".into()) };
    let _ = std::fs::remove_dir_all(&parent);
    r
}
