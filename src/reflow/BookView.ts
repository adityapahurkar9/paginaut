// Book mode: the reflowed text laid out as a two-page book filling the
// screen, turned with a 3D page flip (click, keys, wheel, or drag a corner).
// On a screen taller than it is wide (a phone held upright) it's one page,
// turned over at its left edge, and a swipe anywhere turns it.
//
// Pages are made by our own paginator: blocks are poured into an invisible
// page of the real size and style; a paragraph that doesn't fit is split at
// the last word that does. The flow is cut into sections (at headings, or
// every ~20 original pages) that paginate independently, so the section
// you're in is ready in milliseconds and the rest follows in the background.
import { textOf, type Block, type Flow } from "./model";
import { blockEl, loadPictures, type Pictures } from "./render";
import { paintHits, type FlowHit } from "./find";
import type { ReflowCallbacks, ReflowView } from "./ReadView";

interface Pos { b: number; o: number }
interface Section {
  b0: number;
  b1: number;
  chars: number;
  /** Start of each page; null until paginated. */
  pages: Pos[] | null;
  title: string;
}
interface At { s: number; p: number }

const MARGIN = 22; // around the book, px
const TURN_MS = 620;

export class BookView implements ReflowView {
  readonly el: HTMLDivElement;
  private book: HTMLDivElement;
  private slots: [HTMLDivElement, HTMLDivElement];
  private measurePage: HTMLDivElement;
  private measure: HTMLDivElement;
  private sections: Section[] = [];
  private cur: At = { s: 0, p: 0 };
  private anchor: Pos = { b: 0, o: 0 };
  private pageW = 0;
  private pageH = 0;
  private turning: { cancel(): void; finish(): void } | null = null;
  /** One page at a time (narrow screens) rather than a spread. */
  private single = false;
  private bg = 0;
  private ro: ResizeObserver;
  private wheelAcc = 0;
  private wheelLock = 0;
  private destroyed = false;
  /** Pictures shrunk to fit the space left on their page (block → width px). */
  private shrink = new Map<number, number>();
  /** For each block, the latest big heading at or before it (running head of right pages). */
  private headingAt: Int32Array;

  private hits: FlowHit[] = [];
  private active: FlowHit | null = null;

  constructor(host: HTMLElement, readonly flow: Flow, private pics: Pictures, private cb: ReflowCallbacks, start: number, private title: string) {
    this.el = document.createElement("div");
    this.el.className = "rf rf-stage";
    this.el.tabIndex = 0;
    this.book = document.createElement("div");
    this.book.className = "rf-book";
    this.slots = [this.slot("left"), this.slot("right")];
    this.book.append(...this.slots);
    this.measurePage = document.createElement("div");
    this.measurePage.className = "rf-page measure";
    this.measure = document.createElement("div");
    this.measure.className = "rf-content";
    this.measurePage.append(this.measure);
    this.el.append(this.book, this.measurePage);
    host.append(this.el);
    this.makeSections();
    this.headingAt = new Int32Array(flow.blocks.length);
    let h = -1;
    flow.blocks.forEach((b, i) => {
      if (b.k === "h" && b.level <= 2) h = i;
      this.headingAt[i] = h;
    });
    this.anchor = { b: start, o: 0 };
    this.el.addEventListener("wheel", (e) => this.onWheel(e), { passive: false });
    this.el.addEventListener("pointerdown", (e) => this.onPointerDown(e));
    this.ro = new ResizeObserver(() => this.relayout());
    this.ro.observe(this.el);
  }

  private slot(side: "left" | "right") {
    const d = document.createElement("div");
    d.className = `rf-slot ${side}`;
    return d;
  }

  // ------------------------------------------------------------ sections

  private makeSections() {
    const { blocks } = this.flow;
    let b0 = 0, startPage = 0;
    const push = (b1: number) => {
      if (b1 <= b0) return;
      const slice = blocks.slice(b0, b1);
      const h = slice.find((x) => x.k === "h");
      this.sections.push({ b0, b1, pages: null, chars: slice.reduce((a, x) => a + (x.k === "img" ? 900 : x.len), 0), title: h ? textOf(h) : "" });
      b0 = b1;
    };
    for (let i = 1; i < blocks.length; i++) {
      const b = blocks[i];
      const span = b.page - startPage;
      const headingBreak = b.k === "h" && b.level <= 2 && span >= 6;
      const forced = span >= 20 && b.page !== blocks[i - 1].page;
      if (headingBreak || forced) {
        push(i);
        startPage = b.page;
      }
    }
    push(blocks.length);
  }

