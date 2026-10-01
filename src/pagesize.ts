// Page size tool: crop and resize pages. The crop box is kept as margins of the
// page as it's shown (so a ratio lock means what you see even on turned pages)
// and drawn on the current page by a viewer layer.
import { effect, signal } from "@preact/signals";
import { api } from "./api";
import { parsePages } from "./ocr";
import * as P from "./pages";
import type { LayerPage, PageLayer } from "./render/Viewer";
import { settings, showToast, updateSettings, type Tab } from "./store";

export type Box = [number, number, number, number];
/** Margins of the shown page: left, top, right, bottom (points). */
export type Margins = [number, number, number, number];

const MM = 72 / 25.4;

/** Crop ratios and page sizes (points, portrait). A ratio-only entry has no page size. */
export const PRESETS: { key: string; label: string; group: string; w: number; h: number; ratioOnly?: true }[] = [
  { key: "A3", label: "A3", group: "Paper", w: 297 * MM, h: 420 * MM },
  { key: "A4", label: "A4", group: "Paper", w: 595, h: 842 },
  { key: "A5", label: "A5", group: "Paper", w: 148 * MM, h: 210 * MM },
  { key: "A6", label: "A6", group: "Paper", w: 105 * MM, h: 148 * MM },
  { key: "B5", label: "B5", group: "Paper", w: 176 * MM, h: 250 * MM },
  { key: "Letter", label: "Letter", group: "Paper", w: 612, h: 792 },
  { key: "Legal", label: "Legal", group: "Paper", w: 612, h: 1008 },
  { key: "Tabloid", label: "Tabloid", group: "Paper", w: 792, h: 1224 },
  { key: "passport", label: "Passport photo (35 × 45 mm)", group: "Photo", w: 35 * MM, h: 45 * MM },
  { key: "4x6", label: "4 × 6 in", group: "Photo", w: 288, h: 432 },
  { key: "5x7", label: "5 × 7 in", group: "Photo", w: 360, h: 504 },
  { key: "1:1", label: "1:1 (square)", group: "Screen", w: 1, h: 1, ratioOnly: true },
  { key: "4:5", label: "4:5", group: "Screen", w: 4, h: 5, ratioOnly: true },
  { key: "2:3", label: "2:3", group: "Screen", w: 2, h: 3, ratioOnly: true },
  { key: "3:4", label: "3:4", group: "Screen", w: 3, h: 4, ratioOnly: true },
  { key: "9:16", label: "9:16", group: "Screen", w: 9, h: 16, ratioOnly: true },
];
export const presetOf = (key: string) => PRESETS.find((p) => p.key === key);

export type Unit = "mm" | "in" | "pt";
export const UNIT: Record<Unit, number> = { mm: MM, in: 72, pt: 1 };

export interface PageSizeSettings {
  /** "free", "original" or a preset key. */
  ratio: string;
  landscape: boolean;
  unit: Unit;
  remove: boolean;
  resize: boolean;
  /** A preset key with a page size, or "custom". */
  target: string;
  custom: [number, number];
  fit: 0 | 1 | 2;
  anchor: [number, number];
  /** New space: nothing, a colour, the edges stretched, the page's own paper. */
  fill: 0 | 1 | 2 | 3;
  color: string;
}

export const PAGE_SIZE_DEFAULTS: PageSizeSettings = {
  ratio: "free", landscape: false, unit: "mm", remove: false, resize: false, target: "A4", custom: [595, 842],
  fit: 0, anchor: [0.5, 0.5], fill: 1, color: "#ffffff",
};

export const psSettings = () => ({ ...PAGE_SIZE_DEFAULTS, ...settings.value.pageSize });
export const setPs = (patch: Partial<PageSizeSettings>) => updateSettings({ pageSize: { ...psSettings(), ...patch } });

export type Scope = "page" | "selected" | "all" | "odd" | "even" | "range";

