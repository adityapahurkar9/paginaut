// The continuous-scroll page viewer. Plain DOM, no framework: only pages
// near the viewport exist in the DOM, and all pixel work happens in the
// native engine.
//
// Rendering strategy per visible page:
//   1. "lo"  – a small, fast render (also used by thumbnails) so a page is
//              never blank, even while flinging through a 1,000-page scan;
//   2. "hi"  – the full page at screen resolution;
//   3. tiles – above ~12 MP a page is rendered as 1024px tiles, only where
//              it's visible, so deep zoom stays fast.
// Bitmaps stretch while zooming and are replaced when sharper ones arrive.
//
// The viewer shows a list of PageSpecs: which source page, with what extra
// rotation. Unsaved page edits (reorder, delete, rotate) are just a new list.

import { api, type DocInfo, type LinkItem, type Quad } from "../api";
import { renderer, renderKey, type Bitmap, type Want } from "./RenderManager";
import { openUrl } from "@tauri-apps/plugin-opener";

const PT_TO_CSS = 96 / 72;
const GAP = 14;
const PAD_Y = 20;
const PAD_X = 16;
export const LO_SCALE = 0.4;
const MAX_FULL_PX = 12_000_000;
const TILE = 1024;
export const MIN_ZOOM = 0.1;
export const MAX_ZOOM = 32;

export type FitMode = "none" | "width" | "page";
export type LayoutMode = "single" | "spread" | "spread-cover";
export type PageAction = "rotate-left" | "rotate-right" | "delete";

/** One page as displayed: source page index, extra rotation, displayed size in points. */
export interface PageSpec {
  src: number;
  rot: number;
  w: number;
  h: number;
}

export interface ViewState {
  page: number;
  /** Fraction of the page scrolled past the top of the viewport. */
  frac: number;
  zoom: number;
  fit: FitMode;
}

/** A page as a layer sees it: display index, file page, unrotated size in points. */
export interface LayerPage {
  view: number;
  src: number;
  w: number;
  h: number;
}

/**
 * Something drawn over the pages (form fields, annotations, the layout
 * editor). Its element is laid out in unrotated page points (x right, y down
 * from the page's top-left); the viewer scales and rotates it with the page.
 */
export interface PageLayer {
  /** Builds the layer's element for a page that just came into view (or null for nothing). */
  mount(page: LayerPage): HTMLElement | null;
  /** The page left view; the element is removed by the viewer. */
  unmount?(page: LayerPage, el: HTMLElement): void;
  /** Stacking order among layers (higher is on top). */
  z?: number;
}

export interface ViewerCallbacks {
  onPage(page: number): void;
  onZoom(zoom: number, fit: FitMode): void;
  onSelection(has: boolean): void;
  onPageAction(page: number, action: PageAction): void;
  /**
   * A drag-selection finished (mouse released): file page, the selected text's
   * quads (unrotated page points; empty on a page without text), and the drag's
   * corners (for area highlights on scans).
   */
  onSelectionEnd?(src: number, quads: Quad[], a: [number, number], b: [number, number]): void;
  /**
   * Whether a drag shows the box it covers as it goes (Redact marks a box on scans, or with areas on), told whether
   * the drag has selected text so far.
   */
  dragBox?(hasText: boolean): boolean;
  /** The view scrolled (by the user or the program). */
  onScroll?(): void;
}

interface PageView {
  index: number;
  spec: PageSpec;
  el: HTMLDivElement;
  lo: HTMLCanvasElement;
  hi: HTMLCanvasElement;
  tiles: HTMLDivElement;
  marks: HTMLDivElement;
  links: HTMLDivElement;
  loKey?: string;
  hiKey?: string;
  hiScale: number;
  tileKeys: Map<string, HTMLCanvasElement>;
  tileScale: number;
  linksLoaded: boolean;
  /** Last bitmaps drawn, kept while mounted so night-mode photo patches can be redrawn. */
  loImage?: Bitmap;
  hiImage?: Bitmap;
  photos?: HTMLDivElement;
  /** Container for layers, in unrotated page points. */
  layerBox: HTMLDivElement;
  layerEls: Map<PageLayer, HTMLElement>;
}

/** Text selection on one displayed page; points are in unrotated page space. */
interface Selection {
  view: number;
  a: [number, number];
  b: [number, number];
  /** 0: characters between a and b, 1: words, 2: lines, 3: the word nearest a (touch), 4: its paragraph, 5: the page. */
  mode: 0 | 1 | 2 | 3 | 4 | 5;
  /**
   * A word, paragraph or page selection (modes 3–5) as the engine's character range [from, to):
   * drawn and copied from it exactly. a and b still mark its ends, for the handles to move.
   */
  range?: [number, number];
}

// ------------------------------------------------------------ rotation math
// Mirrors MuPDF's page matrix (rotate, then shift into positive space), so
// display coordinates line up exactly with rendered pixels.

/** Unrotated size of a displayed page. */
function origSize(s: PageSpec): [number, number] {
  return s.rot % 180 ? [s.h, s.w] : [s.w, s.h];
}

function toDisplay(s: PageSpec, x: number, y: number): [number, number] {
  const [W, H] = origSize(s);
  switch (((s.rot % 360) + 360) % 360) {
    case 90: return [H - y, x];
    case 180: return [W - x, H - y];
    case 270: return [y, W - x];
    default: return [x, y];
  }
}

function toOriginal(s: PageSpec, X: number, Y: number): [number, number] {
  const [W, H] = origSize(s);
  switch (((s.rot % 360) + 360) % 360) {
    case 90: return [Y, H - X];
    case 180: return [W - X, H - Y];
    case 270: return [W - Y, X];
    default: return [X, Y];
  }
}

/** Displayed bounding box [x0, y0, x1, y1] of a quad given in original page space. */
function quadBox(s: PageSpec, q: ArrayLike<number>): [number, number, number, number] {
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (let k = 0; k < 8; k += 2) {
    const [x, y] = toDisplay(s, q[k], q[k + 1]);
    x0 = Math.min(x0, x); y0 = Math.min(y0, y); x1 = Math.max(x1, x); y1 = Math.max(y1, y);
  }
  return [x0, y0, x1, y1];
}

// ------------------------------------------------------------ canvases

/** Frees a canvas's pixel memory once something sharper covers it. */
function releaseCanvas(c: HTMLCanvasElement) {
  c.width = c.height = 1;
  c.style.visibility = "hidden";
}

/**
 * Removes canvases (or a container of them) for good, freeing their pixels now.
 * A removed canvas otherwise holds its memory, and its copy in the GPU process,
 * until garbage collection, which can be hundreds of MB after a long scroll.
 */
function discard(...els: (Element | undefined)[]) {
  for (const el of els) {
    if (!el) continue;
    if (el instanceof HTMLCanvasElement) releaseCanvas(el);
    else for (const c of el.querySelectorAll("canvas")) releaseCanvas(c);
    el.remove();
  }
}

/** Copies pixels into a CPU-backed canvas (see fetchRender for why not ImageBitmap). */
export function paint(canvas: HTMLCanvasElement, b: Bitmap) {
  canvas.getContext("2d", { alpha: false, willReadFrequently: true })!.putImageData(b.image, 0, 0);
}

function drawBitmap(canvas: HTMLCanvasElement, b: Bitmap) {
  if (canvas.width !== b.width || canvas.height !== b.height) {
    canvas.width = b.width;
    canvas.height = b.height;
  }
  paint(canvas, b);
  canvas.style.visibility = "visible";
}

const ICONS: Record<PageAction, string> = {
  "rotate-left": '<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5"/>',
  "rotate-right": '<path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/>',
  delete: '<path d="M4 7h16"/><path d="M10 11v6M14 11v6"/><path d="M6 7l1 13h10l1-13"/><path d="M9 7V4h6v3"/>',
};
const TITLES: Record<PageAction, string> = {
  "rotate-left": "Rotate left (Shift+R)",
  "rotate-right": "Rotate right (R)",
  delete: "Delete page (Del)",
};

export class Viewer {
  readonly el: HTMLDivElement;
  private sizer: HTMLDivElement;
  private info: DocInfo;
  private pages: PageSpec[];
  private srcToView = new Map<number, number>();
  private layoutMode: LayoutMode = "single";
  /** One page (or spread) per screen: each row is at least the view's height, its pages centred in it. */
  private slots = false;
  /**
   * With slots: the page being read, kept rather than worked out from the scroll position, which
   * means nothing while the view is resized through in-between sizes (a tablet turning).
   */
  private here = -1;
  private layoutAt = 0;
  // Per page: position in CSS px at the current zoom. Rows hold 1 or 2 pages.
  private px = new Float64Array(0);
  private py = new Float64Array(0);
  private rowOf = new Int32Array(0);
  private rowY = new Float64Array(0);
  private rowFirst = new Int32Array(0);
  private rowW = new Float64Array(0);
  private contentW = 0;
  private contentH = 0;
  private zoom = 1;
  private fit: FitMode = "width";
  private mounted = new Map<number, PageView>();
  private group: string;
  private raf = 0;
  private rafImmediate = false;
  private renderTimer = 0;
  private lastPage = -1;
  private ro: ResizeObserver;
  private tool: "select" | "hand" = "select";
  private selection: Selection | null = null;
  private selQuads: Quad[] = [];
  private search: Map<number, number[][]> = new Map();
  private activeHit: { page: number; index: number } | null = null;
  private links = new Map<number, LinkItem[]>();
  private destroyed = false;
  private sharpWants: Want[] = [];
  private sharpScale = 0;
  /**
   * A view to show once the viewer has a size: Linux's WebKitGTK lays out nothing in a window not yet shown, and a
   * document opened at launch gets here first (fitted then, it would come out at the smallest zoom, scrolled to the top).
   */
  private waiting: ViewState | null = null;
  /** The view when last laid out with a size: a hidden viewer (a background tab) has no scroll position to read. */
  private lastView: ViewState | null = null;
  private lastSharpAt = 0;
  private zoomChangedAt = 0;
  private lastTop = 0;
  private lastT = 0;
  private velocity = 0; // px per ms, smoothed
  private pinch: { zoom: number; ax: number; ay: number; timer: number } | null = null;
  private night = false;
  /** Image rectangles per original page (for night mode), or "loading". */
  private imageRects = new Map<number, [number, number, number, number][] | "loading">();
  /** Photos found inside scanned pages (fractions of the page as shown), by "page:rotation". */
  private scanPhotos = new Map<string, [number, number, number, number][]>();
  private animating = false;
  private layers: PageLayer[] = [];
  private fieldsDrawn = false;
  /** Timings for diagnostics (ms since the viewer was created). */
  readonly stats = { created: performance.now(), firstLo: 0, firstHi: 0, lo: 0, hi: 0, tiles: 0 };

