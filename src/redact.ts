// Redact: mark text or areas (by hand or by search), then remove what's under the marks,
// or only cover it. Sanitize takes hidden things out of the whole document.
import { signal } from "@preact/signals";
import { api, newId, type Quad } from "./api";
import { authorName } from "./annots";
import { DOC_STEP } from "./edits";
import { formsChanged } from "./forms";
import { showToast, type Tab } from "./store";

/** The tab being redacted (Redact panel open), or null. */
export const redactTab = signal<Tab | null>(null);
/** Mark areas (a box) even over text: logos, signatures, photos. */
export const redactAreas = signal(false);
/** Marks waiting to be applied: pages that have them, and how many in all. */
export const redactMarks = signal<{ pages: number; marks: number }>({ pages: 0, marks: 0 });

export function startRedact(tab: Tab) {
  if (!tab.info.value?.isPdf) return showToast("Only PDFs can be redacted. Save this file as a PDF first (Save as).", 4500);
  if (tab.mode.value !== "pages") return showToast("Switch to Single or Double view to redact.", 4000);
  redactTab.value = tab;
  void refreshMarks(tab);
}

export function stopRedact() {
  redactTab.value = null;
}

export async function refreshMarks(tab: Tab) {
  const info = tab.info.value;
  if (!info?.isPdf) return;
  const pages = await api.redactionPages(info.docId).catch(() => []);
  redactMarks.value = { pages: pages.length, marks: pages.reduce((n, [, m]) => n + m, 0) };
}

function changed(tab: Tab) {
  tab.history.push(DOC_STEP);
  tab.historyVersion.value++;
  formsChanged(tab, true);
  void refreshMarks(tab);
}

async function mark(tab: Tab, marks: { page: number; quads?: number[]; rect?: [number, number, number, number] }[]) {
  if (!marks.length) return 0;
  const n = await api.redactMark(tab.info.value!.docId, marks, authorName());
  changed(tab);
  return n;
}

/** A drag on the page while redacting: the text it selected, or (on scans, or with Areas on) the box dragged. */
export async function markFromSelection(tab: Tab, src: number, quads: Quad[], a: [number, number], b: [number, number]) {
  tab.viewer?.clearSelection();
  const r: [number, number, number, number] = [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.max(a[0], b[0]), Math.max(a[1], b[1])];
  try {
    if (quads.length && !redactAreas.value) await mark(tab, [{ page: src, quads: quads.flat() }]);
    else if (r[2] - r[0] >= 3 && r[3] - r[1] >= 3) await mark(tab, [{ page: src, rect: r }]);
  } catch (e) {
    showToast(`Couldn't mark it: ${e}`, 5000);
  }
}

/** Aadhaar's last digit is a Verhoeff check digit: "2026 2025 2024" in a table isn't one. */
function verhoeff(digits: string): boolean {
  const d = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9], [1, 2, 3, 4, 0, 6, 7, 8, 9, 5], [2, 3, 4, 0, 1, 7, 8, 9, 5, 6], [3, 4, 0, 1, 2, 8, 9, 5, 6, 7],
    [4, 0, 1, 2, 3, 9, 5, 6, 7, 8], [5, 9, 8, 7, 6, 0, 4, 3, 2, 1], [6, 5, 9, 8, 7, 1, 0, 4, 3, 2], [7, 6, 5, 9, 8, 2, 1, 0, 4, 3],
    [8, 7, 6, 5, 9, 3, 2, 1, 0, 4], [9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
  ];
  const p = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9], [1, 5, 7, 6, 2, 8, 3, 0, 9, 4], [5, 8, 0, 3, 7, 9, 6, 1, 4, 2], [8, 9, 1, 6, 0, 4, 3, 5, 2, 7],
    [9, 4, 5, 3, 1, 2, 6, 8, 7, 0], [4, 2, 8, 6, 5, 7, 3, 9, 0, 1], [2, 7, 9, 3, 8, 0, 6, 4, 1, 5], [7, 0, 4, 6, 9, 1, 3, 2, 5, 8],
  ];
  let c = 0;
  [...digits].reverse().forEach((ch, i) => (c = d[c][p[i % 8][+ch]]));
  return c === 0;
}

