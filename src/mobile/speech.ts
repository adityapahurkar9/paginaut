// Read aloud with Android's own text-to-speech: paragraph by paragraph from where you are, the
// paragraph kept in view (and marked in Read and Book). Android's voices can't pause, so pausing
// stops and remembers the paragraph.
import { signal } from "@preact/signals";
import { invoke } from "@tauri-apps/api/core";
import { textOf, type Flow } from "../reflow/model";
import { flowOf } from "./flow";
import { doc, errorText, mode, page, say } from "./state";
import { goTo, views } from "./views";

export const RATES = [0.8, 1, 1.25, 1.5, 2];

export interface Reading {
  playing: boolean;
  /** Paragraph (block of the document's text) being read. */
  block: number;
  rate: number;
}

function savedRate() {
  const r = Number(localStorage.getItem("paginaut.m.rate"));
  return RATES.includes(r) ? r : 1;
}

export const reading = signal<Reading | null>(null);
let run = 0;

/** Android reads at most ~4000 characters at once: long paragraphs go in pieces, cut after a sentence. */
function pieces(text: string): string[] {
  const out: string[] = [];
  let rest = text.trim();
  while (rest.length > 3000) {
    const cut = Math.max(rest.lastIndexOf(". ", 3000), rest.lastIndexOf("। ", 3000), rest.lastIndexOf(" ", 3000));
    const at = cut > 1000 ? cut + 1 : 3000;
    out.push(rest.slice(0, at));
    rest = rest.slice(at).trim();
  }
  if (rest) out.push(rest);
  return out;
}

function* items(flow: Flow, from: number) {
  for (let b = from; b < flow.blocks.length; b++) {
    const blk = flow.blocks[b];
    if (blk.k === "img") continue;
    for (const text of pieces(textOf(blk))) yield { b, text };
  }
}

function mark(b: number | null) {
  for (const el of document.querySelectorAll(".rf [data-b].speaking")) el.classList.remove("speaking");
  if (b !== null) for (const el of document.querySelectorAll(`.rf [data-b="${b}"]`)) el.classList.add("speaking");
}

/** Keeps the paragraph being read in view. */
function follow(flow: Flow, b: number) {
  if (mode.value === "pages") {
    const p = flow.blocks[b]?.page;
    if (p !== undefined && p !== page.value) goTo(p);
  } else {
    views.reflow?.reveal(b, 0);
    mark(b);
  }
}

/** Starts reading at paragraph `from`, or at the page you're on. */
export async function startReading(from?: number) {
  const d = doc.value;
  if (!d) return;
  const my = ++run;
  const rate = reading.value?.rate ?? savedRate();
  let flow: Flow;
  try {
    flow = views.reflow?.flow ?? (await flowOf(d.info.docId, d.info.pageCount, page.value));
  } catch (e) {
    say(errorText(e));
    return;
  }
  if (my !== run || doc.value !== d) return;
  const b0 = from ?? (mode.value === "pages" ? flow.pageStart[page.value] : views.reflow?.currentBlock()) ?? 0;
  const it = items(flow, b0);
  // Two pieces queued at a time, so there's no pause between paragraphs.
  const queued: { b: number; done: Promise<{ stopped: boolean }> }[] = [];
  const fill = () => {
    while (queued.length < 2) {
      const n = it.next();
      if (n.done) return;
      queued.push({ b: n.value.b, done: invoke<{ stopped: boolean }>("speak", { text: n.value.text, rate, lang: null }) });
    }
  };
  fill();
  if (!queued.length) {
    say("There's no text to read here. Scanned pages are pictures of text.");
    return;
  }
  reading.value = { playing: true, block: b0, rate };
  while (queued.length) {
    const cur = queued[0];
    reading.value = { ...reading.value!, block: cur.b };
    follow(flow, cur.b);
    let r: { stopped: boolean };
    try {
      r = await cur.done;
    } catch (e) {
      if (my === run) {
        say(errorText(e));
        stopReading();
      }
      return;
    }
    if (my !== run || r.stopped) return;
    queued.shift();
    fill();
  }
  reading.value = null;
  mark(null);
}

/** Pause: stops the voice, keeping the place. */
export function pauseReading() {
  const r = reading.value;
  if (!r) return;
  run++;
  void invoke("stop_speaking").catch(() => {});
  reading.value = { ...r, playing: false };
}

export function resumeReading() {
  const r = reading.value;
  if (r) void startReading(r.block);
}

export function stopReading() {
  run++;
  void invoke("stop_speaking").catch(() => {});
  reading.value = null;
  mark(null);
}

/** The next speed; while reading, it carries on at that speed from the paragraph it's on. */
export function nextRate() {
  const r = reading.value;
  if (!r) return;
  const rate = RATES[(RATES.indexOf(r.rate) + 1) % RATES.length];
  try {
    localStorage.setItem("paginaut.m.rate", String(rate));
  } catch {
    /* not kept */
  }
  if (r.playing) {
    run++;
    void invoke("stop_speaking")
      .catch(() => {})
      .then(() => {
        reading.value = { ...r, rate };
        void startReading(r.block);
      });
  } else reading.value = { ...r, rate };
}