/** The tool: which tab, the crop margins (of the page as shown), and what it applies to. */
export const pageSizeEdit = signal<{ tab: Tab; margins: Margins; scope: Scope; range: string } | null>(null);

// ---------------------------------------------------------------- geometry

/** A page's size in points before and after its extra rotation, and that rotation. */
export function pageGeom(tab: Tab, view: number) {
  const e = tab.edits.value;
  const src = e?.order[view] ?? view;
  const [w, h] = tab.sizes.value?.[src] ?? tab.info.value?.firstSize ?? [612, 792];
  const rot = (e?.rot[src] ?? 0) % 360;
  const [W, H] = rot % 180 ? [h, w] : [w, h];
  return { src, w, h, rot, W, H };
}

/** Shown margins → crop box of the unturned page. */
export function toCrop(m: Margins, w: number, h: number, rot: number): Box {
  const [l, t, r, b] = m;
  const [L, T, R, B] = rot === 90 ? [t, r, b, l] : rot === 180 ? [r, b, l, t] : rot === 270 ? [b, l, t, r] : [l, t, r, b];
  return [L, T, w - R, h - B];
}

/** Crop box of the unturned page → shown margins. */
export function toMargins(c: Box, w: number, h: number, rot: number): Margins {
  const [L, T, R, B] = [c[0], c[1], w - c[2], h - c[3]];
  return rot === 90 ? [B, L, T, R] : rot === 180 ? [R, B, L, T] : rot === 270 ? [T, R, B, L] : [L, T, R, B];
}

/** A point of the unturned page → the page as shown. */
function shownPoint(x: number, y: number, w: number, h: number, rot: number): [number, number] {
  return rot === 90 ? [h - y, x] : rot === 180 ? [w - x, h - y] : rot === 270 ? [y, w - x] : [x, y];
}

/** The crop ratio (shown width / height), or null for none. */
export function cropRatio(tab: Tab): number | null {
  const s = psSettings();
  if (s.ratio === "free") return null;
  if (s.ratio === "original") {
    const g = pageGeom(tab, tab.page.value);
    return g.W / g.H;
  }
  const p = presetOf(s.ratio);
  if (!p) return null;
  const r = p.w / p.h;
  return s.landscape ? 1 / r : r;
}

/** The biggest box of ratio r centred on the page (shown), as margins. */
export function fitRatio(W: number, H: number, r: number | null): Margins {
  if (!r) return [0, 0, 0, 0];
  let w = W, h = W / r;
  if (h > H) {
    h = H;
    w = H * r;
  }
  return [(W - w) / 2, (H - h) / 2, (W - w) / 2, (H - h) / 2];
}

const MIN = 12;

/** Keeps margins on the page and the box at least MIN points each way. */
function clampMargins(m: Margins, W: number, H: number): Margins {
  let [l, t, r, b] = m.map((v) => Math.max(0, v)) as Margins;
  if (W - l - r < MIN) r = Math.max(0, W - l - MIN);
  if (W - l - r < MIN) l = Math.max(0, W - r - MIN);
  if (H - t - b < MIN) b = Math.max(0, H - t - MIN);
  if (H - t - b < MIN) t = Math.max(0, H - b - MIN);
  return [l, t, r, b];
}

// ---------------------------------------------------------------- starting and applying

let layer: CropLayer | null = null;

export function startPageSize(tab: Tab) {
  if (!tab.info.value?.isPdf) return showToast("Only PDF pages can be cropped or resized. Save this file as a PDF first (Save as).", 4500);
  tab.grid.value = false;
  if (tab.mode.value !== "pages") return showToast("Switch to Single or Double view to crop or resize pages.", 4000);
  const g = pageGeom(tab, tab.page.value);
  const sel = tab.pageSel.value;
  pageSizeEdit.value = { tab, margins: fitRatio(g.W, g.H, cropRatio(tab)), scope: sel.size > 1 ? "selected" : "page", range: "" };
  requestAnimationFrame(() => {
    if (!tab.viewer || pageSizeEdit.value?.tab !== tab) return;
    layer = new CropLayer(tab);
    tab.viewer.addLayer(layer);
    // The whole page in view, so every edge of the box can be reached.
    tab.viewer.setFit("page");
  });
}