  constructor(host: HTMLElement, info: DocInfo, private cb: ViewerCallbacks) {
    this.info = info;
    this.group = `view:${info.docId}`;
    this.pages = Array.from({ length: info.pageCount }, (_, i) => ({ src: i, rot: 0, w: info.firstSize[0], h: info.firstSize[1] }));
    this.el = document.createElement("div");
    this.el.className = "viewer";
    this.el.tabIndex = 0;
    this.sizer = document.createElement("div");
    this.sizer.className = "viewer-sizer";
    this.el.appendChild(this.sizer);
    host.appendChild(this.el);

    this.el.addEventListener(
      "scroll",
      () => {
        // Moved by the reader (not by a re-layout settling): the page now in the middle.
        if (this.slots && performance.now() - this.layoutAt > 400) this.here = this.pageAtY(this.el.scrollTop + this.el.clientHeight / 2);
        this.schedule();
        this.cb.onScroll?.();
      },
      { passive: true },
    );
    this.el.addEventListener("wheel", (e) => this.onWheel(e), { passive: false });
    this.el.addEventListener("pointerdown", (e) => this.onPointerDown(e));
    this.el.addEventListener("touchstart", (e) => this.onTouch(e), { passive: true });
    this.el.addEventListener("touchmove", (e) => this.onTouch(e), { passive: false });
    this.el.addEventListener("touchend", (e) => this.onTouch(e));
    this.el.addEventListener("touchcancel", (e) => this.onTouch(e));
    this.ro = new ResizeObserver(() => this.onResize());
    this.ro.observe(this.el);
    this.indexPages();
    this.layout();
  }

  // ------------------------------------------------------------ geometry

  private get cssScale() {
    return this.zoom * PT_TO_CSS;
  }

  private get dpr() {
    return window.devicePixelRatio || 1;
  }

  private indexPages() {
    this.srcToView.clear();
    this.pages.forEach((p, i) => this.srcToView.set(p.src, i));
  }

  private layout() {
    this.layoutAt = performance.now();
    const s = this.cssScale;
    const n = this.pages.length;
    this.px = new Float64Array(n);
    this.py = new Float64Array(n);
    this.rowOf = new Int32Array(n);
    const rowsY: number[] = [], rowsFirst: number[] = [], rowsW: number[] = [];
    const slot = this.slots ? this.el.clientHeight : 0;
    let y = slot ? 0 : PAD_Y;
    let i = 0;
    while (i < n) {
      const pair = this.layoutMode !== "single" && !(this.layoutMode === "spread-cover" && i === 0) && i + 1 < n;
      const count = pair ? 2 : 1;
      let w = 0, h = 0;
      for (let k = 0; k < count; k++) {
        w += this.pages[i + k].w * s;
        h = Math.max(h, this.pages[i + k].h * s);
      }
      if (count === 2) w += GAP;
      const r = rowsY.length;
      rowsY.push(y);
      rowsFirst.push(i);
      rowsW.push(w);
      const rowH = Math.max(h + GAP, slot);
      for (let k = 0; k < count; k++) {
        this.rowOf[i + k] = r;
        // Pages in a row are centred on the row's centre line.
        this.py[i + k] = y + (slot ? (rowH - this.pages[i + k].h * s) / 2 : (h - this.pages[i + k].h * s) / 2);
      }
      y += rowH;
      i += count;
    }
    this.rowY = Float64Array.from([...rowsY, y]);
    this.rowFirst = Int32Array.from(rowsFirst);
    this.rowW = Float64Array.from(rowsW);
    this.contentH = slot ? y : y - GAP + PAD_Y;
    this.contentW = this.localWidth();
    this.positionColumns();
    this.sizer.style.width = `${this.contentW}px`;
    this.sizer.style.height = `${this.contentH}px`;
    for (const pv of this.mounted.values()) this.place(pv);
  }

  /** Horizontal positions: each row centred in the content width. */
  private positionColumns() {
    const s = this.cssScale;
    for (let r = 0; r < this.rowFirst.length; r++) {
      let x = (this.contentW - this.rowW[r]) / 2;
      const end = r + 1 < this.rowFirst.length ? this.rowFirst[r + 1] : this.pages.length;
      for (let i = this.rowFirst[r]; i < end; i++) {
        this.px[i] = x;
        x += this.pages[i].w * s + GAP;
      }
    }
  }

  /**
   * Content width: wide enough for the rows near the viewport only, so a single
   * wide page elsewhere in the document doesn't add empty space to scroll into.
   */
  private localWidth() {
    const vw = this.el.clientWidth;
    if (!this.rowFirst.length) return vw;
    const top = this.el.scrollTop, vh = this.el.clientHeight || 800;
    const r0 = this.rowAtY(Math.max(0, top - vh)), r1 = this.rowAtY(top + vh * 2);
    let w = 0;
    for (let r = r0; r <= r1; r++) w = Math.max(w, this.rowW[r]);
    return Math.max(vw, Math.ceil(w + PAD_X * 2));
  }

  /** Re-checks the content width after scrolling; keeps the view centred on the same spot. */
  private updateWidth() {
    const w = this.localWidth();
    if (Math.abs(w - this.contentW) < 1) return;
    const centre = this.el.scrollLeft + this.el.clientWidth / 2 - this.contentW / 2;
    this.contentW = w;
    this.sizer.style.width = `${w}px`;
    this.positionColumns();
    for (const pv of this.mounted.values()) this.place(pv);
    this.el.scrollLeft = w / 2 + centre - this.el.clientWidth / 2;
  }

  private pageRect(i: number) {
    const s = this.cssScale;
    return { x: this.px[i], y: this.py[i], w: this.pages[i].w * s, h: this.pages[i].h * s };
  }

  private rowAtY(y: number): number {
    let lo = 0, hi = this.rowFirst.length - 1;
    if (hi < 0) return 0;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (this.rowY[mid] <= y) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  }

  private pageAtY(y: number): number {
    return this.rowFirst[this.rowAtY(y)] ?? 0;
  }

  /** Last page index in the row containing y. */
  private lastPageAtY(y: number): number {
    const r = this.rowAtY(y);
    return r + 1 < this.rowFirst.length ? this.rowFirst[r + 1] - 1 : this.pages.length - 1;
  }

  // ------------------------------------------------------------ public API

  /** Replaces the displayed page list (after sizes load or page edits), keeping the reading position. */
  setPages(pages: PageSpec[], keepPage?: number) {
    const st = this.viewState();
    if (keepPage !== undefined) {
      st.page = keepPage;
      st.frac = 0;
    }
    this.pages = pages;
    this.indexPages();
    // Anything drawn for a changed page (new rotation, new size, another page in its place) is stale. Pages that
    // stay as they were keep their pictures and the renders on their way (the real sizes arriving just after
    // opening mostly match the first page's, which the viewer starts with).
    const same = (a: PageSpec, b: PageSpec | undefined) => !!b && a.src === b.src && a.rot === b.rot && a.w === b.w && a.h === b.h;
    let changed = false;
    for (const [i, pv] of this.mounted) {
      if (same(pv.spec, pages[i])) continue;
      changed = true;
      this.dropPage(pv);
      this.mounted.delete(i);
    }
    const kept = new Set([...this.mounted.values()].map((pv) => pv.spec.src));
    // Renders on their way stay wanted while their page is in the document: a render doesn't depend on the page's
    // place or size, so a page laid out again (its real size in place of the first page's) takes the same one.
    this.sharpWants = this.sharpWants.filter((w) => this.srcToView.has(w.params.page));
    for (const map of [this.links, this.imageRects]) for (const src of [...map.keys()]) if (!kept.has(src)) map.delete(src);
    for (const key of [...this.scanPhotos.keys()]) if (!kept.has(Number(key.split(":")[0]))) this.scanPhotos.delete(key);
    if (changed && this.selection) this.clearSelection();
    st.page = Math.min(st.page, Math.max(0, pages.length - 1));
    this.layout();
    this.restore(st);
  }

  get pageCount() {
    return this.pages.length;
  }

  get currentPage() {
    return this.lastPage < 0 ? 0 : this.lastPage;
  }

  get zoomLevel() {
    return this.zoom;
  }

  get layoutModeValue() {
    return this.layoutMode;
  }

  setLayoutMode(mode: LayoutMode) {
    if (mode === this.layoutMode) return;
    const st = this.viewState();
    this.layoutMode = mode;
    this.layout();
    this.restore(st);
  }

