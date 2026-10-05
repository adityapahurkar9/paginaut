// Read mode: the document's text reflowed into one scrolling column at a
// comfortable width. Only the text near the view is in the page (like the
// page viewer's pages): a document's whole text laid out at once took most of
// a minute on a phone, and again whenever it was turned.
import { textOf, type Flow } from "./model";
import { blockEl, loadPictures, type Pictures } from "./render";
import { hitRect, paintHits, type FlowHit } from "./find";

export interface ReflowCallbacks {
  /** The displayed page (original page order after edits) now at the top. */
  onPage(view: number): void;
  /** Book mode: page numbers changed. */
  onInfo?(info: { page: number; total: number; approx: boolean }): void;
}

export interface ReflowView {
  readonly el: HTMLElement;
  readonly flow: Flow;
  /** Find: marks all matches, the active one differently. */
  setHits(hits: FlowHit[], active: FlowHit | null): void;
  /** Brings a match into view. */
  showHit(h: FlowHit): void;
  /** Brings character `off` of block `b` into view (read aloud), leaving the find marks alone. */
  reveal(b: number, off: number): void;
  /** Block at the top of the view (to keep the place when switching modes). */
  currentBlock(): number;
  goToBlock(b: number): void;
  goToPage(view: number): void;
  next(): void;
  prev(): void;
  /** Typography changed (CSS variables already applied to `el`). */
  restyle(): void;
  focus(): void;
  destroy(): void;
}

/** A part of a block shown as one element: characters [from, to) (Infinity: to its end). */
interface Piece { b: number; from: number; to: number }

/**
 * A run of pieces shown or dropped together. Only the chunks near the view hold their text; the
 * rest are empty boxes of their measured (or estimated) height.
 */
interface Chunk {
  pieces: Piece[];
  chars: number;
  el: HTMLElement;
  mounted: boolean;
  /** Height when last shown, and the column width and font size it was measured at. */
  h: number;
  w: number;
  fs: number;
}

/** Longest piece of a paragraph shown as one element, and the text in a chunk (characters). */
const PIECE = 2400;
const CHUNK = 6000;
/** A picture counts as this many characters. */
const PICTURE = 900;

export class ReadView implements ReflowView {
  readonly el: HTMLDivElement;
  private col: HTMLDivElement;
  private chunks: Chunk[] = [];
  private raf = 0;
  private lastPage = -1;
  private hits: FlowHit[] = [];
  private active: FlowHit | null = null;
  private ro: ResizeObserver;
  private width = 0;
  private fontSize = 16;
  private lineHeight = 24;

  constructor(host: HTMLElement, readonly flow: Flow, private pics: Pictures, private cb: ReflowCallbacks, start: number) {
    this.el = document.createElement("div");
    this.el.className = "rf rf-read";
    this.el.tabIndex = 0;
    this.col = document.createElement("div");
    this.col.className = "rf-col";
    this.el.append(this.col);
    host.append(this.el);
    this.build();
    this.measure();
    for (const c of this.chunks) c.el.style.height = `${this.estimate(c)}px`;
    this.el.addEventListener("scroll", () => {
      cancelAnimationFrame(this.raf);
      this.raf = requestAnimationFrame(() => {
        this.update();
        this.track();
      });
    }, { passive: true });
    // A new width (window resized, phone turned) changes every height: keep the place.
    this.ro = new ResizeObserver(() => {
      if (this.col.clientWidth !== this.width) this.relayout();
    });
    this.ro.observe(this.el);
    this.goToBlock(start);
  }

