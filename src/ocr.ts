// Text recognition (OCR): finding scanned documents, running recognition in the
// background, and the language models (built in, or downloaded on request).
// The engine adds the text to the pages as an invisible layer: one undo step,
// kept by saving.
import { effect, signal, type Signal } from "@preact/signals";
import { api, newId } from "./api";
import { DOC_STEP } from "./edits";
import { formsChanged } from "./forms";
import * as P from "./pages";
import { ask, baseName, settings, showToast, storeItem, tabs, updateSettings, type Tab } from "./store";

export type OcrQuality = "fast" | "best";

export interface OcrLang {
  code: string;
  name: string;
  fastSize: number;
  bestSize: number;
  builtin: boolean;
  fast: boolean;
  best: boolean;
}

export interface OcrEnd {
  complete: boolean;
  pages: number;
  /** Of those, pages whose earlier recognised text was replaced. */
  replaced: number;
  /** Pages that read the right way up only when turned: [page in the file, clockwise degrees]. */
  turned: [number, number][];
  words: number;
  changed: boolean;
  dirty: boolean;
  error: string | null;
}

export type OcrState =
  | { phase: "running"; sid: number; done: number; total: number; stopping: boolean; finished: Promise<void> }
  | { phase: "done"; end: OcrEnd; stopped: boolean };

const states = new WeakMap<Tab, Signal<OcrState | null>>();
/** Recognition on this tab: running, or how the last run ended (until dismissed). */
export function ocrState(tab: Tab): Signal<OcrState | null> {
  let s = states.get(tab);
  if (!s) states.set(tab, (s = signal(null)));
  return s;
}

const scans = new WeakMap<Tab, Signal<boolean>>();
/** The document looks scanned (pictures of pages without text). */
export function looksScanned(tab: Tab): Signal<boolean> {
  let s = scans.get(tab);
  if (!s) scans.set(tab, (s = signal(false)));
  return s;
}

/** The Recognize text dialog, for this tab. */
export const ocrDialog = signal<Tab | null>(null);

const IMAGE_EXT = ["png", "jpg", "jpeg", "jfif", "tif", "tiff", "bmp", "gif", "jp2", "jpx", "pnm", "pbm", "pgm", "ppm", "pam", "jxr"];

// ---------------------------------------------------------------- finding scans

const DISMISSED_KEY = "paginaut.ocrDismissed";
function dismissedList(): string[] {
  try {
    return JSON.parse(localStorage.getItem(DISMISSED_KEY) ?? "[]");
  } catch {
    return [];
  }
}
export function offerDismissed(tab: Tab) {
  return dismissedList().includes(tab.path.toLowerCase());
}
/** Stops offering recognition for this file. */
export function dismissOffer(tab: Tab) {
  const list = [tab.path.toLowerCase(), ...dismissedList().filter((p) => p !== tab.path.toLowerCase())].slice(0, 200);
  storeItem(DISMISSED_KEY, JSON.stringify(list));
  looksScanned(tab).value = false;
}

P.onDocReady(async (tab) => {
  const info = tab.info.value;
  if (!info) return;
  const ext = tab.path.split(".").pop()?.toLowerCase() ?? "";
  if (!info.isPdf) {
    looksScanned(tab).value = IMAGE_EXT.includes(ext);
    return;
  }
  try {
    const r = await api.ocrCheck(info.docId);
    looksScanned(tab).value = r.sampled > 0 && r.scans * 2 >= r.sampled;
  } catch {
    looksScanned(tab).value = false;
  }
});

// ---------------------------------------------------------------- languages

export const ocrLangs = signal<OcrLang[]>([]);
export async function loadLangs() {
  try {
    ocrLangs.value = (await api.ocrLanguages()).sort((a, b) => a.name.localeCompare(b.name));
  } catch (e) {
    showToast(`Couldn't list OCR languages: ${e}`);
  }
}

export function hasModel(l: OcrLang, q: OcrQuality) {
  return q === "best" ? l.best : l.builtin || l.fast;
}

/** Downloads in progress: "code/quality" → fraction done. */
export const downloads = signal<Record<string, number>>({});

export async function downloadModel(code: string, q: OcrQuality): Promise<boolean> {
  const key = `${code}/${q}`;
  if (key in downloads.value) return false;
  downloads.value = { ...downloads.value, [key]: 0 };
  try {
    await api.ocrDownload(code, q, (ev) => (downloads.value = { ...downloads.value, [key]: ev.total ? ev.done / ev.total : 0 }));
    return true;
  } catch (e) {
    if (String(e) !== "stopped") showToast(`Couldn't download: ${e}`, 5000);
    return false;
  } finally {
    const { [key]: _, ...rest } = downloads.value;
    downloads.value = rest;
    await loadLangs();
  }
}

export function stopDownload(code: string, q: OcrQuality) {
  void api.ocrDownloadStop(code, q);
}

export async function deleteModel(code: string, q: OcrQuality) {
  try {
    await api.ocrDelete(code, q);
  } catch (e) {
    showToast(`Couldn't remove it: ${e}`);
  }
  await loadLangs();
}

export function rememberChoice(langs: string[], quality: OcrQuality) {
  updateSettings({ ocr: { langs, quality } });
}

// ---------------------------------------------------------------- running