  /** One page per screen (a page turner) or the usual column of pages. */
  setSlots(on: boolean) {
    if (on === this.slots) return;
    const st = this.viewState();
    this.slots = on;
    this.here = on ? st.page : -1;
    this.el.classList.toggle("slots", on);
    this.layout();
    this.restore(st);
  }

  /**
   * With slots: shows the screen `dir` screens on from the one holding `page` (0: back to it),
   * scrolling there smoothly or at once.
   */
  turnFrom(page: number, dir: -1 | 0 | 1, smooth: boolean) {
    if (!this.pages.length) return;
    if (!this.live()) return this.waitAt(page + dir);
    const rows = this.rowFirst.length;
    const r = Math.min(Math.max(0, this.rowOf[Math.min(Math.max(0, page), this.pages.length - 1)] + dir), rows - 1);
    this.here = this.rowFirst[r];
    this.el.scrollTo({ top: this.rowY[r], behavior: smooth ? "smooth" : "auto" });
    this.schedule(true);
  }

  viewState(): ViewState {
    if (this.waiting) return { ...this.waiting };
    if (!this.el.clientWidth && this.lastView) return { ...this.lastView };
    if (!this.pages.length) return { page: 0, frac: 0, zoom: this.zoom, fit: this.fit };
    if (this.slots && this.here >= 0 && this.fit !== "none") return { page: Math.min(this.here, this.pages.length - 1), frac: 0, zoom: this.zoom, fit: this.fit };
    const top = this.el.scrollTop;
    const page = this.pageAtY(top + Math.min(this.el.clientHeight * 0.1, 40));
    const r = this.pageRect(page);
    return { page, frac: r.h > 0 ? (top - r.y) / r.h : 0, zoom: this.zoom, fit: this.fit };
  }

  restore(st: ViewState) {
    this.show(st, true);
  }

  /**
   * Whether the view can move now. A viewer waiting for a size shows its view first if it has one by now;
   * otherwise moves change the view it waits with (moving the unsized layout would be undone when the size comes).
   */
  private live(): boolean {
    // Hidden since it was last shown: moves wait for it to be shown again, from where it was.
    if (!this.waiting && !this.el.clientWidth && this.lastView) this.waiting = { ...this.lastView };
    if (this.waiting && this.el.clientWidth) this.show(this.waiting, false);
    return !this.waiting;
  }

  /** The view to show at a page once there's a size (`y`: points down the page). */
  private waitAt(page: number, y = 0) {
    const p = Math.min(Math.max(0, page), Math.max(0, this.pages.length - 1));
    const h = this.pages[p]?.h ?? 0;
    this.waiting = { ...this.waiting!, page: p, frac: h > 0 ? Math.max(0, y) / h : 0 };
  }

  /** Lays the view out for `st`; `schedule`: and asks for its pages (update() calling it does that itself next). */
  private show(st: ViewState, schedule: boolean) {
    if (!this.el.clientWidth) {
      this.waiting = { ...st };
      return;
    }
    this.waiting = null;
    if (st.fit !== "none") {
      this.fit = st.fit;
      this.zoom = this.fitZoom(st.fit, st.page);
    } else {
      this.fit = "none";
      this.zoom = clampZoom(st.zoom);
    }
    this.layout();
    if (this.pages.length) {
      const p = Math.min(Math.max(0, st.page), this.pages.length - 1);
      const r = this.pageRect(p);
      // A page per screen: back to the page's own screen (the page's size has changed with the view).
      this.el.scrollTop = this.slots && this.fit !== "none" ? this.rowY[this.rowOf[p]] : r.y + st.frac * r.h;
      if (this.slots) this.here = p;
      this.updateWidth();
      this.el.scrollLeft = (this.contentW - this.el.clientWidth) / 2;
    }
    this.cb.onZoom(this.zoom, this.fit);
    if (schedule) this.schedule(true);
  }

  goToPage(page: number, yPt?: number) {
    if (!this.pages.length) return;
    if (!this.live()) return this.waitAt(page, yPt);
    const p = Math.min(Math.max(0, page), this.pages.length - 1);
    const r = this.pageRect(p);
    const y = yPt !== undefined && yPt > 0 ? r.y + yPt * this.cssScale - 16 : this.slots ? this.rowY[this.rowOf[p]] : r.y - GAP / 2;
    this.el.scrollTop = Math.max(0, y);
    if (this.slots) this.here = p;
    this.schedule(true);
  }

  /**
   * Next/previous row: one page in single layout, one spread in book layouts.
   * A row that fits the window is centred in it (page turning in reading mode).
   */
  stepRow(dir: 1 | -1) {
    if (!this.pages.length) return;
    if (!this.live()) return this.waitAt(this.waiting!.page + dir);
    const vh = this.el.clientHeight;
    const row = this.rowOf[this.pageAtY(this.el.scrollTop + vh / 2)];
    const r = Math.min(Math.max(0, row + dir), this.rowFirst.length - 1);
    // Fitted views re-fit when the next spread has a different size (portrait → landscape).
    if (this.fit !== "none") {
      const z = this.fitZoom(this.fit, this.rowFirst[r]);
      if (Math.abs(z - this.zoom) / this.zoom > 0.005) {
        this.zoom = z;
        this.zoomChangedAt = performance.now();
        this.layout();
        this.updateWidth();
        this.el.scrollLeft = (this.contentW - this.el.clientWidth) / 2;
        this.cb.onZoom(this.zoom, this.fit);
      }
    }
    const y0 = this.rowY[r], h = this.rowY[r + 1] - GAP - y0;
    this.el.scrollTop = Math.max(0, this.slots ? y0 : h <= vh ? y0 + h / 2 - vh / 2 : y0 - GAP / 2);
    if (this.slots) this.here = this.rowFirst[r];
    this.schedule(true);
  }

  /** Goes to a page by its index in the file (links, outline, search). */
  goToSource(src: number, yPt?: number) {
    const v = this.srcToView.get(src);
    if (v === undefined) return false;
    const spec = this.pages[v];
    const y = yPt !== undefined && yPt > 0 && spec.rot === 0 ? yPt : undefined;
    this.goToPage(v, y);
    return true;
  }

  /** The page (as shown) and height on it, in points, that is `offset` px below the top of the view. */
  pointAt(offset: number): { page: number; y: number } {
    if (!this.pages.length) return { page: 0, y: 0 };
    const top = this.el.scrollTop + offset;
    const page = this.pageAtY(top);
    return { page, y: (top - this.pageRect(page).y) / this.cssScale };
  }

  /** Scrolls so the point `y` points down page `page` (as shown) is `offset` px below the top of the view. */
  scrollToPoint(page: number, y: number, offset: number) {
    if (!this.pages.length) return;
    if (!this.live()) return this.waitAt(page, y);
    const p = Math.min(Math.max(0, page), this.pages.length - 1);
    this.el.scrollTop = Math.max(0, this.pageRect(p).y + y * this.cssScale - offset);
    this.schedule(true);
  }

  viewOfSource(src: number) {
    return this.srcToView.get(src);
  }

  private fitZoom(fit: FitMode, page: number) {
    if (!this.pages.length) return 1;
    const i = Math.min(Math.max(0, page), this.pages.length - 1);
    const vw0 = this.el.clientWidth - PAD_X * 2 - 4;
    const vh = this.el.clientHeight - PAD_Y * 2;
    if (this.layoutMode === "single") {
      const { w, h } = this.pages[i];
      const byWidth = vw0 / (w * PT_TO_CSS);
      return clampZoom(fit === "width" ? Math.min(byWidth, 4) : Math.min(byWidth, vh / (h * PT_TO_CSS)));
    }
    // Two-page layouts: fit the spread you're on. A lone page (the cover, or an
    // odd last page) is fitted like its neighbouring spread, so the pairs next
    // to it don't overflow.
    let r = this.rowOf[i] ?? 0;
    const rows = this.rowFirst.length;
    const rowEnd = (x: number) => (x + 1 < rows ? this.rowFirst[x + 1] : this.pages.length);
    if (rowEnd(r) - this.rowFirst[r] === 1) {
      if (r + 1 < rows && rowEnd(r + 1) - this.rowFirst[r + 1] === 2) r++;
      else if (r > 0 && rowEnd(r - 1) - this.rowFirst[r - 1] === 2) r--;
    }
    let w = 0, h = 0;
    for (let k = this.rowFirst[r]; k < rowEnd(r); k++) {
      w += this.pages[k].w;
      h = Math.max(h, this.pages[k].h);
    }
    const n = rowEnd(r) - this.rowFirst[r];
    const byWidth = (vw0 - (n - 1) * GAP) / (w * PT_TO_CSS);
    return clampZoom(fit === "width" ? Math.min(byWidth, 4) : Math.min(byWidth, vh / (h * PT_TO_CSS)));
  }

  setFit(fit: FitMode) {
    const live = this.live();
    const st = this.viewState();
    this.fit = fit;
    if (!live) this.waiting = { ...st, fit };
    else if (fit !== "none") {
      st.fit = fit;
      this.restore(st);
    }
  }

