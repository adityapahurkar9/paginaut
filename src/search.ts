// Document search state. Results stream in per page, out of order (pages are
// searched in parallel), so hits are kept per page plus a sorted page list,
// updated in place — never rebuilt as one big list on each update.
import { signal } from "@preact/signals";
import { api, newId } from "./api";
import type { Tab } from "./store";
import { sourceCount } from "./edits";

/** Search options shared by the find bar and the command palette (remembered). */
export interface SearchOpts {
  matchCase: boolean;
  wholeWord: boolean;
  regex: boolean;
}

function loadOpts(): SearchOpts {
  try {
    return { matchCase: false, wholeWord: false, regex: false, ...JSON.parse(localStorage.getItem("paginaut.searchOpts") || "{}") };
  } catch {
    return { matchCase: false, wholeWord: false, regex: false };
  }
}
export const searchOpts = signal<SearchOpts>(loadOpts());
export function setSearchOpts(patch: Partial<SearchOpts>) {
  searchOpts.value = { ...searchOpts.value, ...patch };
  try {
    localStorage.setItem("paginaut.searchOpts", JSON.stringify(searchOpts.value));
  } catch {
    /* not persisted */
  }
}

const escapeRegex = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Whole-word search is sent to the engine as a regular expression with word boundaries. */
function engineNeedle(q: string, o: SearchOpts): { needle: string; regex: boolean } {
  if (!o.wholeWord) return { needle: q, regex: o.regex };
  return { needle: o.regex ? `\\b(?:${q})\\b` : `\\b${escapeRegex(q)}\\b`, regex: true };
}

export interface HitRef {
  page: number;
  index: number;
}

export interface SearchState {
  query: string;
  sid: number;
  /** Page → hits on that page; each hit is flat quad coordinates (8 per quad). */
  hits: Map<number, number[][]>;
  /** Pages that have hits, ascending. */
  pages: number[];
  /** Page → text of the line around each hit (only when snippets were requested). */
  snippets: Map<number, string[]>;
  total: number;
  error: string | null;
  active: HitRef | null;
  done: boolean;
  /** Stopped early because the pattern matched too much. */
  limited: boolean;
  /** True once the user steps through results; until then the first hit after the start page wins. */
  stepped: boolean;
  startPage: number;
  /** Pages searched so far, and how many pages from startPage onwards (wrapping) are all searched. */
  scanned: Uint8Array;
  frontier: number;
}

/** 0-based position of a hit among all hits (for "3 of 57"). */
export function ordinal(s: SearchState, h: HitRef | null): number {
  if (!h) return -1;
  let n = 0;
  for (const p of s.pages) {
    if (p === h.page) return n + h.index;
    n += s.hits.get(p)!.length;
  }
  return -1;
}

function firstAtOrAfter(s: SearchState, page: number): HitRef | null {
  if (!s.pages.length) return null;
  const p = s.pages.find((x) => x >= page) ?? s.pages[0];
  return { page: p, index: 0 };
}

export function step(s: SearchState, dir: 1 | -1): HitRef | null {
  const a = s.active;
  if (!a || !s.pages.length) return firstAtOrAfter(s, s.startPage);
  const n = s.hits.get(a.page)?.length ?? 0;
  const i = a.index + dir;
  if (i >= 0 && i < n) return { page: a.page, index: i };
  const k = s.pages.indexOf(a.page);
  const next = s.pages[(k + dir + s.pages.length) % s.pages.length];
  return { page: next, index: dir > 0 ? 0 : s.hits.get(next)!.length - 1 };
}

function insertSorted(arr: number[], v: number) {
  let lo = 0, hi = arr.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (arr[mid] < v) lo = mid + 1;
    else hi = mid;
  }
  if (arr[lo] !== v) arr.splice(lo, 0, v);
}

let timer = 0;