/** Opens the Recognize text dialog. Other formats (pictures) are saved as a PDF first. */
export async function recognize(tab: Tab) {
  const info = tab.info.value;
  if (!info || tab.status.value !== "ready") return;
  if (ocrState(tab).value?.phase === "running") return;
  if (!info.isPdf) {
    const choice = await ask(
      "Save as a PDF first?",
      `Recognized text is added to the pages of a PDF. Paginaut will save ${baseName(tab.path)} as a PDF, then recognize its text.`,
      [
        { label: "Save as PDF…", value: "save", primary: true },
        { label: "Cancel", value: "cancel" },
      ],
    );
    if (choice !== "save" || !(await P.save(tab))) return;
  }
  void loadLangs();
  ocrDialog.value = tab;
}

/** Starts recognition on `pages` (indices in the file); `replace` reads text Paginaut recognised before again. */
export function startOcr(tab: Tab, pages: number[], langs: string[], quality: OcrQuality, replace = false) {
  const info = tab.info.value;
  if (!info || !pages.length) return;
  const state = ocrState(tab);
  const sid = newId();
  let resolve!: () => void;
  const finished = new Promise<void>((r) => (resolve = r));
  state.value = { phase: "running", sid, done: 0, total: pages.length, stopping: false, finished };
  const docId = info.docId;
  api
    .ocrRun(docId, pages, langs, quality, replace, sid, (ev) => {
      const cur = state.value;
      if (ev.kind === "progress") {
        if (cur?.phase === "running" && cur.sid === sid) state.value = { ...cur, done: ev.done, total: ev.total };
        return;
      }
      const stopped = cur?.phase === "running" && cur.stopping;
      finishedRun(tab, docId, ev.end, stopped);
      resolve();
    })
    .catch((e) => {
      state.value = null;
      resolve();
      showToast(`Couldn't recognize text: ${e}`, 5000);
    });
}

function finishedRun(tab: Tab, docId: number, end: OcrEnd, stopped: boolean) {
  // The tab may have been closed, or saved as another document meanwhile.
  if (tab.info.value?.docId !== docId) return;
  if (end.changed) {
    tab.history.push(DOC_STEP);
    tab.historyVersion.value++;
    tab.textRev.value++;
    formsChanged(tab, end.dirty);
    looksScanned(tab).value = false;
  }
  ocrState(tab).value = { phase: "done", end, stopped };
}

/** Stops recognition; the pages read so far keep their text. Resolves when it has stopped. */
export async function stopOcr(tab: Tab) {
  const s = ocrState(tab).value;
  if (s?.phase !== "running") return;
  ocrState(tab).value = { ...s, stopping: true };
  await api.cancel([s.sid]);
  await s.finished;
}

export function dismissOcrState(tab: Tab) {
  ocrState(tab).value = null;
}

/** Removes the text Paginaut recognised earlier (one undo step). */
export async function removeRecognized(tab: Tab) {
  const info = tab.info.value;
  if (!info?.isPdf) return;
  try {
    const r = await api.ocrRemove(info.docId);
    if (!r.pages) {
      showToast("There's no text recognized by Paginaut in this document");
      return;
    }
    tab.history.push(DOC_STEP);
    tab.historyVersion.value++;
    tab.textRev.value++;
    formsChanged(tab, r.dirty);
    if (ocrState(tab).value?.phase === "done") dismissOcrState(tab);
    showToast(`Removed recognized text from ${r.pages} page${r.pages === 1 ? "" : "s"} · Ctrl+Z to undo`);
  } catch (e) {
    showToast(`Couldn't remove the text: ${e}`);
  }
}

// Saving while recognition runs: stop it first (what's done is kept and saved).
P.setBeforeSave(async (tab) => {
  if (ocrState(tab).value?.phase !== "running") return true;
  const choice = await ask("Text recognition is still running", "Stop it and save? Pages already recognized keep their text.", [
    { label: "Stop and save", value: "stop", primary: true },
    { label: "Cancel", value: "cancel" },
  ]);
  if (choice !== "stop") return false;
  await stopOcr(tab);
  return true;
});

// A closed tab's recognition stops (the engine skips adding text to a closed document).
let running = new Set<Tab>();
effect(() => {
  const open = new Set(tabs.value);
  for (const t of running) {
    const s = ocrState(t).value;
    if (!open.has(t) && s?.phase === "running") void api.cancel([s.sid]);
  }
  running = new Set(tabs.value.filter((t) => ocrState(t).value?.phase === "running"));
});

/** Pages from text like "1-5, 8, 12-" (1-based, display order) → display indices; null if invalid. */
export function parsePages(text: string, count: number): number[] | null {
  const out = new Set<number>();
  for (const part of text.split(/[,;\s]+/).filter(Boolean)) {
    const m = /^(\d*)\s*(?:-|–|to)?\s*(\d*)$/.exec(part);
    if (!m || (!m[1] && !m[2])) return null;
    const range = /-|–|to/.test(part);
    const a = m[1] ? parseInt(m[1], 10) : 1;
    const b = range ? (m[2] ? parseInt(m[2], 10) : count) : a;
    if (a < 1 || b < a || a > count) return null;
    for (let i = a; i <= Math.min(b, count); i++) out.add(i - 1);
  }
  return out.size ? [...out].sort((x, y) => x - y) : null;
}

export const ocrDefaults = () => settings.value.ocr;