/** Patterns to find and mark (kept small); `check` confirms a match from its text. */
export const PATTERNS: { name: string; hint: string; re: string; check?: (text: string) => boolean }[] = [
  // The fourth letter says who holds it (P person, C company, H family, F firm…).
  { name: "PAN", hint: "ABCDE1234F", re: "\\b[A-Z]{3}[ABCFGHJLPT][A-Z][0-9]{4}[A-Z]\\b" },
  {
    name: "Aadhaar",
    hint: "2345 6789 0123",
    re: "\\b[2-9][0-9]{3}[ -]?[0-9]{4}[ -]?[0-9]{4}\\b",
    check: (t) => {
      const digits = t.replace(/[^0-9]/g, "");
      return digits.length === 12 && verhoeff(digits);
    },
  },
  { name: "Phone", hint: "+91 98765 43210", re: "(?:\\+91[ -]?|\\b0)?[6-9][0-9]{4}[ -]?[0-9]{5}\\b" },
  // Starting on a letter keeps MuPDF's pattern engine from giving up on long pages.
  { name: "Email", hint: "name@example.com", re: "\\w[-.\\w]*@\\w[-.\\w]*\\.\\w+" },
  { name: "IFSC", hint: "SBIN0001234", re: "\\b[A-Z]{4}0[A-Z0-9]{6}\\b" },
  { name: "GSTIN", hint: "27ABCDE1234F1Z5", re: "\\b[0-9]{2}[A-Z]{5}[0-9]{4}[A-Z][1-9A-Z]Z[0-9A-Z]\\b" },
];

/** Finds every match in the document: file page → hits (flat quads, 8 numbers each). */
export function findAll(tab: Tab, needle: string, regex: boolean, matchCase: boolean): Promise<{ hits: Map<number, number[][]>; limited: boolean; partial: boolean }> {
  const info = tab.info.value!;
  const hits = new Map<number, number[][]>();
  return new Promise((resolve, reject) => {
    void api
      .search(info.docId, needle, { matchCase, regex, snippets: false, start: 0 }, newId(), (ev) => {
        if (ev.kind === "hits") hits.set(ev.page, ev.hits);
        else if (ev.kind === "done") {
          // A pattern can give up on a very long page: what it found elsewhere still counts.
          if (ev.error && !hits.size) reject(regex ? "That pattern isn't valid, or too slow for these pages." : ev.error);
          else resolve({ hits, limited: ev.limited, partial: !!ev.error });
        }
      })
      .catch(reject);
  });
}

/** Keeps the hits whose text passes `check` (read from under each hit). */
export async function checkHits(tab: Tab, hits: Map<number, number[][]>, check: (text: string) => boolean) {
  const docId = tab.info.value!.docId;
  const out = new Map<number, number[][]>();
  for (const [page, list] of hits) {
    const keep: number[][] = [];
    for (const q of list) {
      const xs = q.filter((_, i) => i % 2 === 0), ys = q.filter((_, i) => i % 2 === 1);
      const text = await api.areaText(docId, page, [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)]).catch(() => "");
      if (check(text)) keep.push(q);
    }
    if (keep.length) out.set(page, keep);
  }
  return out;
}

/** Marks every match (each hit its own mark, so one can be taken off again). Returns how many. */
export async function markAll(tab: Tab, hits: Map<number, number[][]>) {
  const marks = [...hits].flatMap(([page, list]) => list.map((quads) => ({ page, quads })));
  return mark(tab, marks);
}

/** Takes every mark off (one undo step). */
export async function clearMarks(tab: Tab) {
  const n = await api.redactClear(tab.info.value!.docId);
  if (n) changed(tab);
  return n;
}

/** Applies every mark (one undo step). */
export async function applyRedactions(tab: Tab, cover: boolean, rgb: [number, number, number], label: string) {
  const r = await api.redactApply(tab.info.value!.docId, cover, rgb, label);
  if (r.marks) changed(tab);
  return r;
}

/** Takes things out of the whole document (one undo step). */
export async function sanitize(tab: Tab, what: number) {
  const r = await api.sanitize(tab.info.value!.docId, what);
  changed(tab);
  return r;
}