  private sectionOf(b: number) {
    let lo = 0, hi = this.sections.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (this.sections[mid].b0 <= b) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  }

  // ------------------------------------------------------------ pagination

  private overflows() {
    return this.measure.scrollHeight > this.measure.clientHeight + 0.5;
  }

  private lineHeight() {
    const cs = getComputedStyle(this.measure);
    return parseFloat(cs.lineHeight) || parseFloat(cs.fontSize) * 1.5;
  }

  /** Paginates one section, yielding after every page so it can run in slices. */
  private *paginate(sec: Section): Generator<void, Pos[]> {
    const { blocks } = this.flow;
    const pages: Pos[] = [];
    const lh = this.lineHeight();
    let pos: Pos = { b: sec.b0, o: 0 };
    while (pos.b < sec.b1) {
      const start = pos;
      this.measure.replaceChildren();
      let placed = 0;
      for (;;) {
        if (pos.b >= sec.b1) break;
        const block = blocks[pos.b];
        const el = blockEl(block, pos.b, this.pics, pos.o);
        this.measure.append(el);
        if (!this.overflows()) {
          placed++;
          pos = { b: pos.b + 1, o: 0 };
          continue;
        }
        el.remove();
        if (block.k === "img" && placed > 0 && this.fitPicture(el, pos.b)) {
          placed++;
          pos = { b: pos.b + 1, o: 0 };
          break;
        }
        if (block.k === "p" || block.k === "li") {
          const k = this.fit(block, pos.b, pos.o, lh, placed === 0);
          if (k > pos.o) {
            this.measure.append(blockEl(block, pos.b, this.pics, pos.o, k));
            placed++;
            pos = { b: pos.b, o: k };
          }
        }
        if (placed === 0) {
          // Nothing fits on an empty page (a huge picture or heading): place it anyway.
          this.measure.append(el);
          placed++;
          pos = { b: pos.b + 1, o: 0 };
        }
        break;
      }
      // A heading must not end a page: move it to the next one.
      const last = this.measure.lastElementChild as HTMLElement | null;
      if (placed > 1 && last && /^H\d$/.test(last.tagName) && pos.o === 0 && pos.b < sec.b1) pos = { b: Number(last.dataset.b), o: 0 };
      pages.push(start);
      yield;
    }
    if (!pages.length) pages.push({ b: sec.b0, o: 0 });
    return pages;
  }

  /**
   * A picture that doesn't fit at the bottom of a page: shrink it into the
   * space instead of leaving the space empty, as long as it stays readable
   * (tables and drawings have text in them, so they shrink much less).
   */
  private fitPicture(el: HTMLElement, index: number): boolean {
    const b = this.flow.blocks[index];
    const minScale = b.k === "img" && (b.kind === "table" || b.kind === "figure") ? 0.8 : 0.45;
    const last = this.measure.lastElementChild;
    const used = last ? last.getBoundingClientRect().bottom - this.measure.getBoundingClientRect().top : 0;
    this.measure.append(el);
    const natural = el.getBoundingClientRect().height;
    const gap = parseFloat(getComputedStyle(el).marginTop) || 0;
    const room = this.measure.clientHeight - used - gap - 2;
    if (room < 140 || room < natural * minScale) {
      el.remove();
      return false;
    }
    const ar = parseFloat(el.style.getPropertyValue("--ar")) || 1;
    el.style.width = `${Math.floor(room * ar)}px`;
    if (this.overflows()) {
      el.remove();
      return false;
    }
    this.shrink.set(index, Math.floor(room * ar));
    return true;
  }