  private build() {
    const { blocks } = this.flow;
    let cur: Chunk | null = null;
    const add = (p: Piece, chars: number) => {
      if (!cur || cur.chars >= CHUNK) {
        const el = document.createElement("section");
        el.className = "rf-sec";
        cur = { pieces: [], chars: 0, el, mounted: false, h: 0, w: 0, fs: 0 };
        this.chunks.push(cur);
      }
      // A chunk starting inside a paragraph sits right under the part before it.
      if (!cur.pieces.length && p.from > 0) cur.el.classList.add("cont");
      cur.pieces.push(p);
      cur.chars += chars;
    };
    blocks.forEach((blk, i) => {
      if (blk.k === "img") return add({ b: i, from: 0, to: Infinity }, PICTURE);
      const text = textOf(blk);
      // Very long paragraphs (a whole document without breaks) go in pieces, cut after a sentence.
      let from = 0;
      while (text.length - from > PIECE) {
        let to = text.lastIndexOf(". ", from + PIECE) + 2;
        if (to <= from + PIECE / 2) to = text.lastIndexOf(" ", from + PIECE) + 1;
        if (to <= from + PIECE / 2) to = from + PIECE;
        add({ b: i, from, to }, to - from);
        from = to;
      }
      add({ b: i, from, to: Infinity }, text.length - from);
    });
    const frag = document.createDocumentFragment();
    for (const c of this.chunks) frag.append(c.el);
    if (!this.chunks.length) {
      const empty = document.createElement("p");
      empty.className = "rf-empty";
      empty.textContent = "This document has no text to show here.";
      frag.append(empty);
    }
    this.col.append(frag);
  }

  /** Column width and type size, for estimating the height of text not shown. */
  private measure() {
    this.width = this.col.clientWidth;
    const cs = getComputedStyle(this.col);
    this.fontSize = parseFloat(cs.fontSize) || 16;
    this.lineHeight = parseFloat(cs.lineHeight) || this.fontSize * 1.5;
  }

  private estimate(c: Chunk): number {
    if (c.h && c.fs === this.fontSize) return c.w === this.width ? c.h : (c.h * c.w) / Math.max(1, this.width);
    const textW = Math.max(100, this.width - 2 * this.fontSize);
    const perLine = textW / (this.fontSize * 0.5);
    return Math.ceil(c.chars / perLine) * this.lineHeight + c.pieces.length * this.fontSize * 0.65;
  }

  private mount(c: Chunk) {
    if (c.mounted) return;
    const frag = document.createDocumentFragment();
    for (const p of c.pieces) frag.append(blockEl(this.flow.blocks[p.b], p.b, this.pics, p.from, p.to));
    c.el.replaceChildren(frag);
    c.el.style.height = "";
    c.mounted = true;
    loadPictures(c.el, this.pics);
  }

  private unmount(c: Chunk) {
    if (!c.mounted) return;
    c.h = c.el.offsetHeight;
    c.w = this.width;
    c.fs = this.fontSize;
    c.el.style.height = `${c.h}px`;
    c.el.replaceChildren();
    c.mounted = false;
  }

  /** Shows the text from 1½ screens above the view to 2½ below, and drops the rest. */
  private update() {
    const top = this.el.scrollTop, vh = this.el.clientHeight;
    const lo = top - vh * 1.5, hi = top + vh * 2.5;
    // Read every position first, then change the page (no layout between each).
    const near = this.chunks.map((c) => {
      const y = c.el.offsetTop;
      return y + c.el.offsetHeight > lo && y < hi;
    });
    let added = false;
    this.chunks.forEach((c, i) => {
      if (near[i]) {
        if (!c.mounted) added = true;
        this.mount(c);
      } else this.unmount(c);
    });
    if (added && this.hits.length) paintHits(this.col, this.hits, this.active);
  }

