//! Read aloud with the voices installed in Windows (Settings › Time & language › Speech). WebView2's own
//! speechSynthesis has no voices, so the text is spoken here into a WAV, with the time each word starts,
//! and the page plays it and follows along.

use serde::Serialize;
use windows::core::{Interface, HSTRING};
use windows::Media::Core::SpeechCue;
use windows::Media::SpeechSynthesis::{SpeechSynthesizer, VoiceGender};
use windows::Storage::Streams::DataReader;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

#[derive(Serialize)]
pub struct Voice {
    pub id: String,
    pub name: String,
    /// BCP-47, e.g. "en-GB".
    pub lang: String,
    pub female: bool,
    pub default: bool,
}

/// A spoken word: characters [start, end) of the text (UTF-16, as in JavaScript), from `at` milliseconds.
#[derive(Serialize)]
pub struct Word {
    pub start: u32,
    pub end: u32,
    pub at: f64,
}

/// Runs `f` with COM started on this thread (WinRT needs it), and ended again after: the blocking pool's threads
/// are shared, and other work on them starts its own apartment. A thread already in one (either kind) is fine
/// for the speech classes, which are agile. Everything WinRT is released inside `f`.
fn with_com<R>(f: impl FnOnce() -> R) -> R {
    // SAFETY: balanced below; CoUninitialize only after a successful CoInitializeEx (S_OK or S_FALSE).
    let started = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let r = f();
    if started {
        unsafe { CoUninitialize() };
    }
    r
}

fn err(e: windows::core::Error) -> String {
    format!("Windows speech: {}", e.message())
}

pub fn voices() -> Result<Vec<Voice>, String> {
    with_com(voices_now)
}

fn voices_now() -> Result<Vec<Voice>, String> {
    let default = SpeechSynthesizer::DefaultVoice().and_then(|v| v.Id()).map(|s| s.to_string()).unwrap_or_default();
    let mut out = Vec::new();
    for v in SpeechSynthesizer::AllVoices().map_err(err)? {
        let id = v.Id().map_err(err)?.to_string();
        out.push(Voice {
            default: id == default,
            id,
            name: v.DisplayName().map_err(err)?.to_string(),
            lang: v.Language().map_err(err)?.to_string(),
            female: v.Gender().map_err(err)? == VoiceGender::Female,
        });
    }
    Ok(out)
}

/// Speaks `text` with the voice `voice` (its id; the Windows default if empty or gone) at `rate` (1 = normal,
/// 0.5–3). Returns the WAV and its words.
pub fn speak(text: &str, voice: &str, rate: f64) -> Result<(Vec<u8>, Vec<Word>), String> {
    with_com(|| speak_now(text, voice, rate))
}

fn speak_now(text: &str, voice: &str, rate: f64) -> Result<(Vec<u8>, Vec<Word>), String> {
    let synth = SpeechSynthesizer::new().map_err(err)?;
    if !voice.is_empty() {
        if let Some(v) = SpeechSynthesizer::AllVoices().map_err(err)?.into_iter().find(|v| v.Id().map(|i| i == voice).unwrap_or(false)) {
            synth.SetVoice(&v).map_err(err)?;
        }
    }
    let opts = synth.Options().map_err(err)?;
    opts.SetIncludeWordBoundaryMetadata(true).map_err(err)?;
    opts.SetSpeakingRate(rate.clamp(0.5, 3.0)).map_err(err)?;
    let stream = synth.SynthesizeTextToStreamAsync(&HSTRING::from(text)).map_err(err)?.join().map_err(err)?;

    let mut words = Vec::new();
    for track in stream.TimedMetadataTracks().map_err(err)? {
        for cue in track.Cues().map_err(err)? {
            let Ok(cue) = cue.cast::<SpeechCue>() else { continue };
            let (Ok(s), Ok(e)) = (cue.StartPositionInInput(), cue.EndPositionInInput()) else { continue };
            let (Ok(s), Ok(e)) = (s.Value(), e.Value()) else { continue };
            let at = cue.StartTime().map_err(err)?.Duration as f64 / 10_000.0;
            // End is inclusive.
            words.push(Word { start: s.max(0) as u32, end: (e + 1).max(s) as u32, at });
        }
    }
    words.sort_by(|a, b| a.at.total_cmp(&b.at));

    let size = stream.Size().map_err(err)? as u32;
    let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).map_err(err)?).map_err(err)?;
    reader.LoadAsync(size).map_err(err)?.join().map_err(err)?;
    let mut wav = vec![0u8; size as usize];
    reader.ReadBytes(&mut wav).map_err(err)?;
    let _ = stream.Close();
    let _ = synth.Close();
    Ok((wav, words))
}

#[cfg(test)]
mod tests {
    #[test]
    fn speaks_with_words() {
        let voices = super::voices().unwrap();
        println!("{} voices: {:?}", voices.len(), voices.iter().map(|v| &v.name).collect::<Vec<_>>());
        if voices.is_empty() {
            return; // A Windows without voices (some Server editions).
        }
        let text = "Hello there, this is Paginaut reading aloud.";
        let (wav, words) = super::speak(text, "", 1.0).unwrap();
        assert_eq!(&wav[..4], b"RIFF");
        let spoken: Vec<&str> = words.iter().map(|w| &text[w.start as usize..w.end as usize]).collect();
        println!("{spoken:?} at {:?}", words.iter().map(|w| w.at).collect::<Vec<_>>());
        assert!(spoken.contains(&"Paginaut"), "{spoken:?}");
        assert!(words.windows(2).all(|w| w[0].at <= w[1].at));
    }
}
