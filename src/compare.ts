// Compare: two files side by side, lined up word by word (see compare.rs), in a tab of their own. A PDF with a
// PDF, or with the Word, PowerPoint, Excel or Markdown file it was made from.
import { batch, signal } from "@preact/signals";
import { api, type CompareOptions, type CompareOutcome, type CompareSideReq, type DocInfo } from "./api";
import { activeKey, baseName, HOME, homeOpen, settings, tabs, updateSettings } from "./store";

/** The Compare tab's key (like the Home tab's). */
export const COMPARE = "compare";

/** Files whose words are read straight from the file (Word, PowerPoint, Excel and OpenDocument). */
const TEXT_READ = ["docx", "docm", "dotx", "dotm", "pptx", "pptm", "ppsx", "ppsm", "potx", "potm", "xlsx", "xlsm", "xltx", "xltm", "odt", "ott", "odp", "otp", "ods", "ots"];
const ext = (p: string) => p.split(".").pop()?.toLowerCase() ?? "";
export const readsText = (path: string) => TEXT_READ.includes(ext(path));

export const COMPARE_DEFAULTS: CompareOptions = { matchCase: true, skipEdges: true, pictures: true };

export interface CompareSide {
  path: string;
  name: string;
  /** An open document (PDFs, Markdown, pictures, older Office files through a PDF made of them). */
  info: DocInfo | null;
  sizes: [number, number][] | null;
}

export interface CompareSession {
  a: CompareSide;
  b: CompareSide;
  state: "opening" | "comparing" | "done" | "error";
  /** What's happening, and how far along (0–1, or -1 when unknown). */
  progress: string;
  fraction: number;
  error: string;
  outcome: CompareOutcome | null;
  /** Bumped by each run (the panes rebuild their marks). */
  run: number;
}

export const compare = signal<CompareSession | null>(null);
/** The change picked in the list (or on a page), -1 for none. */
export const compareSel = signal(-1);
/** Scroll the two sides together. */
export const compareSync = signal(true);
/** The setup dialog, with the two files chosen so far. */
export const compareSetup = signal<{ a: string | null; b: string | null } | null>(null);
/** A password asked for while opening a side. */
export const comparePassword = signal<{ name: string; wrong: boolean; answer: (pw: string | null) => void } | null>(null);

export function openCompareSetup(a?: string | null, b?: string | null) {
  const tab = tabs.value.find((t) => t.key === activeKey.value);
  compareSetup.value = { a: a ?? tab?.path ?? null, b: b ?? null };
}

export const compareOptions = () => ({ ...COMPARE_DEFAULTS, ...settings.value.compare });

// The documents a comparison opened (closed with it).
let owned: number[] = [];
let generation = 0;

function closeDocs() {
  for (const id of owned) void api.close(id).catch(() => {});
  owned = [];
}

function askPassword(name: string, wrong: boolean): Promise<string | null> {
  return new Promise((resolve) => {
    comparePassword.value = {
      name,
      wrong,
      answer: (pw) => {
        comparePassword.value = null;
        resolve(pw);
      },
    };
  });
}

async function openSide(path: string, gen: number): Promise<CompareSide> {
  const side: CompareSide = { path, name: baseName(path), info: null, sizes: null };
  if (readsText(path)) return side;
  let info = await api.open(path);
  owned.push(info.docId);
  let wrong = false;
  while (info.needsPassword) {
    const pw = await askPassword(side.name, wrong);
    if (pw === null || gen !== generation) throw new Error("stopped");
    const next = await api.unlock(info.docId, pw);
    if (next) info = next;
    wrong = true;
  }
  return { ...side, info, sizes: await api.pageSizes(info.docId) };
}

const request = (s: CompareSide): CompareSideReq => ({
  path: s.path,
  doc: s.info?.docId ?? null,
  // Pages as drawn (not laid out by Paginaut, not made from an Office file): drawings can be compared too.
  fixed: !!s.info && !s.info.reflowable && !s.info.converted,
});

function update(patch: Partial<CompareSession>) {
  if (compare.value) compare.value = { ...compare.value, ...patch };
}

/** Opens both files and compares them in the Compare tab. */
export async function startCompare(a: string, b: string) {
  const gen = ++generation;
  void api.compareStop().catch(() => {});
  comparePassword.value?.answer(null);
  closeDocs();
  const blank = (path: string): CompareSide => ({ path, name: baseName(path), info: null, sizes: null });
  batch(() => {
    compare.value = { a: blank(a), b: blank(b), state: "opening", progress: "Opening…", fraction: -1, error: "", outcome: null, run: gen };
    compareSel.value = -1;
    if (activeKey.value === HOME) homeOpen.value = false;
    activeKey.value = COMPARE;
  });
  try {
    const sa = await openSide(a, gen);
    if (gen !== generation) return;
    update({ a: sa });
    const sb = await openSide(b, gen);
    if (gen !== generation) return;
    update({ b: sb });
    await runCompare(gen);
  } catch (e) {
    if (gen !== generation) return;
    const msg = e instanceof Error ? e.message : String(e);
    update({ state: "error", error: msg === "stopped" ? "Stopped." : msg });
  }
}

async function runCompare(gen: number) {
  const s = compare.value;
  if (!s) return;
  update({ state: "comparing", progress: "Comparing…", fraction: -1, error: "", outcome: null, run: gen });
  compareSel.value = -1;
  const names = { a: s.a.name, b: s.b.name };
  try {
    const outcome = await api.compareRun(request(s.a), request(s.b), compareOptions(), (e) => {
      if (gen !== generation) return;
      if (e.kind === "reading") update({ progress: `Reading ${names[e.side]}…`, fraction: e.total > 1 ? e.done / e.total : -1 });
      else if (e.kind === "comparing") update({ progress: "Lining up the words…", fraction: -1 });
      else update({ progress: "Comparing drawings and scans…", fraction: e.done / Math.max(1, e.total) });
    });
    if (gen !== generation) return;
    update({ state: "done", outcome });
  } catch (e) {
    if (gen !== generation) return;
    update({ state: "error", error: String(e) === "stopped" ? "Stopped." : String(e) });
  }
}

export function setCompareOption(patch: Partial<CompareOptions>) {
  updateSettings({ compare: { ...compareOptions(), ...patch } });
}

/** Compares the same two files again (other options). */
export function compareAgain(patch?: Partial<CompareOptions>) {
  if (patch) setCompareOption(patch);
  const s = compare.value;
  if (!s || s.state === "opening") return;
  // Sides that failed to open are opened again.
  if (s.state === "error" && ((!s.a.info && !readsText(s.a.path)) || (!s.b.info && !readsText(s.b.path)))) return void startCompare(s.a.path, s.b.path);
  void runCompare(++generation);
}

export function swapSides() {
  const s = compare.value;
  if (!s || s.state === "opening") return;
  compare.value = { ...s, a: s.b, b: s.a };
  void runCompare(++generation);
}

export function stopCompare() {
  void api.compareStop().catch(() => {});
  comparePassword.value?.answer(null);
}

export function closeCompare() {
  generation++;
  stopCompare();
  closeDocs();
  batch(() => {
    compare.value = null;
    compareSel.value = -1;
    if (activeKey.value === COMPARE) activeKey.value = tabs.value[tabs.value.length - 1]?.key ?? null;
  });
}