  /** Zooms to `z`, keeping the document point under (ax, ay) (viewport px) fixed. */
  setZoom(z: number, ax?: number, ay?: number) {
    z = clampZoom(z);
    if (!this.live()) {
      this.waiting = { ...this.waiting!, zoom: z, fit: "none" };
      return;
    }
    if (Math.abs(z - this.zoom) < 1e-4 && this.fit === "none") return;
    if (!this.pages.length) return;
    const vx = ax ?? this.el.clientWidth / 2;
    const vy = ay ?? this.el.clientHeight / 2;
    const docY = this.el.scrollTop + vy;
    const docX = this.el.scrollLeft + vx;
    const page = this.pageAtY(docY);
    const r0 = this.pageRect(page);
    const fy = (docY - r0.y) / r0.h;
    const fx = (docX - r0.x) / r0.w;
    this.zoom = z;
    this.fit = "none";
    this.zoomChangedAt = performance.now();
    this.layout();
    const r1 = this.pageRect(page);
    this.el.scrollTop = r1.y + fy * r1.h - vy;
    this.updateWidth();
    const r2 = this.pageRect(page);
    this.el.scrollLeft = r2.x + fx * r2.w - vx;
    this.cb.onZoom(this.zoom, this.fit);
    this.schedule();
  }

  /** Animated zoom step (buttons, Ctrl+= / Ctrl+-): a quick GPU scale, then a real re-layout. */
  zoomBy(factor: number) {
    if (!this.live()) return this.setZoom(this.waiting!.zoom * factor);
    if (this.animating || this.pinch) {
      this.setZoom(this.zoom * factor);
      return;
    }
    const target = clampZoom(this.zoom * factor);
    if (Math.abs(target - this.zoom) < 1e-4) return;
    const k = target / this.zoom;
    const ox = this.el.scrollLeft + this.el.clientWidth / 2;
    const oy = this.el.scrollTop + this.el.clientHeight / 2;
    this.animating = true;
    this.sizer.style.transformOrigin = `${ox}px ${oy}px`;
    this.sizer.style.transition = "transform 140ms cubic-bezier(0.2, 0.8, 0.2, 1)";
    this.sizer.style.transform = `scale(${k})`;
    setTimeout(() => {
      this.sizer.style.transition = "";
      this.sizer.style.transform = "";
      this.animating = false;
      this.setZoom(target);
    }, 145);
  }

  /** Night mode inverts pages; photos and figures are patched back to their real colours. */
  setNight(on: boolean) {
    if (on === this.night) return;
    this.night = on;
    for (const pv of this.mounted.values()) this.drawPhotos(pv);
  }

  setTool(tool: "select" | "hand") {
    this.tool = tool;
    this.el.classList.toggle("tool-hand", tool === "hand");
  }

  /** Search hits keyed by source page; `active.page` is a source page too. */
  setSearchHits(hits: Map<number, number[][]>, active: { page: number; index: number } | null) {
    this.search = hits;
    this.activeHit = active;
    for (const pv of this.mounted.values()) this.drawMarks(pv);
  }

  revealHit(src: number, quad: number[]) {
    const v = this.srcToView.get(src);
    if (v === undefined) return;
    const spec = this.pages[v];
    if (!this.live()) return this.waitAt(v, quadBox(spec, quad)[1]);
    const r = this.pageRect(v);
    const s = this.cssScale;
    const [x0, y0] = quadBox(spec, quad);
    const y = r.y + y0 * s;
    const x = r.x + x0 * s;
    const vh = this.el.clientHeight;
    if (y < this.el.scrollTop + 40 || y > this.el.scrollTop + vh - 60) this.el.scrollTop = y - vh / 3;
    this.updateWidth();
    const x2 = this.pageRect(v).x + x0 * s;
    if (x2 < this.el.scrollLeft || x2 > this.el.scrollLeft + this.el.clientWidth - 40) this.el.scrollLeft = x2 - 60;
    void x;
    this.schedule(true);
  }

  async copySelection(): Promise<string | null> {
    const s = this.selection;
    if (!s) return null;
    const src = this.pages[s.view].src;
    const text = await (s.range ? api.rangeCopy(this.info.docId, src, s.range[0], s.range[1]) : api.copy(this.info.docId, src, s.a, s.b));
    return text || null;
  }

  clearSelection() {
    this.selection = null;
    this.selQuads = [];
    for (const pv of this.mounted.values()) this.drawMarks(pv);
    this.cb.onSelection(false);
  }

  /** Selects all the text on displayed page `view` (by default the one being read). */
  selectAllOnPage(view?: number) {
    const v = view ?? this.currentPage;
    // Mode 5: the engine's first character to its last (corner to corner misses lines: MuPDF
    // takes each corner to its nearest line).
    void this.updateSelection({ view: v, a: [0, 0], b: [0, 0], mode: 5 });
  }


  focus() {
    this.el.focus({ preventScroll: true });
  }

  /** Background tabs give back their page canvases and pending renders. */
  setActive(active: boolean) {
    if (active) {
      this.schedule(true);
      return;
    }
    cancelAnimationFrame(this.raf);
    clearTimeout(this.renderTimer);
    for (const pv of this.mounted.values()) {
      this.dropPage(pv);
    }
    this.mounted.clear();
    this.sharpWants = [];
    this.sharpScale = 0;
    renderer.release(this.group);
    api.trim(this.info.docId).catch(() => {});
  }

  /** Called after the file on disk changed (save): forget everything rendered from it. */
  reset(info: DocInfo) {
    this.info = info;
    renderer.release(this.group);
    renderer.forget(`${info.docId}.`);
    this.links.clear();
  }

  destroy() {
    this.destroyed = true;
    cancelAnimationFrame(this.raf);
    clearTimeout(this.renderTimer);
    if (this.pinch) clearTimeout(this.pinch.timer);
    this.ro.disconnect();
    renderer.release(this.group);
    for (const pv of this.mounted.values()) this.dropPage(pv);
    this.mounted.clear();
    this.el.remove();
  }

  // ------------------------------------------------------------ events

  private onResize() {
    if (this.destroyed || !this.el.clientWidth) return;
    if (this.waiting) return this.restore(this.waiting);
    const st = this.viewState();
    this.layout();
    if (this.fit !== "none") {
      const z = this.zoom;
      this.show(st, false);
      // A size that changes every frame (a pane or the sidebar sliding, the window dragged bigger) re-fits every
      // frame: sharp pages wait for it to settle, as they do while zooming.
      if (Math.abs(this.zoom - z) > 1e-4) this.zoomChangedAt = performance.now();
    }
    this.schedule();
  }

  /**
   * Ctrl+wheel and trackpad pinch: scale the already-rendered pages on the GPU
   * while the gesture runs, then do one real re-layout when it pauses.
   */
  private onWheel(e: WheelEvent) {
    if (!e.ctrlKey) return;
    e.preventDefault();
    const rect = this.el.getBoundingClientRect();
    // Mouse wheels send whole ±100-ish steps per notch (≈ 25% zoom each).
    // Touchpad pinches arrive as small fractional deltas of -100·ln(scale),
    // so ×0.01 makes the page follow the fingers exactly.
    const delta = e.deltaMode === 1 ? e.deltaY * 33 : e.deltaY;
    const notch = Number.isInteger(delta) && Math.abs(delta) >= 50;
    const factor = Math.exp(-Math.max(-300, Math.min(300, delta)) * (notch ? 0.0022 : 0.01));
    const ax = e.clientX - rect.left, ay = e.clientY - rect.top;
    if (!this.pinch) {
      this.pinch = { zoom: this.zoom, ax, ay, timer: 0 };
      this.sizer.style.transition = "";
      this.sizer.style.transformOrigin = `${this.el.scrollLeft + ax}px ${this.el.scrollTop + ay}px`;
    }
    const p = this.pinch;
    p.zoom = clampZoom(p.zoom * factor);
    this.sizer.style.transform = `scale(${p.zoom / this.zoom})`;
    clearTimeout(p.timer);
    p.timer = window.setTimeout(() => {
      this.sizer.style.transform = "";
      this.pinch = null;
      this.setZoom(p.zoom, p.ax, p.ay);
    }, 130);
  }