export function endPageSize() {
  const st = pageSizeEdit.value;
  if (layer && st) st.tab.viewer?.removeLayer(layer);
  layer = null;
  pageSizeEdit.value = null;
}

export function setMargins(m: Margins) {
  const st = pageSizeEdit.value;
  if (!st) return;
  const g = pageGeom(st.tab, st.tab.page.value);
  pageSizeEdit.value = { ...st, margins: clampMargins(m, g.W, g.H) };
}

/** The crop box at the page's content (what's drawn on it), with a little room. */
export async function fitToContent() {
  const st = pageSizeEdit.value;
  if (!st) return;
  const g = pageGeom(st.tab, st.tab.page.value);
  const box = await api.contentBox(st.tab.info.value!.docId, g.src).catch(() => null);
  if (!box) return showToast("This page is blank.");
  const pad = 6;
  const c: Box = [Math.max(0, box[0] - pad), Math.max(0, box[1] - pad), Math.min(g.w, box[2] + pad), Math.min(g.h, box[3] + pad)];
  setPs({ ratio: "free" });
  setMargins(toMargins(c, g.w, g.h, g.rot));
}

/** Display indices the tool applies to, or an error message. */
export function scopeViews(tab: Tab, scope: Scope, range: string): number[] | string {
  const n = tab.edits.value?.order.length ?? 0;
  const all = Array.from({ length: n }, (_, i) => i);
  switch (scope) {
    case "page":
      return [tab.page.value];
    case "selected":
      return tab.pageSel.value.size ? [...tab.pageSel.value].sort((a, b) => a - b) : [tab.page.value];
    case "all":
      return all;
    case "odd":
      return all.filter((i) => i % 2 === 0);
    case "even":
      return all.filter((i) => i % 2 === 1);
    case "range":
      return parsePages(range, n) ?? `Pages must be numbers from 1 to ${n}, like 1-5, 8.`;
  }
}

/** The new page size (shown), turned like the crop box; null when not resizing. */
export function targetSize(m: Margins, W: number, H: number): [number, number] | null {
  const s = psSettings();
  if (!s.resize) return null;
  const p = s.target === "custom" ? { w: s.custom[0], h: s.custom[1] } : presetOf(s.target);
  if (!p) return null;
  const landscape = W - m[0] - m[2] > H - m[1] - m[3];
  const [a, b] = p.w < p.h ? [p.w, p.h] : [p.h, p.w];
  return s.target === "custom" ? [p.w, p.h] : landscape ? [b, a] : [a, b];
}

function hexColor(hex: string): [number, number, number] {
  const n = parseInt(hex.replace("#", ""), 16) || 0;
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
}

/** Crops / resizes the pages in scope (same margins on each) as one undo step. */
export async function applyPageSize(): Promise<string | null> {
  const st = pageSizeEdit.value;
  if (!st) return null;
  const tab = st.tab;
  const views = scopeViews(tab, st.scope, st.range);
  if (typeof views === "string") return views;
  const s = psSettings();
  const ops = views.map((v) => {
    const g = pageGeom(tab, v);
    const m = clampMargins(st.margins, g.W, g.H);
    const size = targetSize(m, g.W, g.H);
    return { page: g.src, crop: toCrop(m, g.w, g.h, g.rot), size: size ? ((g.rot % 180 ? [size[1], size[0]] : size) as [number, number]) : undefined };
  });
  const whole = !s.resize && st.margins.every((x) => x < 0.5);
  if (whole) return "The crop box covers the whole page: drag its edges, or turn on Change the page size.";
  try {
    await P.resizePages(tab, ops, { fit: s.fit, anchor: s.anchor, remove: s.remove, fill: s.resize ? s.fill : 0, color: hexColor(s.color) });
  } catch (e) {
    return `Couldn't change the pages: ${e}`;
  }
  endPageSize();
  const what = s.resize ? (st.margins.some((x) => x > 0.5) ? "Cropped and resized" : "Resized") : "Cropped";
  showToast(`${what} ${ops.length === 1 ? "1 page" : `${ops.length} pages`} · Ctrl+Z to undo`, 3500);
  return null;
}

