// Read aloud: the document's reflowed text (as Read mode has it: no running headers, footers or page numbers)
// spoken with a voice installed in Windows or a downloaded Piper voice, a paragraph at a time. Read and Book modes mark the word being
// spoken and keep it in view; the page view follows the pages.
import { effect, signal } from "@preact/signals";
import { api } from "./api";
import { textOf, type Flow } from "./reflow/model";
import { paintRange, rangeOf } from "./reflow/find";
import { overridesOf } from "./reflow/overrides";
import { activeTab, settings, showToast, tabs, updateSettings, type Tab } from "./store";
import { analysisOf, flowOf } from "./ui/Reflow";

export interface Voice {
  id: string;
  name: string;
  lang: string;
  female: boolean;
  default: boolean;
}

export interface Speaking {
  tab: Tab;
  /** Block being read. */
  block: number;
  paused: boolean;
  /** Waiting for the text or the voice. */
  loading: boolean;
}

export const speaking = signal<Speaking | null>(null);
export const voices = signal<Voice[] | null>(null);

export const RATES = [0.75, 1, 1.25, 1.5, 1.75, 2];
export const speechPrefs = () => ({ voice: "", rate: 1, ...settings.value.speech });

/** A Piper voice on offer (Settings › Read aloud). */
export interface PiperVoice {
  id: string;
  name: string;
  lang: string;
  country: string;
  tag: string;
  quality: string;
  /** Bytes to download (with Piper itself while no voice is installed). */
  size: number;
  installed: boolean;
}

/** Piper voices are told apart from Windows' by this in front of their id. */
export const PIPER = "piper:";
export const piperVoices = signal<PiperVoice[] | null>(null);
/** Voice downloads under way: id → share done. */
export const voiceDownloads = signal<Record<string, number>>({});

export async function loadPiperVoices() {
  try {
    piperVoices.value = await api.piperVoices();
  } catch (e) {
    showToast(`Couldn't list the voices: ${e}`, 5000);
  }
}

/** Reads both lists again (a voice came or went). */
function refreshVoices() {
  voicesAsked = null;
  void loadVoices().catch(() => {});
  void loadPiperVoices();
}

export async function downloadVoice(id: string) {
  if (id in voiceDownloads.value) return;
  voiceDownloads.value = { ...voiceDownloads.value, [id]: 0 };
  try {
    await api.piperDownload(id, (ev) => (voiceDownloads.value = { ...voiceDownloads.value, [id]: ev.total ? ev.done / ev.total : 0 }));
  } catch (e) {
    if (String(e) !== "stopped") showToast(`Couldn't download the voice: ${e}`, 5000);
  } finally {
    const { [id]: _, ...rest } = voiceDownloads.value;
    voiceDownloads.value = rest;
    refreshVoices();
  }
}

export const stopVoiceDownload = (id: string) => void api.piperDownloadStop(id);

export async function removeVoice(id: string) {
  // Reading with it carries on with Windows' voice (its files go).
  if (speechPrefs().voice === PIPER + id) setSpeech({ voice: "" });
  try {
    await api.piperDelete(id);
  } catch (e) {
    showToast(`Couldn't remove the voice: ${e}`);
  }
  refreshVoices();
}

let sample: { id: string; audio: HTMLAudioElement } | null = null;
export const samplePlaying = signal<string | null>(null);
/** Plays (or stops) a voice's sample recording, fetched from the voice repository. */
export async function playSample(id: string) {
  const was = sample?.id;
  if (sample) {
    sample.audio.pause();
    URL.revokeObjectURL(sample.audio.src);
    sample = null;
    samplePlaying.value = null;
  }
  if (was === id) return;
  samplePlaying.value = id;
  try {
    const buf = await api.piperSample(id);
    if (samplePlaying.value !== id) return;
    const audio = new Audio(URL.createObjectURL(new Blob([buf], { type: "audio/mpeg" })));
    sample = { id, audio };
    audio.addEventListener("ended", () => {
      if (sample?.audio === audio) void playSample(id);
    });
    await audio.play();
  } catch (e) {
    if (samplePlaying.value === id) samplePlaying.value = null;
    showToast(`Couldn't play the sample: ${e}`, 5000);
  }
}

let voicesAsked: Promise<Voice[]> | null = null;
export function loadVoices(): Promise<Voice[]> {
  voicesAsked ??= api.ttsVoices().then(
    (v) => (voices.value = v),
    (e) => {
      voicesAsked = null;
      throw e;
    },
  );
  return voicesAsked;
}