  /** Largest word boundary k such that characters [from, k) of the paragraph fit. */
  private fit(block: Extract<Block, { k: "p" | "li" }>, index: number, from: number, lh: number, emptyPage: boolean): number {
    const text = textOf(block);
    const cuts: number[] = [];
    for (let i = from + 1; i < text.length; i++) if (text[i - 1] === " ") cuts.push(i);
    let lo = 0, hi = cuts.length - 1, best = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      const el = blockEl(block, index, this.pics, from, cuts[mid]);
      this.measure.append(el);
      const ok = !this.overflows();
      el.remove();
      if (ok) {
        best = mid;
        lo = mid + 1;
      } else hi = mid - 1;
    }
    if (best < 0) return from;
    const k = cuts[best];
    if (emptyPage) return k;
    // Don't leave a single line at the bottom of a page.
    const el = blockEl(block, index, this.pics, from, k);
    this.measure.append(el);
    const lines = el.getBoundingClientRect().height / lh;
    el.remove();
    return lines < 1.8 ? from : k;
  }

  private ensure(s: number): Pos[] {
    const sec = this.sections[s];
    if (sec.pages) return sec.pages;
    const gen = this.paginate(sec);
    let r = gen.next();
    while (!r.done) r = gen.next();
    sec.pages = r.value;
    return sec.pages;
  }

  /** Paginates the remaining sections a few milliseconds at a time (for page numbers). */
  private background() {
    clearTimeout(this.bg);
    let gen: Generator<void, Pos[]> | null = null;
    let s = -1;
    const run = () => {
      if (this.destroyed) return;
      if (this.turning) {
        this.bg = window.setTimeout(run, 120);
        return;
      }
      const t0 = performance.now();
      while (performance.now() - t0 < 8) {
        if (!gen) {
          s = this.sections.findIndex((x) => !x.pages);
          if (s < 0) {
            this.info();
            this.refreshFolios();
            return;
          }
          gen = this.paginate(this.sections[s]);
        }
        const r = gen.next();
        if (r.done) {
          this.sections[s].pages = r.value;
          gen = null;
        }
      }
      this.info();
      this.bg = window.setTimeout(run, 0);
    };
    this.bg = window.setTimeout(run, 60);
  }

  // ------------------------------------------------------------ positions

  private count(s: number) {
    return this.ensure(s).length;
  }

  private nextAt(a: At): At | null {
    if (a.p + 1 < this.count(a.s)) return { s: a.s, p: a.p + 1 };
    return a.s + 1 < this.sections.length ? { s: a.s + 1, p: 0 } : null;
  }

  private prevAt(a: At): At | null {
    if (a.p > 0) return { s: a.s, p: a.p - 1 };
    return a.s > 0 ? { s: a.s - 1, p: this.count(a.s - 1) - 1 } : null;
  }

  private startOf(a: At): Pos {
    return this.ensure(a.s)[a.p];
  }

  /** Page (in its section) that contains block position `pos`. */
  private atOf(pos: Pos): At {
    const s = this.sectionOf(pos.b);
    const pages = this.ensure(s);
    let p = 0;
    while (p + 1 < pages.length && (pages[p + 1].b < pos.b || (pages[p + 1].b === pos.b && pages[p + 1].o <= pos.o))) p++;
    return { s, p };
  }

  /** Book page number of `a` (1-based) and total; exact once every section is paginated. */
  private numbers(a: At) {
    let before = 0, total = 0, approx = false;
    const done = this.sections.filter((x) => x.pages);
    const perPage = done.reduce((n, x) => n + x.chars, 0) / Math.max(1, done.reduce((n, x) => n + x.pages!.length, 0)) || 1500;
    this.sections.forEach((x, i) => {
      const n = x.pages ? x.pages.length : Math.max(1, Math.round(x.chars / perPage));
      if (!x.pages) approx = true;
      if (i < a.s) before += n;
      total += n;
    });
    return { page: before + a.p + 1, total, approx };
  }

  /** Page numbers on screen were estimates until every section was paginated. */
  private refreshFolios() {
    for (const page of this.book.querySelectorAll<HTMLElement>(".rf-page[data-s]")) {
      const foot = page.querySelector(".rf-foot");
      if (foot) foot.textContent = String(this.numbers({ s: Number(page.dataset.s), p: Number(page.dataset.p) }).page);
    }
  }

  private info() {
    this.cb.onInfo?.(this.numbers(this.cur));
  }

  // ------------------------------------------------------------ drawing

  private pageEl(a: At | null, side: "left" | "right" | "single"): HTMLDivElement {
    const page = document.createElement("div");
    page.className = `rf-page ${side}`;
    const content = document.createElement("div");
    content.className = "rf-content";
    page.append(content);
    if (!a) return page;
    const sec = this.sections[a.s];
    const pages = this.ensure(a.s);
    const start = pages[a.p];
    const end: Pos = pages[a.p + 1] ?? { b: sec.b1, o: 0 };
    for (let i = start.b; i < sec.b1; i++) {
      if (i === end.b && end.o === 0) break;
      const from = i === start.b ? start.o : 0;
      const to = i === end.b ? end.o : Infinity;
      const el = blockEl(this.flow.blocks[i], i, this.pics, from, to);
      const w = this.shrink.get(i);
      if (w) el.style.width = `${w}px`;
      content.append(el);
      if (i === end.b) break;
    }
    const head = document.createElement("div");
    head.className = "rf-head";
    const h = this.headingAt[start.b];
    head.textContent = side === "left" ? this.title : h >= 0 ? textOf(this.flow.blocks[h]) : sec.title;
    const foot = document.createElement("div");
    foot.className = "rf-foot";
    foot.textContent = String(this.numbers(a).page);
    page.dataset.s = String(a.s);
    page.dataset.p = String(a.p);
    page.append(head, foot);
    return page;
  }

  private show() {
    if (this.single) {
      this.slots[0].replaceChildren(this.pageEl(this.cur, "single"));
      this.slots[1].replaceChildren();
    } else {
      const right = this.nextAt(this.cur);
      this.slots[0].replaceChildren(this.pageEl(this.cur, "left"));
      this.slots[1].replaceChildren(this.pageEl(right, "right"));
    }
    for (const s of this.slots) loadPictures(s, this.pics);
    if (this.hits.length) paintHits(this.book, this.hits, this.active);
    this.anchor = this.startOf(this.cur);
    const b = this.flow.blocks[this.anchor.b];
    if (b) this.cb.onPage(b.page);
    this.info();
  }

  /** Size the book to the screen and repaginate, keeping the current place. */
  private relayout() {
    const W = this.el.clientWidth, H = this.el.clientHeight;
    if (!W || !H) return;
    const pageH = Math.floor(H - MARGIN * 2);
    const single = W < H * 0.9;
    const pageW = Math.floor(Math.min((W - MARGIN * 2) / (single ? 1 : 2), pageH * (single ? 0.8 : 1.05)));
    if (pageW === this.pageW && pageH === this.pageH && single === this.single) return;
    this.single = single;
    this.book.classList.toggle("single", single);
    this.pageW = pageW;
    this.pageH = pageH;
    this.el.style.setProperty("--page-w", `${pageW}px`);
    this.el.style.setProperty("--page-h", `${pageH}px`);
    this.repaginate();
  }

  private repaginate() {
    this.turning?.cancel();
    for (const s of this.sections) s.pages = null;
    this.shrink.clear();
    // --avail-h lets pictures shrink to fit a page.
    this.el.style.setProperty("--avail-h", `${this.measure.clientHeight}px`);
    this.cur = this.atOf(this.anchor);
    this.show();
    this.background();
  }

  // ------------------------------------------------------------ turning

  /**
   * Starts turning a leaf: forward lifts the right page over the spine,
   * backward the left. Returns null at either end of the book.
   */
  private beginTurn(dir: 1 | -1) {
    const cur = this.cur;
    const fwd = dir > 0;
    const right = this.single ? null : this.nextAt(cur);
    let target: At | null;
    if (this.single) target = fwd ? this.nextAt(cur) : this.prevAt(cur);
    else if (fwd) target = right && this.nextAt(right);
    else {
      const p = this.prevAt(cur);
      target = p && (this.prevAt(p) ?? p);
    }
    if (!target) return null;
    const tRight = this.single ? null : this.nextAt(target);
    const cast = document.createElement("div");
    cast.className = "rf-cast";
    const leaf = document.createElement("div");
    let front: HTMLDivElement, back: HTMLDivElement, reveal: HTMLDivElement;
    if (this.single) {
      // One page: forward, it lifts off to the left showing the next; backward, the previous comes back over it.
      reveal = this.pageEl(fwd ? target : cur, "single");
      this.slots[0].replaceChildren(reveal);
      leaf.className = "rf-leaf single";
      front = this.pageEl(fwd ? cur : target, "single");
      back = this.pageEl(null, "single");
    } else {
      // Under the leaf: the page that stays, and the one being revealed.
      const stay = fwd ? this.pageEl(cur, "left") : this.pageEl(right, "right");
      reveal = fwd ? this.pageEl(tRight, "right") : this.pageEl(target, "left");
      this.slots[fwd ? 0 : 1].replaceChildren(stay);
      this.slots[fwd ? 1 : 0].replaceChildren(reveal);
      leaf.className = `rf-leaf ${fwd ? "fwd" : "back"}`;
      front = fwd ? this.pageEl(right, "right") : this.pageEl(cur, "left");
      back = fwd ? this.pageEl(target, "left") : this.pageEl(tRight, "right");
    }
    reveal.append(cast);
    front.classList.add("face", "front");
    back.classList.add("face", "back");
    const shadeF = document.createElement("div"), shadeB = document.createElement("div");
    shadeF.className = shadeB.className = "rf-shade";
    front.append(shadeF);
    back.append(shadeB);
    leaf.append(front, back);
    this.book.append(leaf);
    for (const el of [reveal, front, back]) loadPictures(el, this.pics);

    let progress = 0, raf = 0;
    const set = (p: number) => {
      progress = Math.max(0, Math.min(1, p));
      // A single page turned back starts lying over to the left and comes down onto the book.
      const shown = this.single && !fwd ? 1 - progress : progress;
      const deg = this.single ? -180 * shown : (fwd ? -180 : 180) * progress;
      leaf.style.transform = `rotateY(${deg}deg)`;
      shadeF.style.opacity = String(0.5 * shown);
      shadeB.style.opacity = String(0.5 * (1 - shown));
      cast.style.opacity = String(0.55 * (1 - shown));
    };
    const cleanup = () => {
      cancelAnimationFrame(raf);
      leaf.remove();
      this.turning = null;
    };
    const commit = () => {
      cleanup();
      this.cur = target!;
      this.show();
    };
    const revert = () => {
      cleanup();
      this.show();
    };
    const animate = (to: 0 | 1) => {
      cancelAnimationFrame(raf);
      const from = progress, t0 = performance.now();
      const dur = TURN_MS * Math.max(0.25, Math.abs(to - from));
      const ease = (t: number) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);
      const tick = () => {
        const t = Math.min(1, (performance.now() - t0) / dur);
        set(from + (to - from) * ease(t));
        if (t < 1) raf = requestAnimationFrame(tick);
        else if (to === 1) commit();
        else revert();
      };
      raf = requestAnimationFrame(tick);
    };
    set(0);
    const ctl = { set, animate, cancel: revert, finish: commit };
    this.turning = ctl;
    return ctl;
  }

  private turn(dir: 1 | -1) {
    this.turning?.finish();
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
      const t = this.single
        ? dir > 0 ? this.nextAt(this.cur) : this.prevAt(this.cur)
        : dir > 0 ? this.nextAt(this.cur) && this.nextAt(this.nextAt(this.cur)!) : this.prevAt(this.cur) && (this.prevAt(this.prevAt(this.cur)!) ?? this.prevAt(this.cur));
      if (t) {
        this.cur = t;
        this.show();
      }
      return;
    }
    this.beginTurn(dir)?.animate(1);
  }

  next() {
    this.turn(1);
  }

  prev() {
    this.turn(-1);
  }

  private onWheel(e: WheelEvent) {
    if (e.ctrlKey) return; // font size, handled by the app
    e.preventDefault();
    const now = performance.now();
    if (now < this.wheelLock) {
      this.wheelLock = now + 180; // one turn per gesture
      return;
    }
    this.wheelAcc += Math.abs(e.deltaY) > Math.abs(e.deltaX) ? e.deltaY : e.deltaX;
    if (Math.abs(this.wheelAcc) > 60) {
      this.turn(this.wheelAcc > 0 ? 1 : -1);
      this.wheelAcc = 0;
      this.wheelLock = now + 180;
    }
  }

  /** Drag from the outer edge of a page to turn it by hand; click the outer part to turn. */
  private onPointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    const r = this.book.getBoundingClientRect();
    const x = e.clientX - r.left;
    const w = r.width;
    const edge = w * 0.09, zone = w * 0.14;
    const dir: 1 | -1 | 0 = x > w - zone ? 1 : x < zone ? -1 : 0;
    if (!dir) {
      if (e.pointerType === "touch") this.swipe(e, w);
      return;
    }
    const onEdge = dir > 0 ? x > w - edge : x < edge;
    e.preventDefault();
    this.focus();
    this.turning?.finish();
    const startX = e.clientX;
    let ctl: ReturnType<BookView["beginTurn"]> | null = null;
    let moved = false;
    const move = (ev: PointerEvent) => {
      const dx = (ev.clientX - startX) * -dir;
      if (!moved && Math.abs(dx) < 6) return;
      if (!onEdge && !moved) return;
      moved = true;
      if (!ctl) ctl = this.beginTurn(dir);
      ctl?.set(dx / (w * 0.75));
    };
    const up = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      if (!moved) return this.turn(dir); // a click
      if (!ctl) return;
      const dx = (ev.clientX - startX) * -dir;
      ctl.animate(dx / (w * 0.75) > 0.3 ? 1 : 0);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  /** Touch: a sideways swipe anywhere turns the page, following the finger. */
  private swipe(e: PointerEvent, w: number) {
    const startX = e.clientX, startY = e.clientY;
    let ctl: ReturnType<BookView["beginTurn"]> | null = null;
    let dir: 1 | -1 = 1;
    let gone = false;
    const move = (ev: PointerEvent) => {
      const dx = ev.clientX - startX, dy = ev.clientY - startY;
      if (!ctl) {
        if (gone || Math.abs(dx) < 12) return;
        // Mostly up or down: not a page turn.
        if (Math.abs(dy) > Math.abs(dx)) {
          gone = true;
          return;
        }
        dir = dx < 0 ? 1 : -1;
        this.turning?.finish();
        ctl = this.beginTurn(dir);
        if (!ctl) gone = true;
      }
      ctl?.set((dx * -dir) / (w * 0.75));
    };
    const up = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      if (!ctl) return;
      const dx = (ev.clientX - startX) * -dir;
      ctl.animate(ev.type === "pointerup" && dx / (w * 0.75) > 0.15 ? 1 : 0);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  }

  // ------------------------------------------------------------ ReflowView

  setHits(hits: FlowHit[], active: FlowHit | null) {
    this.hits = hits;
    this.active = active;
    paintHits(this.book, hits, active);
  }

  showHit(h: FlowHit) {
    this.active = h;
    this.reveal(h.b, h.from);
    paintHits(this.book, this.hits, this.active);
  }

  reveal(b: number, off: number) {
    this.turning?.finish();
    const at = this.atOf({ b, o: off });
    const right = this.single ? null : this.nextAt(this.cur);
    const same = (a: At | null) => !!a && a.s === at.s && a.p === at.p;
    if (!same(this.cur) && !same(right)) {
      this.cur = at;
      this.show();
    }
  }

  currentBlock() {
    return this.anchor.b;
  }

  goToBlock(b: number) {
    this.turning?.cancel();
    this.anchor = { b: Math.max(0, b), o: 0 };
    if (!this.pageW) return; // laid out on first resize
    this.cur = this.atOf(this.anchor);
    this.show();
  }

  goToPage(view: number) {
    const { pageStart, blocks } = this.flow;
    const v = Math.max(0, Math.min(view, pageStart.length - 1));
    this.goToBlock(Math.min(pageStart[v] ?? 0, blocks.length - 1));
  }

  restyle() {
    this.pageW = 0;
    this.relayout();
  }

  focus() {
    this.el.focus({ preventScroll: true });
  }

  destroy() {
    this.destroyed = true;
    clearTimeout(this.bg);
    this.turning?.cancel();
    this.ro.disconnect();
    this.el.remove();
  }
}
