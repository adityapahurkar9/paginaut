// Find in the open document: the engine's search over the pages, or the reflowed text in Read and Book.
import { signal } from "@preact/signals";
import { api, newId } from "../api";
import { searchFlow, type FlowHit } from "../reflow/find";
import { doc, mode, page } from "./state";
import { views } from "./views";

export interface Found {
  query: string;
  /** Matches so far, and which one is shown (-1: none yet). */
  total: number;
  index: number;
  done: boolean;
  error: string | null;
}

export const found = signal<Found | null>(null);

// Pages: matches per page (flat quads), and the pages with matches in order.
let sid = 0;
let hits = new Map<number, number[][]>();
let pages: number[] = [];
// Read and Book: matches in the text.
let flowHits: FlowHit[] = [];
let timer = 0;

function at(i: number): { page: number; index: number } | null {
  for (const p of pages) {
    const n = hits.get(p)!.length;
    if (i < n) return { page: p, index: i };
    i -= n;
  }
  return null;
}

function show(i: number) {
  const f = found.value;
  if (!f || f.total === 0) return;
  const k = ((i % f.total) + f.total) % f.total;
  found.value = { ...f, index: k };
  if (mode.value === "pages") {
    const h = at(k);
    if (!h) return;
    views.viewer?.setSearchHits(hits, h);
    views.viewer?.revealHit(h.page, hits.get(h.page)![h.index]);
  } else {
    views.reflow?.setHits(flowHits, flowHits[k]);
    views.reflow?.showHit(flowHits[k]);
  }
}

export const next = () => show((found.value?.index ?? -1) + 1);
export const prev = () => show((found.value?.index ?? 0) - 1);

/** Searches for `query` after a short pause in typing (empty: clears). */
export function find(query: string, now = false) {
  clearTimeout(timer);
  timer = window.setTimeout(() => run(query.trim()), now ? 0 : 350);
}

export function clearFind() {
  clearTimeout(timer);
  if (sid) void api.cancel([sid]).catch(() => {});
  sid = 0;
  hits = new Map();
  pages = [];
  flowHits = [];
  found.value = null;
  views.viewer?.setSearchHits(new Map(), null);
  views.reflow?.setHits([], null);
}

function run(q: string) {
  const d = doc.value;
  clearFind();
  if (!d || !q) return;
  if (mode.value !== "pages") {
    const flow = views.reflow?.flow;
    if (!flow) return;
    const r = searchFlow(flow, q, { matchCase: false, wholeWord: false, regex: false });
    flowHits = r.hits;
    found.value = { query: q, total: r.hits.length, index: -1, done: true, error: r.error ?? null };
    // The first match after the place you're reading.
    const from = flow.pageStart[page.value] ?? 0;
    const first = r.hits.findIndex((h) => h.b >= from);
    if (r.hits.length) show(first < 0 ? 0 : first);
    return;
  }
  const id = (sid = newId());
  const start = page.value;
  const n = d.info.pageCount;
  // Pages are searched in parallel: the first match is shown once every page from `start` to it
  // has been searched (how far that is: `frontier`, wrapping round the end).
  const scanned = new Uint8Array(n);
  let frontier = 0;
  let shown = false;
  const ordinal = (h: { page: number; index: number }) => pages.filter((p) => p < h.page).reduce((k, p) => k + hits.get(p)!.length, 0) + h.index;
  const settle = (done: boolean) => {
    if (shown || !pages.length) return;
    const first = pages.find((p) => p >= start) ?? pages[0];
    if (!done && (first - start + n) % n >= frontier) return;
    shown = true;
    show(ordinal({ page: first, index: 0 }));
  };
  found.value = { query: q, total: 0, index: -1, done: false, error: null };
  api
    .search(d.info.docId, q, { matchCase: false, regex: false, snippets: false, start }, id, (ev) => {
      if (id !== sid) return;
      const f = found.value!;
      if (ev.kind === "hits") {
        // Pages arrive out of order: keep the match shown the same one.
        const active = f.index >= 0 ? at(f.index) : null;
        hits.set(ev.page, ev.hits);
        pages.push(ev.page);
        pages.sort((a, b) => a - b);
        found.value = { ...f, total: f.total + ev.hits.length, index: active ? ordinal(active) : -1 };
        views.viewer?.setSearchHits(hits, active);
        settle(false);
      } else if (ev.kind === "scanned") {
        scanned.fill(1, ev.from, ev.to);
        while (frontier < n && scanned[(start + frontier) % n]) frontier++;
        settle(false);
      } else {
        found.value = { ...f, done: true, error: ev.error };
        settle(true);
      }
    })
    .catch((e) => {
      if (id === sid) found.value = { query: q, total: 0, index: -1, done: true, error: String(e?.message ?? e) };
    });
}
