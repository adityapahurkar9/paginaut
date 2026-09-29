// Find in Read and Book modes: searches the reflowed text itself and marks
// matches with the CSS Custom Highlight API (no DOM changes, so page layout
// in Book mode is untouched).
import { textOf, type Flow } from "./model";

export interface FlowHit {
  /** Block index and character range within the block's text. */
  b: number;
  from: number;
  to: number;
}

export interface FindOptions {
  matchCase: boolean;
  wholeWord: boolean;
  regex: boolean;
}

const LIMIT = 20_000;

export function searchFlow(flow: Flow, query: string, o: FindOptions): { hits: FlowHit[]; error?: string; limited: boolean } {
  const q = query.trim();
  if (!q) return { hits: [], limited: false };
  let re: RegExp;
  try {
    const body = o.regex ? q : q.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    re = new RegExp(o.wholeWord ? `\\b(?:${body})\\b` : body, o.matchCase ? "gu" : "giu");
  } catch {
    return { hits: [], error: "That pattern isn't valid.", limited: false };
  }
  const hits: FlowHit[] = [];
  flow.blocks.forEach((blk, b) => {
    if (blk.k === "img" || hits.length >= LIMIT) return;
    const text = textOf(blk);
    re.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = re.exec(text)) && hits.length < LIMIT) {
      if (!m[0].length) {
        re.lastIndex++;
        continue;
      }
      hits.push({ b, from: m.index, to: m.index + m[0].length });
    }
  });
  return { hits, limited: hits.length >= LIMIT };
}

type HighlightRegistry = Map<string, unknown>;
const registry = (): HighlightRegistry | null => (globalThis.CSS as unknown as { highlights?: HighlightRegistry })?.highlights ?? null;
const HighlightCtor = (globalThis as unknown as { Highlight?: new (...r: Range[]) => unknown }).Highlight;

/** Marks `hits` inside `root` (elements carry data-b / data-o), the active one differently. */
export function paintHits(root: HTMLElement, hits: FlowHit[], active: FlowHit | null) {
  const reg = registry();
  if (!reg || !HighlightCtor) return;
  if (!hits.length) {
    reg.delete("rf-hit");
    reg.delete("rf-hit-on");
    return;
  }
  const byBlock = new Map<number, FlowHit[]>();
  for (const h of hits) {
    const list = byBlock.get(h.b);
    if (list) list.push(h);
    else byBlock.set(h.b, [h]);
  }
  const all: Range[] = [];
  const on: Range[] = [];
  for (const el of root.querySelectorAll<HTMLElement>("[data-b]")) {
    const list = byBlock.get(Number(el.dataset.b));
    if (!list) continue;
    const base = Number(el.dataset.o ?? 0);
    // Text nodes of the element with their starting offsets in the block.
    const nodes: [Text, number][] = [];
    let pos = base;
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode() as Text | null; n; n = walker.nextNode() as Text | null) {
      nodes.push([n, pos]);
      pos += n.data.length;
    }
    const end = pos;
    const at = (off: number): [Text, number] | null => {
      for (let i = nodes.length - 1; i >= 0; i--) if (off >= nodes[i][1]) return [nodes[i][0], Math.min(nodes[i][0].data.length, off - nodes[i][1])];
      return null;
    };
    for (const h of list) {
      if (h.to <= base || h.from >= end) continue;
      const a = at(Math.max(h.from, base)), z = at(Math.min(h.to, end));
      if (!a || !z) continue;
      const r = document.createRange();
      r.setStart(a[0], a[1]);
      r.setEnd(z[0], z[1]);
      all.push(r);
      if (active && h.b === active.b && h.from === active.from) on.push(r);
    }
  }
  reg.set("rf-hit", new HighlightCtor(...all));
  reg.set("rf-hit-on", new HighlightCtor(...on));
}

/** Screen rectangle of a hit inside `root`, if it's rendered there. */
export function hitRect(root: HTMLElement, h: FlowHit): DOMRect | null {
  for (const el of root.querySelectorAll<HTMLElement>(`[data-b="${h.b}"]`)) {
    const base = Number(el.dataset.o ?? 0);
    const len = el.textContent?.length ?? 0;
    if (h.from >= base && h.from < base + len) return el.getBoundingClientRect();
  }
  return null;
}