// Leaving the page view (Read/Book mode, another tab, the grid) ends the tool.
effect(() => {
  const st = pageSizeEdit.value;
  if (!st) return;
  if (st.tab.mode.value !== "pages" || st.tab.grid.value || st.tab.status.value !== "ready") endPageSize();
});

// The box follows you to other pages, and redraws when it changes.
effect(() => {
  const st = pageSizeEdit.value;
  if (!st) return;
  void st.tab.page.value;
  void st.tab.edits.value;
  layer?.refresh();
});

// ---------------------------------------------------------------- the crop box on the page

type Handle = "move" | "l" | "r" | "t" | "b" | "tl" | "tr" | "bl" | "br";
const HANDLES: Handle[] = ["tl", "t", "tr", "r", "br", "b", "bl", "l"];

class CropLayer implements PageLayer {
  z = 30;
  private roots = new Map<number, HTMLElement>();
  constructor(private tab: Tab) {}

  mount(page: LayerPage): HTMLElement {
    const root = document.createElement("div");
    root.className = "crop-root";
    this.roots.set(page.view, root);
    this.draw(page.view, root);
    return root;
  }

  unmount(page: LayerPage) {
    this.roots.delete(page.view);
  }

  refresh() {
    for (const [view, root] of this.roots) this.draw(view, root);
  }

  private draw(view: number, root: HTMLElement) {
    root.replaceChildren();
    const st = pageSizeEdit.value;
    if (!st || view !== this.tab.page.value) return;
    const g = pageGeom(this.tab, view);
    const m = clampMargins(st.margins, g.W, g.H);
    const [x0, y0, x1, y1] = toCrop(m, g.w, g.h, g.rot);
    const box = document.createElement("div");
    box.className = "crop-box";
    Object.assign(box.style, { left: `${x0}px`, top: `${y0}px`, width: `${x1 - x0}px`, height: `${y1 - y0}px` });
    box.addEventListener("pointerdown", (e) => this.drag(e, view, "move"));
    const u = psSettings().unit;
    const label = document.createElement("span");
    label.className = "crop-size";
    const f = (v: number) => (v / UNIT[u]).toFixed(u === "in" ? 2 : 0);
    label.textContent = `${f(g.W - m[0] - m[2])} × ${f(g.H - m[1] - m[3])} ${u}`;
    box.append(label);
    for (const h of HANDLES) {
      const el = document.createElement("span");
      el.className = `crop-h crop-${h}`;
      el.addEventListener("pointerdown", (e) => this.drag(e, view, this.shownHandle(h, g, [x0, y0, x1, y1])));
      box.append(el);
    }
    root.append(box);
  }

  /** Which edge a handle is on the page as shown (it turns with the page). */
  private shownHandle(h: Handle, g: ReturnType<typeof pageGeom>, c: Box): Handle {
    const x = h.includes("l") ? c[0] : h.includes("r") ? c[2] : (c[0] + c[2]) / 2;
    const y = h.includes("t") ? c[1] : h.includes("b") ? c[3] : (c[1] + c[3]) / 2;
    const [sx, sy] = shownPoint(x, y, g.w, g.h, g.rot);
    const [a0, b0] = shownPoint(c[0], c[1], g.w, g.h, g.rot);
    const [a1, b1] = shownPoint(c[2], c[3], g.w, g.h, g.rot);
    const [X0, X1, Y0, Y1] = [Math.min(a0, a1), Math.max(a0, a1), Math.min(b0, b1), Math.max(b0, b1)];
    const hx = Math.abs(sx - X0) < 0.5 ? "l" : Math.abs(sx - X1) < 0.5 ? "r" : "";
    const hy = Math.abs(sy - Y0) < 0.5 ? "t" : Math.abs(sy - Y1) < 0.5 ? "b" : "";
    return ((hy + hx) || "move") as Handle;
  }

