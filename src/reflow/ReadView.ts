// Read mode: the document's text reflowed into one scrolling column at a
// comfortable width. Sections per original page keep layout cheap
// (content-visibility) and map positions back to pages.
import type { Flow } from "./model";
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

export class ReadView implements ReflowView {
  readonly el: HTMLDivElement;
  private col: HTMLDivElement;
  private sections: HTMLElement[] = [];
  private io: IntersectionObserver;
  private raf = 0;
  private lastPage = -1;
  private hits: FlowHit[] = [];
  private active: FlowHit | null = null;

  constructor(host: HTMLElement, readonly flow: Flow, private pics: Pictures, private cb: ReflowCallbacks, start: number) {
    this.el = document.createElement("div");
    this.el.className = "rf rf-read";
    this.el.tabIndex = 0;
    this.col = document.createElement("div");
    this.col.className = "rf-col";
    this.el.append(this.col);
    host.append(this.el);
    this.io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) if (e.isIntersecting) loadPictures(e.target as HTMLElement, this.pics);
      },
      { root: this.el, rootMargin: "1500px 0px" },
    );
    this.build();
    this.el.addEventListener("scroll", () => {
      cancelAnimationFrame(this.raf);
      this.raf = requestAnimationFrame(() => this.track());
    }, { passive: true });
    this.goToBlock(start);
  }

  private build() {
    const { blocks, pageStart } = this.flow;
    const frag = document.createDocumentFragment();
    for (let p = 0; p < pageStart.length; p++) {
      const a = pageStart[p], b = p + 1 < pageStart.length ? pageStart[p + 1] : blocks.length;
      if (a >= b) continue;
      const sec = document.createElement("section");
      sec.className = "rf-sec";
      sec.dataset.p = String(p);
      for (let i = a; i < b; i++) sec.append(blockEl(blocks[i], i, this.pics));
      frag.append(sec);
      this.sections.push(sec);
      if (sec.querySelector("figure")) this.io.observe(sec);
    }
    if (!this.sections.length) {
      const empty = document.createElement("p");
      empty.className = "rf-empty";
      empty.textContent = "This document has no text to show here.";
      frag.append(empty);
    }
    this.col.append(frag);
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
    const el = this.col.querySelector<HTMLElement>(`[data-b="${Math.max(0, b)}"]`);
    if (el) this.el.scrollTop += el.getBoundingClientRect().top - this.el.getBoundingClientRect().top - 24;
    else this.el.scrollTop = 0;
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
    let r = hitRect(this.col, h);
    if (!r) return;
    const box = this.el.getBoundingClientRect();
    if (r.top < box.top + 40 || r.top > box.bottom - 80) {
      this.el.scrollTop += r.top - box.top - box.height / 3;
      // Sections outside the view were only estimated; correct once they've laid out.
      requestAnimationFrame(() => {
        r = hitRect(this.col, h);
        if (r && (r.top < box.top || r.top > box.bottom - 40)) this.el.scrollTop += r.top - box.top - box.height / 3;
      });
    }
    paintHits(this.col, this.hits, this.active);
  }

  next() {
    this.el.scrollBy({ top: this.el.clientHeight * 0.9, behavior: "smooth" });
  }

  prev() {
    this.el.scrollBy({ top: -this.el.clientHeight * 0.9, behavior: "smooth" });
  }

  restyle() {
    // Keep the place: the block at the top stays at the top.
    const b = this.currentBlock();
    requestAnimationFrame(() => this.goToBlock(b));
  }

  focus() {
    this.el.focus({ preventScroll: true });
  }

  destroy() {
    cancelAnimationFrame(this.raf);
    this.io.disconnect();
    this.el.remove();
  }
}