// Scripts a paragraph can be told apart by, and the languages whose voices read them.
const SCRIPTS: [RegExp, string[]][] = [
  [/\p{Script=Devanagari}/gu, ["hi", "mr"]],
  [/\p{Script=Arabic}/gu, ["ar", "ur", "fa"]],
  [/\p{Script=Hebrew}/gu, ["he"]],
  [/\p{Script=Cyrillic}/gu, ["ru", "uk", "bg"]],
  [/\p{Script=Greek}/gu, ["el"]],
  [/\p{Script=Thai}/gu, ["th"]],
  [/[\p{Script=Hiragana}\p{Script=Katakana}]/gu, ["ja"]],
  [/\p{Script=Hangul}/gu, ["ko"]],
  [/\p{Script=Han}/gu, ["zh", "ja"]],
  [/\p{Script=Tamil}/gu, ["ta"]],
  [/\p{Script=Telugu}/gu, ["te"]],
];

/** With Windows' voice chosen: a voice for the paragraph's script when it isn't Latin and one is installed. */
function voiceFor(text: string): string {
  const list = voices.value;
  if (!list?.length) return "";
  const letters = text.match(/\p{L}/gu)?.length ?? 0;
  for (const [re, langs] of SCRIPTS) {
    if ((text.match(re)?.length ?? 0) * 2 < letters) continue;
    const fits = (v: Voice) => langs.includes(v.lang.split("-")[0]);
    // A downloaded Piper voice first: it was got for this.
    const piper = list.find((v) => v.id.startsWith(PIPER) && fits(v));
    if (piper) return piper.id;
    const def = list.find((v) => v.default);
    if (def && fits(def)) return "";
    return list.find(fits)?.id ?? "";
  }
  return "";
}

// ---------------------------------------------------------------- the text in pieces

interface Piece {
  b: number;
  /** Characters [from, to) of the block's text. */
  from: number;
  to: number;
}

// Short pieces start sooner; the next one is made while this one plays.
const PIECE = 700;

