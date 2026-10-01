// Read aloud's controls: a small bar at the bottom of the document while it reads, and the voice and speed
// choices (also in the Reading pane).
import { useEffect } from "preact/hooks";
import { Pause, Play, SkipBack, SkipForward, Square } from "lucide-preact";
import { loadVoices, pauseOrResume, RATES, setSpeech, skipParagraph, speaking, speechPrefs, stopReading, voices } from "../speech";
import { activeTab } from "../store";

const voiceName = (n: string) => n.replace(/^Microsoft\s+/, "").replace(/\s+Desktop$/, "");

export function VoiceChoices() {
  useEffect(() => void loadVoices().catch(() => {}), []);
  const { voice, rate } = speechPrefs();
  const list = voices.value ?? [];
  const chosen = list.some((v) => v.id === voice) ? voice : "";
  return (
    <>
      <select class="inp sm" aria-label="Voice" title="Voice. Windows' voice switches to one for the text's language when there is one (download natural voices in Settings › Read aloud)" value={chosen} onChange={(e) => setSpeech({ voice: e.currentTarget.value })}>
        <option value="">{`Windows' voice${list.find((v) => v.default) ? ` (${voiceName(list.find((v) => v.default)!.name)})` : ""}`}</option>
        {list.map((v) => (
          <option key={v.id} value={v.id}>
            {voiceName(v.name)} · {v.lang}
          </option>
        ))}
      </select>
      <select class="inp sm" aria-label="Speed" title="Speed" value={String(rate)} onChange={(e) => setSpeech({ rate: Number(e.currentTarget.value) })}>
        {RATES.map((r) => (
          <option key={r} value={String(r)}>
            {r === 1 ? "Normal speed" : `${r}×`}
          </option>
        ))}
      </select>
    </>
  );
}

export function SpeechBar() {
  const s = speaking.value;
  if (!s) return null;
  const elsewhere = activeTab.value !== s.tab;
  return (
    <div class="speechbar" role="toolbar" aria-label="Read aloud">
      <button class="ib" aria-label="Previous paragraph" title="Previous paragraph" disabled={s.block < 0} onClick={() => skipParagraph(-1)}>
        <SkipBack size={16} strokeWidth={1.7} />
      </button>
      <button class="ib" aria-label={s.paused ? "Resume" : "Pause"} title={s.paused ? "Resume (Ctrl+Shift+Space)" : "Pause (Ctrl+Shift+Space)"} disabled={s.loading} onClick={pauseOrResume}>
        {s.loading ? <span class="spin sm" /> : s.paused ? <Play size={16} strokeWidth={1.7} /> : <Pause size={16} strokeWidth={1.7} />}
      </button>
      <button class="ib" aria-label="Next paragraph" title="Next paragraph" disabled={s.block < 0} onClick={() => skipParagraph(1)}>
        <SkipForward size={16} strokeWidth={1.7} />
      </button>
      <button class="ib" aria-label="Stop reading" title="Stop reading (Ctrl+Shift+U)" onClick={stopReading}>
        <Square size={14} strokeWidth={1.9} />
      </button>
      <div class="sep" />
      {elsewhere && <span class="sb-doc" title={s.tab.name}>{s.tab.name}</span>}
      <VoiceChoices />
    </div>
  );
}