/** Starts (after a short typing pause) a new search in `tab`, replacing any running one. */
export function runSearch(
  tab: Tab,
  query: string,
  onFirstReveal: (tab: Tab) => void,
  onError: (e: unknown) => void,
  extra: { snippets?: boolean; delay?: number } = {},
) {
  clearTimeout(timer);
  const prev = tab.search.value;
  if (prev) api.cancel([prev.sid]).catch(() => {});
  const info = tab.info.value;
  const q = query.trim();
  if (!info || !q) {
    tab.search.value = null;
    tab.viewer?.setSearchHits(new Map(), null);
    return;
  }
  timer = window.setTimeout(() => {
    const sid = newId();
    const opts = searchOpts.value;
    const { needle, regex } = engineNeedle(q, opts);
    const n = tab.edits.value ? sourceCount(tab.edits.value) : (tab.info.value?.pageCount ?? 0);
    const startPage = tab.page.value;
    const state: SearchState = {
      query: q, sid, hits: new Map(), pages: [], snippets: new Map(), total: 0, error: null,
      active: null, done: false, limited: false, stepped: false, startPage, scanned: new Uint8Array(n), frontier: 0,
    };
    tab.search.value = state;
    // Pages are searched in parallel and finish out of order. Jump to the first
    // hit at or after where you are only once every page between here and it
    // has been searched — otherwise the view would bounce between hits.
    const settle = (cur: SearchState) => {
      if (cur.stepped || cur.active) return;
      const first = firstAtOrAfter(cur, cur.startPage);
      if (!first) return;
      const offset = (first.page - cur.startPage + n) % n;
      if (!cur.done && offset > cur.frontier) return;
      cur.active = first;
      tab.viewer?.setSearchHits(cur.hits, first);
      tab.search.value = { ...cur };
      onFirstReveal(tab);
    };
    api
      .search(info.docId, needle, { matchCase: opts.matchCase, regex, snippets: !!extra.snippets, start: startPage }, sid, (ev) => {
        const cur = tab.search.value;
        if (!cur || cur.sid !== sid) return;
        if (ev.kind === "done") {
          const error = ev.error ? (regex ? "That pattern isn't valid." : ev.error) : null;
          const next = { ...cur, done: true, error, limited: ev.limited };
          tab.search.value = next;
          settle(next);
          return;
        }
        if (ev.kind === "scanned") {
          cur.scanned.fill(1, ev.from, ev.to);
          while (cur.frontier < n && cur.scanned[(cur.startPage + cur.frontier) % n]) cur.frontier++;
          settle(cur);
          return;
        }
        cur.hits.set(ev.page, ev.hits);
        if (ev.snippets.length) cur.snippets.set(ev.page, ev.snippets);
        insertSorted(cur.pages, ev.page);
        cur.total += ev.hits.length;
        tab.search.value = { ...cur };
        tab.viewer?.setSearchHits(cur.hits, cur.active);
        settle(cur);
      })
      .catch(onError);
  }, extra.delay ?? 160);
}

export function reveal(tab: Tab) {
  const s = tab.search.value;
  const h = s?.active;
  if (!s || !h) return;
  const quad = s.hits.get(h.page)?.[h.index];
  tab.viewer?.setSearchHits(s.hits, h);
  if (quad) tab.viewer?.revealHit(h.page, quad);
}

export function stepSearch(tab: Tab, dir: 1 | -1) {
  const s = tab.search.value;
  if (!s || !s.total) return;
  tab.search.value = { ...s, active: step(s, dir), stepped: true };
  reveal(tab);
}

export function clearSearch(tab: Tab) {
  const s = tab.search.value;
  if (s) api.cancel([s.sid]).catch(() => {});
  tab.search.value = null;
  tab.viewer?.setSearchHits(new Map(), null);
}

/** First `limit` hits in page order, with snippets, for result lists. */
export function listHits(s: SearchState, limit: number): { page: number; index: number; snippet: string; occurrence: number }[] {
  const out: { page: number; index: number; snippet: string; occurrence: number }[] = [];
  for (const p of s.pages) {
    const hits = s.hits.get(p)!;
    const snips = s.snippets.get(p) ?? [];
    for (let i = 0; i < hits.length; i++) {
      // Several hits on one line share a snippet; remember which occurrence this one is.
      let occurrence = 0;
      while (occurrence < i && snips[i - 1 - occurrence] === snips[i]) occurrence++;
      out.push({ page: p, index: i, snippet: snips[i] ?? "", occurrence });
      if (out.length >= limit) return out;
    }
  }
  return out;
}

/** Selects a specific hit (e.g. picked from a list) and scrolls to it. */
export function selectHit(tab: Tab, page: number, index: number) {
  const s = tab.search.value;
  if (!s) return;
  tab.search.value = { ...s, active: { page, index }, stepped: true };
  reveal(tab);
}