function piecesOf(flow: Flow): Piece[] {
  const out: Piece[] = [];
  flow.blocks.forEach((blk, b) => {
    if (blk.k === "img") return;
    const text = textOf(blk);
    if (!/[\p{L}\p{N}]/u.test(text)) return;
    let from = 0;
    while (text.length - from > PIECE) {
      // Break after the last sentence end that fits, else the last space.
      const part = text.slice(from, from + PIECE);
      let cut = -1;
      for (const m of part.matchAll(/[.!?…;:]["'”’)\]]*\s+/g)) if (m.index! > PIECE / 4) cut = m.index! + m[0].length;
      if (cut < 0) cut = part.lastIndexOf(" ") + 1 || PIECE;
      out.push({ b, from, to: from + cut });
      from += cut;
    }
    out.push({ b, from, to: text.length });
  });
  return out;
}

// ---------------------------------------------------------------- playing

interface Word {
  start: number;
  end: number;
  at: number;
}
interface Clip {
  url: string;
  words: Word[];
}

class Reader {
  private audio = new Audio();
  private clips = new Map<number, Promise<Clip>>();
  private i = 0;
  private word = -1;
  private raf = 0;
  private gen = 0;
  private marked: HTMLElement[] = [];

  /** What the text was made from: a change to any of them stops reading. */
  readonly made: unknown[];

  constructor(readonly tab: Tab, private flow: Flow, private pieces: Piece[]) {
    this.made = madeFrom(tab);
    this.audio.addEventListener("ended", () => this.play(this.i + 1));
    this.audio.addEventListener("error", () => this.fail("The voice's audio couldn't be played."));
  }

  private text(p: Piece) {
    return textOf(this.flow.blocks[p.b]).slice(p.from, p.to);
  }

  private clip(i: number): Promise<Clip> {
    let c = this.clips.get(i);
    if (!c) {
      const { voice, rate } = speechPrefs();
      const text = this.text(this.pieces[i]);
      // A voice chosen before and since removed from Windows: as if none was chosen.
      const chosen = voice && voices.value?.some((v) => v.id === voice) ? voice : "";
      c = api.ttsSpeak(text, chosen || voiceFor(text), rate).then((buf) => {
        const n = new DataView(buf).getUint32(0, true);
        const words = JSON.parse(new TextDecoder().decode(new Uint8Array(buf, 4, n))) as Word[];
        return { url: URL.createObjectURL(new Blob([new Uint8Array(buf, 4 + n)], { type: "audio/wav" })), words };
      });
      this.clips.set(i, c);
    }
    return c;
  }

  /** Forgets made clips outside [keepFrom, keepTo]. */
  private drop(keepFrom = Infinity, keepTo = -1) {
    for (const [k, c] of this.clips) {
      if (k >= keepFrom && k <= keepTo) continue;
      this.clips.delete(k);
      c.then((x) => URL.revokeObjectURL(x.url), () => {});
    }
  }

  /** Index of the first piece at or after character `off` of block `b`. */
  indexOf(b: number, off = 0) {
    const i = this.pieces.findIndex((p) => p.b > b || (p.b === b && p.to > off));
    return i < 0 ? this.pieces.length : i;
  }

  async play(i: number) {
    const gen = ++this.gen;
    cancelAnimationFrame(this.raf);
    this.audio.pause();
    if (i >= this.pieces.length) return stopReading();
    this.i = i;
    this.word = -1;
    this.drop(i, i + 1);
    const p = this.pieces[i];
    speaking.value = { tab: this.tab, block: p.b, paused: false, loading: true };
    this.mark(p.b, p.from, p.from);
    let clip: Clip;
    try {
      clip = await this.clip(i);
    } catch (e) {
      if (gen === this.gen) this.fail(String((e as Error)?.message ?? e));
      return;
    }
    if (gen !== this.gen) return;
    if (i + 1 < this.pieces.length) this.clip(i + 1).catch(() => {});
    this.audio.src = clip.url;
    try {
      await this.audio.play();
    } catch {
      if (gen === this.gen) this.fail("The voice's audio couldn't be played.");
      return;
    }
    if (gen !== this.gen) return;
    speaking.value = { tab: this.tab, block: p.b, paused: false, loading: false };
    this.follow(clip);
  }

  /** Marks each word as it's spoken. */
  private follow(clip: Clip) {
    const p = this.pieces[this.i];
    cancelAnimationFrame(this.raf);
    const step = () => {
      const t = this.audio.currentTime * 1000;
      let w = this.word;
      while (w + 1 < clip.words.length && clip.words[w + 1].at <= t) w++;
      if (w !== this.word && w >= 0) {
        this.word = w;
        this.mark(p.b, p.from + clip.words[w].start, p.from + clip.words[w].end);
      }
      this.raf = requestAnimationFrame(step);
    };
    step();
  }

  /** Marks the word (and its paragraph) and keeps it in view, unless you've just moved the view yourself. */
  private mark(b: number, from: number, to: number) {
    const tab = this.tab;
    const view = tab.reflowView;
    const idle = performance.now() - lastUserMove > 4000;
    if (view) {
      if (idle) view.reveal(b, from);
      for (const el of this.marked) el.classList.remove("rf-reading");
      this.marked = [...view.el.querySelectorAll<HTMLElement>(`[data-b="${b}"]`)];
      for (const el of this.marked) el.classList.add("rf-reading");
      paintRange("rf-speak", to > from ? rangeOf(view.el, b, from, to) : null);
    } else if (idle && tab.viewer && activeTab.value === tab) {
      const page = this.flow.blocks[b].page;
      if (page !== tab.page.value) tab.viewer.goToPage(page);
    }
  }

  private unmark() {
    for (const el of this.marked) el.classList.remove("rf-reading");
    this.marked = [];
    paintRange("rf-speak", null);
  }

  private fail(msg: string) {
    stopReading();
    showToast(`Couldn't read aloud: ${msg}`, 5000);
  }

  pause() {
    if (!speaking.value || speaking.value.loading) return;
    this.audio.pause();
    cancelAnimationFrame(this.raf);
    speaking.value = { ...speaking.value, paused: true };
  }

  resume() {
    if (!speaking.value?.paused) return;
    if (!this.audio.src) return void this.play(this.i);
    speaking.value = { ...speaking.value, paused: false };
    const gen = this.gen;
    void Promise.all([this.clips.get(this.i), this.audio.play()]).then(
      ([clip]) => clip && gen === this.gen && this.follow(clip),
      () => gen === this.gen && this.fail("The voice's audio couldn't be played."),
    );
  }

  /** Next or previous paragraph. */
  skip(dir: 1 | -1) {
    const b = this.pieces[this.i].b;
    let j = this.i;
    if (dir > 0) while (j < this.pieces.length && this.pieces[j].b === b) j++;
    else {
      // Back to this paragraph's start, or if already there (within a second), the one before.
      const atStart = this.pieces[j].from === 0 && this.audio.currentTime < 1;
      while (j > 0 && this.pieces[j - 1].b === b) j--;
      if (atStart && j > 0) {
        const prev = this.pieces[j - 1].b;
        j--;
        while (j > 0 && this.pieces[j - 1].b === prev) j--;
      }
    }
    void this.play(j);
  }

  /** The voice or speed changed: carry on from the word being read (still paused, if it was). */
  restart() {
    const c = this.pieces[this.i];
    const clip = this.clips.get(this.i);
    this.drop();
    const again = (at: number) => {
      const split = at > c.from && at < c.to;
      if (split) this.pieces.splice(this.i, 1, { ...c, to: at }, { ...c, from: at });
      const next = split ? this.i + 1 : this.i;
      if (!speaking.value?.paused) return void this.play(next);
      // Resuming makes the new clip (see resume).
      this.i = next;
      this.word = -1;
      this.audio.removeAttribute("src");
    };
    if (!clip || this.word < 0) return again(c.from);
    void clip.then((x) => again(c.from + x.words[this.word].start), () => again(c.from));
  }

  from(b: number, off = 0) {
    void this.play(this.indexOf(b, off));
  }

  destroy() {
    this.gen++;
    cancelAnimationFrame(this.raf);
    this.audio.pause();
    this.audio.removeAttribute("src");
    this.drop();
    this.unmark();
  }
}

let reader: Reader | null = null;
let lastUserMove = 0;
for (const ev of ["wheel", "keydown", "pointerdown"] as const)
  window.addEventListener(ev, (e) => {
    // Read aloud's own controls don't count.
    if (!(e.target as HTMLElement | null)?.closest?.(".speechbar")) lastUserMove = performance.now();
  }, { capture: true, passive: true });

/** Where reading starts: the selected text's paragraph, else the paragraph at the top of the view, else the page. */
function startOf(tab: Tab, flow: Flow): { b: number; off: number } {
  const view = tab.reflowView;
  const sel = getSelection();
  if (view && sel && sel.rangeCount && !sel.isCollapsed) {
    const el = (sel.anchorNode instanceof Element ? sel.anchorNode : sel.anchorNode?.parentElement)?.closest<HTMLElement>("[data-b]");
    if (el && view.el.contains(el)) return { b: Number(el.dataset.b), off: 0 };
  }
  if (view) return { b: view.currentBlock(), off: 0 };
  return { b: flow.pageStart[tab.page.value] ?? 0, off: 0 };
}

/** Starts reading the tab aloud (from `b` if given). */
export async function readAloud(tab: Tab, b?: number) {
  if (tab.status.value !== "ready" || !tab.info.value) return;
  stopReading();
  speaking.value = { tab, block: -1, paused: false, loading: true };
  let flow: Flow;
  try {
    [flow] = await Promise.all([analysisOf(tab).analysis.then((a) => flowOf(tab, a)), loadVoices().catch(() => [])]);
  } catch (e) {
    if (speaking.value?.tab === tab && !reader) speaking.value = null;
    showToast(`Couldn't read aloud: ${String((e as Error)?.message ?? e)}`, 5000);
    return;
  }
  // Stopped (or started elsewhere) while the text was being prepared.
  if (speaking.value?.tab !== tab || reader) return;
  const pieces = piecesOf(flow);
  if (!pieces.length) {
    speaking.value = null;
    showToast(flow.scans ? "This is a scan: recognize its text first (Tools › Recognize text)." : "There's no text here to read.", 5000);
    return;
  }
  reader = new Reader(tab, flow, pieces);
  const at = b !== undefined ? { b, off: 0 } : startOf(tab, flow);
  reader.from(at.b, at.off);
}

export function stopReading() {
  reader?.destroy();
  reader = null;
  speaking.value = null;
}

export function toggleReadAloud(tab: Tab | null) {
  if (speaking.value) return stopReading();
  if (tab) void readAloud(tab);
}

export function pauseOrResume() {
  if (speaking.value?.paused) reader?.resume();
  else reader?.pause();
}

export const skipParagraph = (dir: 1 | -1) => reader?.skip(dir);

/** While reading: carry on from paragraph `b` (a click in Read or Book mode). */
export function readFrom(tab: Tab, b: number) {
  if (reader && reader.tab === tab) reader.from(b);
}

export function setSpeech(patch: Partial<{ voice: string; rate: number }>) {
  updateSettings({ speech: { ...speechPrefs(), ...patch } });
  reader?.restart();
}

// The page order, not the edits: turning a page leaves its text as it was.
const madeFrom = (t: Tab): unknown[] => [t.info.value?.docId, t.edits.value?.order, t.textRev.value, overridesOf(t).value];

// A closed tab, a changed document or new layout corrections: stop (the text read no longer matches).
effect(() => {
  const s = speaking.value;
  if (!s || !reader) return;
  const now = madeFrom(reader.tab);
  if (!tabs.value.includes(reader.tab) || now.some((v, i) => v !== reader!.made[i])) stopReading();
});