  /**
   * Two-finger pinch on a touch screen: like a trackpad pinch, the pages are scaled (and moved
   * with the fingers) on the GPU while it runs, then laid out once at the new zoom. One finger
   * scrolls natively.
   */
  private touchPinch: { zoom: number; d0: number; cx: number; cy: number; dx: number; dy: number } | null = null;
  private onTouch(e: TouchEvent) {
    const rect = this.el.getBoundingClientRect();
    const two = e.touches.length === 2 ? [e.touches[0], e.touches[1]] : null;
    const p = this.touchPinch;
    if (two && !p) {
      if (this.pinch) return;
      const [a, b] = two;
      const cx = (a.clientX + b.clientX) / 2 - rect.left, cy = (a.clientY + b.clientY) / 2 - rect.top;
      this.touchPinch = { zoom: this.zoom, d0: Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY) || 1, cx, cy, dx: 0, dy: 0 };
      this.sizer.style.transition = "";
      this.sizer.style.transformOrigin = `${this.el.scrollLeft + cx}px ${this.el.scrollTop + cy}px`;
      return;
    }
    if (!p) return;
    if (two && e.type === "touchmove") {
      e.preventDefault();
      const [a, b] = two;
      const d = Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY);
      p.zoom = clampZoom((this.zoom * d) / p.d0);
      p.dx = (a.clientX + b.clientX) / 2 - rect.left - p.cx;
      p.dy = (a.clientY + b.clientY) / 2 - rect.top - p.cy;
      this.sizer.style.transform = `translate(${p.dx}px, ${p.dy}px) scale(${p.zoom / this.zoom})`;
      return;
    }
    if (two) return;
    // A finger lifted: settle at the new zoom, the pinched point where the fingers left it.
    this.touchPinch = null;
    this.sizer.style.transform = "";
    if (Math.abs(p.zoom - this.zoom) > 1e-3) this.setZoom(p.zoom, p.cx, p.cy);
    this.el.scrollLeft -= p.dx;
    this.el.scrollTop -= p.dy;
  }

  // ------------------------------------------------------------ touch selection (the Android viewer)

  /**
   * The page under a screen point and the point on it, by geometry (not by the element there:
   * the selection's handles or a menu may be on top).
   */
  private pageAtClient(clientX: number, clientY: number): { view: number; p: [number, number] } | null {
    for (const pv of this.mounted.values()) {
      const r = pv.el.getBoundingClientRect();
      if (clientX >= r.left && clientX <= r.right && clientY >= r.top && clientY <= r.bottom) return { view: pv.index, p: this.pointToPage(pv, clientX, clientY) };
    }
    return null;
  }

  /** A long press: selects the word under the screen point. Resolves true if there was text. */
  async selectWordAt(clientX: number, clientY: number): Promise<boolean> {
    const at = this.pageAtClient(clientX, clientY);
    if (!at) return false;
    // Mode 3: the engine's word nearest the point (MuPDF's own word snapping needs the point close
    // to the middle of the line, which a fingertip on small text rarely is).
    void this.updateSelection({ view: at.view, a: at.p, b: at.p, mode: 3 });
    await this.selectionSettled();
    if (!this.selQuads.length) this.clearSelection();
    return this.selQuads.length > 0;
  }

  /**
   * Selects the paragraph at a point on displayed page `view` (unrotated page points), or with
   * none, the paragraph holding the selection's start. Resolves true if there was text.
   */
  async selectParagraph(at?: { view: number; p: [number, number] }): Promise<boolean> {
    const s = this.selection;
    const where = at ?? (s ? { view: s.view, p: s.a } : null);
    if (!where) return false;
    void this.updateSelection({ view: where.view, a: where.p, b: where.p, mode: 4 });
    await this.selectionSettled();
    return this.selQuads.length > 0;
  }

  /** Moves the start ("a") or end ("b") of the selection to a screen point on its page (a handle dragged). */
  moveSelectionEnd(end: "a" | "b", clientX: number, clientY: number) {
    const s = this.selection;
    const pv = s && this.mounted.get(s.view);
    if (!s || !pv) return;
    // From here it's a range between two points (the word, paragraph or page range is left behind).
    const p = this.pointToPage(pv, clientX, clientY);
    void this.updateSelection({ view: s.view, a: end === "a" ? p : s.a, b: end === "b" ? p : s.b, mode: 0 });
  }

  /**
   * The selection's first and last line on screen: where its handles go (bottom-left of the
   * first, bottom-right of the last) and the menu (above the top). Null with none, or off screen.
   */
  selectionEnds(): { start: [number, number]; end: [number, number]; top: number; bottom: number } | null {
    const s = this.selection;
    const pv = s && this.mounted.get(s.view);
    if (!s || !pv || !this.selQuads.length) return null;
    const r = pv.el.getBoundingClientRect();
    const spec = pv.spec;
    const box = (q: ArrayLike<number>) => {
      const [x0, y0, x1, y1] = quadBox(spec, q);
      return [r.left + (x0 / spec.w) * r.width, r.top + (y0 / spec.h) * r.height, r.left + (x1 / spec.w) * r.width, r.top + (y1 / spec.h) * r.height];
    };
    const first = box(this.selQuads[0]), last = box(this.selQuads[this.selQuads.length - 1]);
    let top = Infinity, bottom = -Infinity;
    for (const q of this.selQuads) {
      const b = box(q);
      top = Math.min(top, b[1]);
      bottom = Math.max(bottom, b[3]);
    }
    return { start: [first[0], first[3]], end: [last[2], last[3]], top, bottom };
  }

  /** The displayed page the selection is on (null with none). */
  get selectionView() {
    return this.selection?.view ?? null;
  }

  get hasSelection() {
    return this.selQuads.length > 0;
  }

  /** Screen point → unrotated page points on displayed page `view` (null if it isn't in view). */
  clientToPage(view: number, clientX: number, clientY: number): [number, number] | null {
    const pv = this.mounted.get(view);
    return pv ? this.pointToPage(pv, clientX, clientY) : null;
  }

  /** Mouse position → unrotated page coordinates of the page under it. */
  private pointToPage(pv: PageView, clientX: number, clientY: number): [number, number] {
    const rect = pv.el.getBoundingClientRect();
    const spec = pv.spec;
    const X = Math.max(0, Math.min(spec.w, ((clientX - rect.left) / rect.width) * spec.w));
    const Y = Math.max(0, Math.min(spec.h, ((clientY - rect.top) / rect.height) * spec.h));
    return toOriginal(spec, X, Y);
  }

  private onPointerDown(e: PointerEvent) {
    // Touch scrolls and pinches (see onTouch); it doesn't select. Older Android WebViews (83)
    // report touches with an empty pointerType, so only a mouse or pen selects.
    if (e.pointerType !== "mouse" && e.pointerType !== "pen") return;
    if (e.button === 1 || (e.button === 0 && this.tool === "hand")) return this.startPan(e);
    if (e.button !== 0) return;
    const t = e.target as HTMLElement;
    if (t.closest(".pg-ctl") || t.closest("a") || t.closest(".fw")) return;
    const target = t.closest<HTMLElement>(".pg");
    if (!target) return;
    const pv = this.mounted.get(Number(target.dataset.index));
    if (!pv) return;
    e.preventDefault();
    this.focus();
    const start = this.pointToPage(pv, e.clientX, e.clientY);
    const mode: 0 | 1 | 2 = e.detail >= 3 ? 2 : e.detail === 2 ? 1 : 0;
    let moved = mode !== 0;
    if (mode !== 0) this.updateSelection({ view: pv.index, a: start, b: start, mode });
    else if (this.selection) this.clearSelection();

    // The box a drag covers, drawn while it goes when asked for (in fractions of the page, so scrolling keeps it).
    const frac = (ev: { clientX: number; clientY: number }): [number, number] => {
      const r = pv.el.getBoundingClientRect();
      return [Math.max(0, Math.min(1, (ev.clientX - r.left) / r.width)), Math.max(0, Math.min(1, (ev.clientY - r.top) / r.height))];
    };
    const f0 = frac(e);
    let box: HTMLDivElement | null = null;
    const drawBox = (ev: PointerEvent) => {
      if (!this.cb.dragBox?.(this.selQuads.length > 0)) {
        box?.remove();
        box = null;
        return;
      }
      if (!box) {
        box = document.createElement("div");
        box.className = "drag-box";
        pv.el.append(box);
      }
      const f1 = frac(ev);
      Object.assign(box.style, {
        left: `${Math.min(f0[0], f1[0]) * 100}%`,
        top: `${Math.min(f0[1], f1[1]) * 100}%`,
        width: `${Math.abs(f1[0] - f0[0]) * 100}%`,
        height: `${Math.abs(f1[1] - f0[1]) * 100}%`,
      });
    };
    const move = (ev: PointerEvent) => {
      const p = this.pointToPage(pv, ev.clientX, ev.clientY);
      if (!moved && Math.hypot(p[0] - start[0], p[1] - start[1]) < 2) return;
      moved = true;
      this.updateSelection({ view: pv.index, a: start, b: p, mode });
      drawBox(ev);
      this.autoScroll(ev);
    };
    let last = start;
    const track = (ev: PointerEvent) => (last = this.pointToPage(pv, ev.clientX, ev.clientY));
    window.addEventListener("pointermove", track);
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointermove", track);
      window.removeEventListener("pointerup", up);
      this.stopAutoScroll();
      box?.remove();
      if (moved && this.cb.onSelectionEnd) {
        const src = pv.spec.src;
        void this.selectionSettled().then(() => this.cb.onSelectionEnd?.(src, this.selQuads, start, last));
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  /** Waits for the text selection's last update to arrive from the engine. */
  private async selectionSettled() {
    while (this.selBusy) await new Promise((r) => setTimeout(r, 8));
  }

  private autoTimer = 0;
  private autoScroll(ev: PointerEvent) {
    const r = this.el.getBoundingClientRect();
    const edge = 40;
    const dy = ev.clientY < r.top + edge ? -1 : ev.clientY > r.bottom - edge ? 1 : 0;
    this.stopAutoScroll();
    if (dy) this.autoTimer = window.setInterval(() => (this.el.scrollTop += dy * 18), 16);
  }
  private stopAutoScroll() {
    clearInterval(this.autoTimer);
  }

  private startPan(e: PointerEvent) {
    e.preventDefault();
    const sx = e.clientX, sy = e.clientY;
    const st = this.el.scrollTop, sl = this.el.scrollLeft;
    this.el.classList.add("panning");
    const move = (ev: PointerEvent) => {
      this.el.scrollTop = st - (ev.clientY - sy);
      this.el.scrollLeft = sl - (ev.clientX - sx);
    };
    const up = () => {
      this.el.classList.remove("panning");
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  private selBusy = false;
  private selNext: Selection | null = null;
  private async updateSelection(sel: Selection) {
    this.selection = sel;
    if (this.selBusy) {
      this.selNext = sel;
      return;
    }
    this.selBusy = true;
    try {
      let cur: Selection | null = sel;
      while (cur) {
        this.selNext = null;
        const src = this.pages[cur.view].src;
        if (cur.mode >= 3) {
          // A word, paragraph or page: the engine's character range, drawn as it is.
          const r = await api.textRange(this.info.docId, src, cur.a, cur.mode as 3 | 4 | 5).catch(() => null);
          if (r?.quads.length) {
            const q0 = r.quads[0], qn = r.quads[r.quads.length - 1];
            this.selection = { view: cur.view, a: [q0[0] + 0.1, (q0[1] + q0[5]) / 2], b: [qn[6] - 0.1, (qn[3] + qn[7]) / 2], mode: 0, range: r.range };
            this.selQuads = r.quads;
          } else this.selQuads = [];
          for (const p of this.mounted.values()) this.drawMarks(p);
          this.cb.onSelection(this.selQuads.length > 0);
          cur = this.selNext;
          continue;
        }
        const quads = await api.select(this.info.docId, src, cur.a, cur.b, cur.mode as 0 | 1 | 2).catch(() => [] as Quad[]);
        if (cur.mode !== 0 && quads.length) {
          // Snapped selections become a normal range from the snapped quad.
          const q = quads[0];
          this.selection = { ...cur, a: [q[0] + 0.1, (q[1] + q[5]) / 2], b: [q[6] - 0.1, (q[3] + q[7]) / 2], mode: 0 };
          this.selQuads = await api.select(this.info.docId, src, this.selection.a, this.selection.b, 0).catch(() => quads);
        } else {
          this.selQuads = quads;
        }
        for (const p of this.mounted.values()) this.drawMarks(p);
        this.cb.onSelection(this.selQuads.length > 0);
        cur = this.selNext;
      }
    } finally {
      this.selBusy = false;
    }
  }

  // ------------------------------------------------------------ mounting & rendering

  schedule(immediate = false) {
    if (this.destroyed) return;
    cancelAnimationFrame(this.raf);
    // A pass asked for at once stays at once when a scroll or resize asks again in the same frame.
    this.rafImmediate ||= immediate;
    this.raf = requestAnimationFrame(() => {
      const now = this.rafImmediate;
      this.rafImmediate = false;
      this.update(now);
    });
  }

  private update(immediate: boolean) {
    // Sized now, though the resize observer may not have said so yet: the view first.
    this.live();
    const n = this.pages.length;
    const vh = this.el.clientHeight;
    if (!n || !vh) {
      for (const pv of this.mounted.values()) this.dropPage(pv);
      this.mounted.clear();
      return;
    }
    if (!this.pinch && !this.animating) this.updateWidth();
    this.lastView = this.viewState();
    const top = this.el.scrollTop;

    const first = this.pageAtY(top);
    const last = this.lastPageAtY(top + vh);
    // Mount a bit more while a pinch is in progress: zooming out reveals more pages.
    const extra = this.pinch ? 2 : 1;
    const mountFrom = this.pageAtY(Math.max(0, top - vh * extra));
    const mountTo = this.lastPageAtY(top + vh * (extra + 1));

    for (const [i, pv] of this.mounted) {
      if (i < mountFrom || i > mountTo) {
        this.dropPage(pv);
        this.mounted.delete(i);
      }
    }
    for (let i = mountFrom; i <= mountTo; i++) if (!this.mounted.has(i)) this.mount(i);

    // Current page: the one covering the upper part of the viewport.
    const cur = this.pageAtY(top + Math.min(vh * 0.35, 200));
    if (cur !== this.lastPage) {
      this.lastPage = cur;
      this.cb.onPage(cur);
    }

    // Scroll velocity drives how far ahead previews are prefetched.
    const now = performance.now();
    const dt = now - this.lastT;
    this.velocity = dt > 0 && dt < 250 ? 0.6 * this.velocity + 0.4 * ((top - this.lastTop) / dt) : 0;
    this.lastTop = top;
    this.lastT = now;
    const pageH = this.contentH / n;
    const ahead = Math.min(40, Math.round((Math.abs(this.velocity) * 500) / Math.max(1, pageH)));

    // Previews go out every frame so nothing is blank. Sharp renders are
    // throttled while scrolling (every 100 ms) and debounced while zooming,
    // so intermediate zoom levels don't get rendered for nothing.
    this.requestRenders(first, last, mountFrom, mountTo, false, ahead * Math.sign(this.velocity));
    clearTimeout(this.renderTimer);
    if (this.pinch || this.animating) return;
    const zooming = now - this.zoomChangedAt < 150;
    const delay = immediate ? 0 : zooming ? 120 : Math.max(0, 100 - (now - this.lastSharpAt));
    this.renderTimer = window.setTimeout(() => {
      this.lastSharpAt = performance.now();
      this.requestRenders(first, last, mountFrom, mountTo, true, 0);
    }, delay);
  }

  private mount(i: number) {
    const spec = this.pages[i];
    const el = document.createElement("div");
    el.className = "pg";
    el.dataset.index = String(i);
    const lo = document.createElement("canvas");
    lo.className = "pg-lo";
    const hi = document.createElement("canvas");
    hi.className = "pg-hi";
    const tiles = document.createElement("div");
    tiles.className = "pg-tiles";
    const marks = document.createElement("div");
    marks.className = "pg-marks";
    const links = document.createElement("div");
    links.className = "pg-links";
    lo.style.visibility = hi.style.visibility = "hidden";
    const layerBox = document.createElement("div");
    layerBox.className = "pg-layers";
    el.append(lo, hi, tiles, marks, links, layerBox, this.pageControls(i));
    const pv: PageView = {
      index: i, spec, el, lo, hi, tiles, marks, links, hiScale: 0, tileKeys: new Map(), tileScale: 0, linksLoaded: false,
      layerBox, layerEls: new Map(),
    };
    this.mounted.set(i, pv);
    this.place(pv);
    this.sizer.appendChild(el);
    this.drawMarks(pv);
    for (const l of this.layers) this.mountLayer(pv, l);
  }

  /** Rotate / delete buttons that appear when hovering a page. */
  private pageControls(i: number) {
    const bar = document.createElement("div");
    bar.className = "pg-ctl";
    for (const action of ["rotate-left", "rotate-right", "delete"] as PageAction[]) {
      const b = document.createElement("button");
      b.className = "pg-ctl-btn";
      b.title = TITLES[action];
      b.setAttribute("aria-label", TITLES[action]);
      b.innerHTML = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round">${ICONS[action]}</svg>`;
      b.addEventListener("pointerdown", (e) => e.stopPropagation());
      b.addEventListener("click", (e) => {
        e.stopPropagation();
        this.cb.onPageAction(i, action);
      });
      bar.appendChild(b);
    }
    return bar;
  }

  private place(pv: PageView) {
    const r = this.pageRect(pv.index);
    const st = pv.el.style;
    st.transform = `translate(${r.x}px, ${r.y}px)`;
    st.width = `${r.w}px`;
    st.height = `${r.h}px`;
    this.placeLayers(pv);
  }

  // ------------------------------------------------------------ layers

  /** Adds a layer over every page (see PageLayer). */
  addLayer(layer: PageLayer) {
    if (this.layers.includes(layer)) return;
    this.layers.push(layer);
    this.layers.sort((a, b) => (a.z ?? 0) - (b.z ?? 0));
    for (const pv of this.mounted.values()) this.mountLayer(pv, layer);
  }

  removeLayer(layer: PageLayer) {
    this.layers = this.layers.filter((l) => l !== layer);
    for (const pv of this.mounted.values()) {
      const el = pv.layerEls.get(layer);
      if (!el) continue;
      layer.unmount?.(this.layerPage(pv), el);
      el.remove();
      pv.layerEls.delete(layer);
    }
  }

  /** Rebuilds a layer's elements on every page in view (its content changed). */
  refreshLayer(layer: PageLayer) {
    for (const pv of this.mounted.values()) {
      const el = pv.layerEls.get(layer);
      if (el) {
        layer.unmount?.(this.layerPage(pv), el);
        el.remove();
        pv.layerEls.delete(layer);
      }
      this.mountLayer(pv, layer);
    }
  }

  private layerPage(pv: PageView): LayerPage {
    const [w, h] = origSize(pv.spec);
    return { view: pv.index, src: pv.spec.src, w, h };
  }

  private mountLayer(pv: PageView, layer: PageLayer) {
    const el = layer.mount(this.layerPage(pv));
    if (!el) return;
    el.classList.add("pg-layer");
    el.style.zIndex = String(layer.z ?? 0);
    pv.layerEls.set(layer, el);
    pv.layerBox.append(el);
  }

  private unmountLayers(pv: PageView) {
    for (const [layer, el] of pv.layerEls) layer.unmount?.(this.layerPage(pv), el);
    pv.layerEls.clear();
  }

  /** Takes a page off screen for good, freeing its canvases right away (see discard). */
  private dropPage(pv: PageView) {
    this.unmountLayers(pv);
    discard(pv.lo, pv.hi, pv.tiles, pv.photos);
    pv.el.remove();
  }

  /** Scales and rotates the layer box so page points land on the displayed page. */
  private placeLayers(pv: PageView) {
    const s = this.cssScale;
    const [W, H] = origSize(pv.spec);
    const st = pv.layerBox.style;
    st.width = `${W}px`;
    st.height = `${H}px`;
    st.setProperty("--pg-scale", String(s));
    const rot = ((pv.spec.rot % 360) + 360) % 360;
    const move = rot === 90 ? `translate(${H * s}px, 0) ` : rot === 180 ? `translate(${W * s}px, ${H * s}px) ` : rot === 270 ? `translate(0, ${W * s}px) ` : "";
    st.transform = `${move}rotate(${rot}deg) scale(${s})`;
  }

  private key(spec: PageSpec, kind: string, scale: number, extra = "") {
    return renderKey(this.info.docId, spec.src, spec.rot, kind, scale, extra + (this.fieldsDrawn ? "~nf" : ""));
  }

  /** Renders for the page view leave out fillable fields (the form layer draws them as inputs). */
  private get extras() {
    return this.fieldsDrawn ? ("nofields" as const) : undefined;
  }

  /** The app draws the form fields itself: render pages without them. */
  setFieldsDrawn(on: boolean) {
    if (on === this.fieldsDrawn) return;
    this.fieldsDrawn = on;
    this.schedule(true);
  }

  private requestRenders(first: number, last: number, from: number, to: number, sharp: boolean, ahead: number) {
    if (this.destroyed) return;
    const wants: Want[] = [];
    const sharpWants: Want[] = [];
    const doc = this.info.docId;
    const center = (first + last) / 2;
    const deviceScale = this.cssScale * this.dpr;

    for (let i = from; i <= to; i++) {
      const pv = this.mounted.get(i);
      if (!pv) continue;
      const spec = pv.spec;
      const visible = i >= first && i <= last;
      const dist = Math.abs(i - center);

      // Low-res placeholder (shared with thumbnails).
      const loKey = this.key(spec, "lo", LO_SCALE);
      if (pv.loKey !== loKey && !pv.hiKey) {
        wants.push({
          key: loKey,
          params: { doc, page: spec.src, scale: LO_SCALE, rotate: spec.rot, extras: this.extras },
          prio: (visible ? 1200 : 700) - dist,
          onReady: (b) => {
            if (this.mounted.get(i) !== pv) return;
            pv.loKey = loKey;
            drawBitmap(pv.lo, b);
            pv.loImage = b;
            if (this.night && !pv.hiImage) this.drawPhotos(pv);
            this.stats.lo++;
            if (!this.stats.firstLo) this.stats.firstLo = performance.now() - this.stats.created;
          },
        });
      }
      if (!sharp) continue;
      if (!visible && Math.abs(i - center) > (last - first) / 2 + 1.5) continue;

      const fullPx = spec.w * deviceScale * spec.h * deviceScale;
      if (fullPx <= MAX_FULL_PX) {
        this.clearTiles(pv);
        const s = quantize(deviceScale);
        const hiKey = this.key(spec, "hi", s);
        if (pv.hiKey !== hiKey) {
          sharpWants.push({
            key: hiKey,
            params: { doc, page: spec.src, scale: s, rotate: spec.rot, extras: this.extras },
            prio: (visible ? 1000 : 400) - dist,
            onReady: (b) => {
              if (this.mounted.get(i) !== pv) return;
              pv.hiKey = hiKey;
              pv.hiScale = s;
              drawBitmap(pv.hi, b);
              releaseCanvas(pv.lo);
              pv.loKey = undefined;
              pv.hiImage = b;
              pv.loImage = undefined;
              if (this.night) this.drawPhotos(pv);
              this.stats.hi++;
              if (!this.stats.firstHi) this.stats.firstHi = performance.now() - this.stats.created;
            },
          });
        }
      } else if (visible) {
        this.wantTiles(pv, deviceScale, sharpWants, dist);
      }
    }
    // Prefetch previews ahead of a fast scroll (bitmaps wait in the cache).
    for (let k = 1; k <= Math.abs(ahead); k++) {
      const i = ahead > 0 ? to + k : from - k;
      if (i < 0 || i >= this.pages.length) break;
      const spec = this.pages[i];
      wants.push({ key: this.key(spec, "lo", LO_SCALE), params: { doc, page: spec.src, scale: LO_SCALE, rotate: spec.rot, extras: this.extras }, prio: 650 - k, onReady: () => {} });
    }
    // Keep sharp renders already requested alive between sharp passes, as long
    // as their page is still near and the zoom hasn't changed.
    const q = quantize(deviceScale);
    if (sharp) {
      this.sharpWants = sharpWants;
      this.sharpScale = q;
    } else if (this.sharpScale === q) {
      for (const w of this.sharpWants) {
        const v = this.srcToView.get(w.params.page);
        if (v !== undefined && v >= from && v <= to) wants.push(w);
      }
    }
    renderer.sync(this.group, sharp ? [...wants, ...sharpWants] : wants);
    for (let i = first; i <= last; i++) {
      const pv = this.mounted.get(i);
      if (pv && !pv.linksLoaded) this.loadLinks(pv);
    }
  }

  private clearTiles(pv: PageView) {
    if (!pv.tileKeys.size) return;
    discard(...pv.tiles.children);
    pv.tileKeys.clear();
    pv.tileScale = 0;
  }

  private wantTiles(pv: PageView, deviceScale: number, wants: Want[], dist: number) {
    const i = pv.index;
    const spec = pv.spec;
    const s = quantize(deviceScale);
    const pw = Math.round(spec.w * s);
    const ph = Math.round(spec.h * s);
    const r = this.pageRect(i);
    const dpr = this.dpr;
    // Visible part of the page in device px (plus a margin).
    const m = 256;
    const vx0 = Math.max(0, (this.el.scrollLeft - r.x) * dpr - m);
    const vy0 = Math.max(0, (this.el.scrollTop - r.y) * dpr - m);
    const vx1 = Math.min(pw, (this.el.scrollLeft + this.el.clientWidth - r.x) * dpr + m);
    const vy1 = Math.min(ph, (this.el.scrollTop + this.el.clientHeight - r.y) * dpr + m);
    if (pv.tileScale !== s) {
      // New zoom level: old tiles stay until each new one lands on top.
      pv.tileScale = s;
      for (const c of pv.tiles.children) (c as HTMLElement).dataset.stale = "1";
      pv.tileKeys.clear();
    }
    // Keep the full-page image as a base under the tiles, at the largest size allowed.
    const baseScale = quantize(Math.sqrt(MAX_FULL_PX / (spec.w * spec.h)));
    const baseKey = this.key(spec, "hi", baseScale);
    if (pv.hiKey !== baseKey && pv.hiScale < baseScale) {
      wants.push({
        key: baseKey,
        params: { doc: this.info.docId, page: spec.src, scale: baseScale, rotate: spec.rot, extras: this.extras },
        prio: 950 - dist,
        onReady: (b) => {
          if (this.mounted.get(i) !== pv) return;
          pv.hiKey = baseKey;
          pv.hiScale = baseScale;
          drawBitmap(pv.hi, b);
          pv.hiImage = b;
          if (this.night) this.drawPhotos(pv);
        },
      });
    }
    for (let ty = Math.floor(vy0 / TILE) * TILE; ty < vy1; ty += TILE) {
      for (let tx = Math.floor(vx0 / TILE) * TILE; tx < vx1; tx += TILE) {
        const clip: [number, number, number, number] = [tx, ty, Math.min(pw, tx + TILE), Math.min(ph, ty + TILE)];
        const key = this.key(spec, "tile", s, `@${tx},${ty}`);
        if (pv.tileKeys.has(key)) continue;
        wants.push({
          key,
          params: { doc: this.info.docId, page: spec.src, scale: s, rotate: spec.rot, clip, extras: this.extras },
          prio: 900 - dist,
          onReady: (b) => {
            if (this.mounted.get(i) !== pv || pv.tileScale !== s || pv.tileKeys.has(key)) return;
            const c = document.createElement("canvas");
            c.className = "pg-tile";
            c.style.left = `${(clip[0] / pw) * 100}%`;
            c.style.top = `${(clip[1] / ph) * 100}%`;
            c.style.width = `${((clip[2] - clip[0]) / pw) * 100}%`;
            c.style.height = `${((clip[3] - clip[1]) / ph) * 100}%`;
            drawBitmap(c, b);
            pv.tiles.appendChild(c);
            this.stats.tiles++;
            pv.tileKeys.set(key, c);
            // Drop stale tiles once the new level covers the view.
            if (pv.tiles.querySelector("[data-stale]") && pv.tileKeys.size >= 2) {
              discard(...pv.tiles.querySelectorAll("[data-stale]"));
            }
          },
        });
      }
    }
  }

  private async loadLinks(pv: PageView) {
    pv.linksLoaded = true;
    const src = pv.spec.src;
    let links = this.links.get(src);
    if (!links) {
      links = await api.links(this.info.docId, src).catch(() => [] as LinkItem[]);
      this.links.set(src, links);
    }
    if (this.mounted.get(pv.index) !== pv) return;
    const spec = pv.spec;
    pv.links.replaceChildren(
      ...links.map((l) => {
        const q = [l.r[0], l.r[1], l.r[2], l.r[1], l.r[0], l.r[3], l.r[2], l.r[3]];
        const [x0, y0, x1, y1] = quadBox(spec, q);
        const a = document.createElement("a");
        a.className = "pg-link";
        a.style.left = `${(x0 / spec.w) * 100}%`;
        a.style.top = `${(y0 / spec.h) * 100}%`;
        a.style.width = `${((x1 - x0) / spec.w) * 100}%`;
        a.style.height = `${((y1 - y0) / spec.h) * 100}%`;
        a.title = l.u ?? `Page ${(l.p ?? 0) + 1}`;
        a.href = "#";
        a.addEventListener("click", (e) => {
          e.preventDefault();
          if (l.u) openUrl(l.u).catch(() => {});
          else if (l.p !== undefined && l.p >= 0) this.goToSource(l.p, l.y);
        });
        return a;
      }),
    );
  }

  /**
   * Night mode: draws the page's photos from the unfiltered bitmap on top of the
   * inverted page, so pictures keep their colours. Pages that are mostly one
   * big image (scans) are left fully inverted.
   */
  private drawPhotos(pv: PageView) {
    discard(pv.photos);
    pv.photos = undefined;
    const bmp = pv.hiImage ?? pv.loImage;
    if (!this.night || !bmp) return;
    const spec = pv.spec;
    const rects = this.imageRects.get(spec.src);
    if (rects === undefined) {
      this.imageRects.set(spec.src, "loading");
      api
        .images(this.info.docId, spec.src)
        .then((r) => {
          this.imageRects.set(spec.src, r);
          if (this.mounted.get(pv.index) === pv) this.drawPhotos(pv);
        })
        .catch(() => this.imageRects.set(spec.src, []));
      return;
    }
    if (rects === "loading" || !rects.length) return;
    const [W, H] = origSize(spec);
    const area = rects.reduce((a, r) => a + Math.max(0, r[2] - r[0]) * Math.max(0, r[3] - r[1]), 0);
    // Bitmap boxes [x, y, w, h] to show in their real colours.
    const boxes: [number, number, number, number][] = [];
    if (area > 0.6 * W * H) {
      // A scan (or a page that is one big picture, like a screenshot): its text
      // stays inverted, and the photos in it are found in the bitmap itself.
      const key = `${spec.src}:${spec.rot}`;
      let found = this.scanPhotos.get(key);
      if (!found) {
        // Small text looks like grey mush at low resolution: wait for a sharp render.
        if (bmp.scale < 0.9) return;
        found = findPhotos(bmp.image);
        this.scanPhotos.set(key, found);
      }
      for (const [f0, g0, f1, g1] of found) {
        const sx = Math.floor(f0 * bmp.width), sy = Math.floor(g0 * bmp.height);
        boxes.push([sx, sy, Math.ceil(f1 * bmp.width) - sx, Math.ceil(g1 * bmp.height) - sy]);
      }
    } else {
      const k = bmp.width / spec.w; // bitmap px per displayed point
      for (const r of rects) {
        const q = [r[0], r[1], r[2], r[1], r[0], r[3], r[2], r[3]];
        const [x0, y0, x1, y1] = quadBox(spec, q);
        const sx = Math.max(0, Math.floor(x0 * k)), sy = Math.max(0, Math.floor(y0 * k));
        boxes.push([sx, sy, Math.min(bmp.width, Math.ceil(x1 * k)) - sx, Math.min(bmp.height, Math.ceil(y1 * k)) - sy]);
      }
    }
    if (!boxes.length) return;
    const layer = document.createElement("div");
    layer.className = "pg-photos";
    for (const [sx, sy, sw, sh] of boxes) {
      if (sw < 2 || sh < 2) continue;
      const c = document.createElement("canvas");
      c.width = sw;
      c.height = sh;
      c.style.left = `${(sx / bmp.width) * 100}%`;
      c.style.top = `${(sy / bmp.height) * 100}%`;
      c.style.width = `${(sw / bmp.width) * 100}%`;
      c.style.height = `${(sh / bmp.height) * 100}%`;
      c.getContext("2d", { alpha: false, willReadFrequently: true })!.putImageData(bmp.image, -sx, -sy, sx, sy, sw, sh);
      layer.appendChild(c);
    }
    pv.tiles.after(layer);
    pv.photos = layer;
  }

  private drawMarks(pv: PageView) {
    const spec = pv.spec;
    const boxes: HTMLElement[] = [];
    const box = (q: ArrayLike<number>, cls: string) => {
      const [x0, y0, x1, y1] = quadBox(spec, q);
      const d = document.createElement("div");
      d.className = cls;
      d.style.left = `${(x0 / spec.w) * 100}%`;
      d.style.top = `${(y0 / spec.h) * 100}%`;
      d.style.width = `${((x1 - x0) / spec.w) * 100}%`;
      d.style.height = `${((y1 - y0) / spec.h) * 100}%`;
      boxes.push(d);
    };
    const hits = this.search.get(spec.src);
    if (hits) {
      hits.forEach((hit, hi) => {
        const active = this.activeHit?.page === spec.src && this.activeHit.index === hi;
        for (let k = 0; k + 7 < hit.length; k += 8) box(hit.slice(k, k + 8), active ? "mark-hit active" : "mark-hit");
      });
    }
    if (this.selection?.view === pv.index) for (const q of this.selQuads) box(q, "mark-sel");
    pv.marks.replaceChildren(...boxes);
  }
}

/**
 * Photos inside a scanned page, as boxes [x0, y0, x1, y1] in fractions of the
 * bitmap. The page is cut into small cells: a photo's cells are mostly
 * mid-tones or colour, while text on paper is mostly paper and ink (black and
 * white with a few grey edge pixels). Neighbouring photo cells are joined into
 * regions; small or ragged ones (a coloured word, a logo) are dropped.
 */
export function findPhotos(img: ImageData): [number, number, number, number][] {
  const { width: w, height: h, data } = img;
  const cell = Math.max(6, Math.round(w / 64));
  const cols = Math.ceil(w / cell), rows = Math.ceil(h / cell);
  const step = Math.max(1, Math.floor(cell / 6));
  const photo = new Uint8Array(cols * rows);
  for (let cy = 0; cy < rows; cy++) {
    for (let cx = 0; cx < cols; cx++) {
      let n = 0, mid = 0, sat = 0;
      const y1 = Math.min(h, (cy + 1) * cell), x1 = Math.min(w, (cx + 1) * cell);
      for (let y = cy * cell; y < y1; y += step) {
        for (let x = cx * cell; x < x1; x += step) {
          const i = (y * w + x) * 4;
          const r = data[i], g = data[i + 1], b = data[i + 2];
          const lum = (r * 299 + g * 587 + b * 114) / 1000;
          n++;
          if (lum > 45 && lum < 210) mid++;
          sat += Math.max(r, g, b) - Math.min(r, g, b);
        }
      }
      const midF = mid / n, satF = sat / n / 255;
      photo[cy * cols + cx] = midF > 0.55 || (satF > 0.2 && midF > 0.3) ? 1 : 0;
    }
  }
  // Close small gaps (very dark or bright patches in a photo) before joining cells.
  const grown = new Uint8Array(photo);
  for (let cy = 0; cy < rows; cy++)
    for (let cx = 0; cx < cols; cx++)
      if (!photo[cy * cols + cx]) {
        const l = cx > 0 && photo[cy * cols + cx - 1], r = cx < cols - 1 && photo[cy * cols + cx + 1];
        const t = cy > 0 && photo[(cy - 1) * cols + cx], b = cy < rows - 1 && photo[(cy + 1) * cols + cx];
        if ((l && r) || (t && b)) grown[cy * cols + cx] = 1;
      }
  const seen = new Uint8Array(cols * rows);
  const out: [number, number, number, number][] = [];
  const stack: number[] = [];
  for (let start = 0; start < cols * rows; start++) {
    if (!grown[start] || seen[start]) continue;
    let x0 = cols, y0 = rows, x1 = -1, y1 = -1, count = 0;
    seen[start] = 1;
    stack.push(start);
    while (stack.length) {
      const c = stack.pop()!;
      const cx = c % cols, cy = (c - cx) / cols;
      count++;
      x0 = Math.min(x0, cx); x1 = Math.max(x1, cx); y0 = Math.min(y0, cy); y1 = Math.max(y1, cy);
      for (const d of [c - 1, c + 1, c - cols, c + cols]) {
        if (d < 0 || d >= cols * rows || seen[d] || !grown[d]) continue;
        if ((d === c - 1 && cx === 0) || (d === c + 1 && cx === cols - 1)) continue;
        seen[d] = 1;
        stack.push(d);
      }
    }
    const boxCells = (x1 - x0 + 1) * (y1 - y0 + 1);
    if (count < 12 || count < boxCells * 0.5) continue;
    const box = trimPaper(img, x0 * cell, y0 * cell, Math.min(w, (x1 + 1) * cell), Math.min(h, (y1 + 1) * cell), cell);
    out.push([box[0] / w, box[1] / h, box[2] / w, box[3] / h]);
  }
  return out;
}

/** Moves a box's edges inwards (at most `limit` px) past rows and columns that are nearly all paper. */
function trimPaper(img: ImageData, x0: number, y0: number, x1: number, y1: number, limit: number): [number, number, number, number] {
  const { width: w, data } = img;
  const paper = (x: number, y: number) => {
    const i = (y * w + x) * 4;
    return data[i] > 225 && data[i + 1] > 225 && data[i + 2] > 225;
  };
  const rowPaper = (y: number) => {
    let n = 0;
    for (let x = x0; x < x1; x++) n += paper(x, y) ? 1 : 0;
    return n > (x1 - x0) * 0.9;
  };
  const colPaper = (x: number) => {
    let n = 0;
    for (let y = y0; y < y1; y++) n += paper(x, y) ? 1 : 0;
    return n > (y1 - y0) * 0.9;
  };
  for (let k = 0; k < limit && y1 - y0 > 2 && rowPaper(y0); k++) y0++;
  for (let k = 0; k < limit && y1 - y0 > 2 && rowPaper(y1 - 1); k++) y1--;
  for (let k = 0; k < limit && x1 - x0 > 2 && colPaper(x0); k++) x0++;
  for (let k = 0; k < limit && x1 - x0 > 2 && colPaper(x1 - 1); k++) x1--;
  return [x0, y0, x1, y1];
}

function clampZoom(z: number) {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z));
}

/** Render scales are exact (to 0.1%) so text stays pixel-sharp; the settle delay avoids re-rendering mid-zoom. */
function quantize(s: number) {
  return Math.max(0.05, Math.round(s * 1000) / 1000);
}