  private drag(e: PointerEvent, view: number, handle: Handle) {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    const st = pageSizeEdit.value;
    const viewer = this.tab.viewer;
    if (!st || !viewer) return;
    const g = pageGeom(this.tab, view);
    const at = (ev: PointerEvent) => {
      const p = viewer.clientToPage(view, ev.clientX, ev.clientY);
      return p ? shownPoint(p[0], p[1], g.w, g.h, g.rot) : null;
    };
    const start = at(e);
    if (!start) return;
    const [l0, t0, r0, b0] = st.margins;
    const D0: Box = [l0, t0, g.W - r0, g.H - b0];
    const ratio = cropRatio(this.tab);
    const move = (ev: PointerEvent) => {
      const p = at(ev);
      if (!p) return;
      const dx = p[0] - start[0], dy = p[1] - start[1];
      let [x0, y0, x1, y1] = D0;
      if (handle === "move") {
        const w = x1 - x0, h = y1 - y0;
        x0 = Math.max(0, Math.min(g.W - w, x0 + dx));
        y0 = Math.max(0, Math.min(g.H - h, y0 + dy));
        x1 = x0 + w;
        y1 = y0 + h;
      } else {
        if (handle.includes("l")) x0 = Math.max(0, Math.min(x1 - MIN, x0 + dx));
        if (handle.includes("r")) x1 = Math.min(g.W, Math.max(x0 + MIN, x1 + dx));
        if (handle.includes("t")) y0 = Math.max(0, Math.min(y1 - MIN, y0 + dy));
        if (handle.includes("b")) y1 = Math.min(g.H, Math.max(y0 + MIN, y1 + dy));
        if (ratio) [x0, y0, x1, y1] = lockRatio([x0, y0, x1, y1], D0, handle, ratio, g.W, g.H);
      }
      pageSizeEdit.value = { ...pageSizeEdit.value!, margins: [x0, y0, g.W - x1, g.H - y1] };
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }
}

/** Keeps a box at width/height r while one handle moves, inside W×H. */
function lockRatio(b: Box, before: Box, handle: Handle, r: number, W: number, H: number): Box {
  let [x0, y0, x1, y1] = b;
  const horizontal = handle === "l" || handle === "r";
  const vertical = handle === "t" || handle === "b";
  let w = x1 - x0, h = y1 - y0;
  if (horizontal) h = w / r;
  else if (vertical) w = h * r;
  else if (w / h > r) h = w / r;
  else w = h * r;
  // Too big for the page: shrink, keeping the ratio.
  const cx = (before[0] + before[2]) / 2, cy = (before[1] + before[3]) / 2;
  const maxW = horizontal ? W : vertical ? 2 * Math.min(cx, W - cx) : handle.includes("l") ? before[2] : W - before[0];
  const maxH = vertical ? H : horizontal ? 2 * Math.min(cy, H - cy) : handle.includes("t") ? before[3] : H - before[1];
  const k = Math.min(1, maxW / w, maxH / h);
  w *= k;
  h *= k;
  // The side opposite the handle stays; edge handles keep the box centred the other way.
  if (horizontal) {
    if (handle === "l") x0 = x1 - w;
    else x1 = x0 + w;
    y0 = cy - h / 2;
    y1 = cy + h / 2;
  } else if (vertical) {
    if (handle === "t") y0 = y1 - h;
    else y1 = y0 + h;
    x0 = cx - w / 2;
    x1 = cx + w / 2;
  } else {
    [x0, x1] = handle.includes("l") ? [before[2] - w, before[2]] : [before[0], before[0] + w];
    [y0, y1] = handle.includes("t") ? [before[3] - h, before[3]] : [before[1], before[1] + h];
  }
  return [x0, y0, x1, y1];
}