  /** The chunk showing character `off` of block `b`. */
  private chunkAt(b: number, off = 0): Chunk | null {
    let lo = 0, hi = this.chunks.length - 1, found = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      const p = this.chunks[mid].pieces[0];
      if (p.b < b || (p.b === b && p.from <= off)) {
        found = mid;
        lo = mid + 1;
      } else hi = mid - 1;
    }
    return found >= 0 ? this.chunks[found] : null;
  }

  /** The element showing character `off` of block `b` (its chunk shown first). */
  private elementAt(b: number, off = 0): HTMLElement | null {
    const c = this.chunkAt(b, off);
    if (!c) return null;
    this.mount(c);
    let best: HTMLElement | null = null;
    for (const el of c.el.querySelectorAll<HTMLElement>(`[data-b="${b}"]`)) {
      if (Number(el.dataset.o ?? 0) <= off) best = el;
    }
    return best;
  }

  private relayout() {
    const b = this.currentBlock();
    this.measure();
    for (const c of this.chunks) if (!c.mounted) c.el.style.height = `${this.estimate(c)}px`;
    this.goToBlock(b);
  }

  private blockAtTop(): HTMLElement | null {
    const r = this.el.getBoundingClientRect();
    const x = r.left + r.width / 2;
    for (const dy of [12, 40, 80, 140]) {
      const hit = document.elementFromPoint(x, r.top + dy)?.closest<HTMLElement>("[data-b]");
      if (hit && this.el.contains(hit)) return hit;
    }
    return null;
  }

  private track() {
    const b = this.blockAtTop();
    if (!b) return;
    const p = this.flow.blocks[Number(b.dataset.b)]?.page ?? 0;
    if (p !== this.lastPage) {
      this.lastPage = p;
      this.cb.onPage(p);
    }
  }

  currentBlock() {
    const b = this.blockAtTop();
    return b ? Number(b.dataset.b) : 0;
  }

  goToBlock(b: number) {
    const el = this.elementAt(Math.max(0, b));
    if (!el) {
      this.el.scrollTop = 0;
      this.update();
      return;
    }
    const place = () => (this.el.scrollTop += el.getBoundingClientRect().top - this.el.getBoundingClientRect().top - 24);
    place();
    // Text shown above it can differ from its estimate: put it back in place.
    this.update();
    place();
    this.track();
  }

  goToPage(view: number) {
    const { pageStart, blocks } = this.flow;
    let v = Math.max(0, Math.min(view, pageStart.length - 1));
    while (v < pageStart.length - 1 && pageStart[v] >= (pageStart[v + 1] ?? blocks.length)) v++;
    this.goToBlock(pageStart[v] ?? 0);
  }

  setHits(hits: FlowHit[], active: FlowHit | null) {
    this.hits = hits;
    this.active = active;
    paintHits(this.col, hits, active);
  }

  showHit(h: FlowHit) {
    this.reveal(h.b, h.from);
    paintHits(this.col, this.hits, this.active);
  }

  reveal(b: number, off: number) {
    const h = { b, from: off, to: off + 1 };
    this.elementAt(b, off);
    let r = hitRect(this.col, h);
    if (!r) return;
    const box = this.el.getBoundingClientRect();
    if (r.top < box.top + 40 || r.top > box.bottom - 80) {
      this.el.scrollTop += r.top - box.top - box.height / 3;
      this.update();
      // Text shown above it can differ from its estimate: correct once it's laid out.
      r = hitRect(this.col, h);
      if (r && (r.top < box.top || r.top > box.bottom - 40)) this.el.scrollTop += r.top - box.top - box.height / 3;
    }
  }

  next() {
    this.el.scrollBy({ top: this.el.clientHeight * 0.9, behavior: "smooth" });
  }

  prev() {
    this.el.scrollBy({ top: -this.el.clientHeight * 0.9, behavior: "smooth" });
  }

  restyle() {
    // Keep the place: the block at the top stays at the top. Heights measured at the old size are stale.
    const b = this.currentBlock();
    requestAnimationFrame(() => {
      for (const c of this.chunks) c.h = 0;
      this.measure();
      for (const c of this.chunks) if (!c.mounted) c.el.style.height = `${this.estimate(c)}px`;
      this.goToBlock(b);
    });
  }

  focus() {
    this.el.focus({ preventScroll: true });
  }

  destroy() {
    cancelAnimationFrame(this.raf);
    this.ro.disconnect();
    this.el.remove();
  }
}
