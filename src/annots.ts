// Annotations: highlights, underlines, pen, notes, text boxes, shapes, stamps.
// They are real PDF annotations made by the engine (so Chrome and Adobe show
// them too) and drawn into the page by it. This layer only handles input:
// drawing new ones with the current tool, and selecting, moving, resizing,
// recolouring and deleting existing ones. Every change is one undo step.
import { effect, signal } from "@preact/signals";
import { api, type Annot, type AnnotProps, type Quad } from "./api";
import { DOC_STEP } from "./edits";
import { formsChanged } from "./forms";
import type { LayerPage, PageLayer } from "./render/Viewer";
import { activeTab, settings, showToast, updateSettings, type Tab } from "./store";

export type AnnotTool =
  | "highlight" | "underline" | "strike" | "area" | "pen" | "note" | "text" | "rect" | "ellipse" | "line" | "arrow"
  | `stamp:${string}`
  | `sig:${string}`
  | `box:${string}`;

/** Click-to-place tools from other modules (saved signatures), by the prefix before ":". */
type Placer = (tab: Tab, src: number, arg: string, at: [number, number]) => unknown;
const placers = new Map<string, Placer>();
export function registerPlacer(prefix: string, fn: Placer) {
  placers.set(prefix, fn);
}

/** Tools that just ask for a box on a page ("box:<name>"), e.g. where a digital signature goes. */
type Boxer = (tab: Tab, src: number, rect: number[]) => unknown;
const boxers = new Map<string, Boxer>();
export function registerBoxer(name: string, fn: Boxer) {
  boxers.set(name, fn);
}

/** The active drawing tool (null: normal selecting and reading). */
export const annotTool = signal<AnnotTool | null>(null);

/** The selected annotation (on the active tab). */
export const selectedAnnot = signal<{ tab: Tab; src: number; a: Annot } | null>(null);

/** An annotation to select as soon as its page shows it (just created, or picked in the comments list). */
let pendingSelect: { tab: Tab; src: number; n: number } | null = null;
export function selectWhenShown(tab: Tab, src: number, n: number) {
  pendingSelect = { tab, src, n };
  layers.get(tab)?.refresh();
}

export const PALETTE = ["#ffd400", "#7ee06a", "#5ec8ff", "#ff7bc4", "#e8453c", "#ff9a1f", "#8f6bff", "#1c1c1c", "#2b58d1"];

export const hexToRgb = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
export const rgbToHex = (c?: number[]) =>
  c ? "#" + c.map((v) => Math.round(Math.max(0, Math.min(1, v)) * 255).toString(16).padStart(2, "0")).join("") : "#000000";

/** Colour and width per kind of tool, remembered. */
export type StyleKind = "hl" | "pen" | "shape" | "text" | "note";
export function styleOf(kind: StyleKind) {
  const s = settings.value.annot;
  return { color: s.colors[kind], width: s.width, fontSize: s.fontSize };
}
export function setStyle(kind: StyleKind, patch: { color?: string; width?: number; fontSize?: number }) {
  const s = settings.value.annot;
  updateSettings({ annot: { ...s, colors: patch.color ? { ...s.colors, [kind]: patch.color } : s.colors, width: patch.width ?? s.width, fontSize: patch.fontSize ?? s.fontSize } });
}
export const toolStyle = (t: AnnotTool | null): StyleKind =>
  t === "highlight" || t === "underline" || t === "strike" || t === "area" ? "hl" : t === "pen" ? "pen" : t === "text" ? "text" : t === "note" ? "note" : "shape";

let author = "";
export function setAuthor(name: string) {
  author = name;
}
export const authorName = () => author;

const layers = new WeakMap<Tab, AnnotLayer>();

/** Sends an annotation change to the engine; one undo step; pages redraw. */
export async function editAnnot(tab: Tab, src: number, req: { num?: number; kind?: string; props?: AnnotProps; delete?: boolean }): Promise<number | null> {
  const info = tab.info.value;
  if (!info?.isPdf) {
    showToast("Comments can be added to PDFs. Save this file as a PDF first (Save as).");
    return null;
  }
  try {
    const r = await api.annotEdit(info.docId, src, { ...req, author });
    tab.history.push(DOC_STEP);
    tab.historyVersion.value++;
    formsChanged(tab, r.changed);
    return r.num;
  } catch (e) {
    showToast(`Couldn't change the annotation: ${e}`);
    return null;
  }
}

export async function deleteSelected() {
  const s = selectedAnnot.value;
  if (!s) return;
  selectedAnnot.value = null;
  await editAnnot(s.tab, s.src, { num: s.a.n, delete: true });
}

/** Called by forms.ts when anything in the document changed: refresh hit areas. */
export function refreshAnnots(tab: Tab) {
  layers.get(tab)?.refresh();
}

// The selection frame follows the selected annotation.
// Only a change of *which* annotation is selected redraws (fresh data for the same one doesn't).
let lastSel: Tab | null = null;
let lastKey = "";
effect(() => {
  const s = selectedAnnot.value;
  const tab = s?.tab ?? activeTab.value;
  const key = s ? `${s.tab.key}|${s.src}|${s.a.n}` : "";
  if (key === lastKey && tab === lastSel) return;
  lastKey = key;
  if (lastSel && lastSel !== tab) layers.get(lastSel)?.redraw();
  if (tab) layers.get(tab)?.redraw();
  lastSel = tab ?? null;
});

// Clicking anywhere else (not the annotation or its bar) deselects it.
window.addEventListener("pointerdown", (e) => {
  if (!selectedAnnot.value) return;
  const t = e.target as HTMLElement;
  if (t.closest(".an-sel, .an-hit, .annotbar, .menu")) return;
  selectedAnnot.value = null;
}, true);

export function attachAnnotLayer(tab: Tab) {
  const v = tab.viewer;
  if (!v || layers.has(tab)) return;
  const layer = new AnnotLayer(tab);
  layers.set(tab, layer);
  v.addLayer(layer);
}

/** Text-markup tools act on the text you select with them. */
export async function markupFromSelection(tab: Tab, src: number, quads: Quad[], a: [number, number], b: [number, number]) {
  const t = annotTool.value;
  if (t !== "highlight" && t !== "underline" && t !== "strike") return;
  const color = hexToRgb(styleOf("hl").color);
  tab.viewer?.clearSelection();
  if (quads.length) {
    const kind = t === "highlight" ? "Highlight" : t === "underline" ? "Underline" : "StrikeOut";
    await editAnnot(tab, src, { kind, props: { quads: quads.flat(), color: t === "highlight" ? color : color.map((c) => c * 0.8) } });
  } else if (t === "highlight") {
    // No text there (a scan): highlight the area you dragged over.
    const r = [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.max(a[0], b[0]), Math.max(a[1], b[1])];
    if (r[2] - r[0] < 4 || r[3] - r[1] < 4) return;
    await editAnnot(tab, src, { kind: "Highlight", props: { quads: [r[0], r[1], r[2], r[1], r[0], r[3], r[2], r[3]], color } });
  }
}

// ---------------------------------------------------------------- the layer

const RECT_TYPES = new Set(["Square", "Circle", "FreeText", "Stamp", "Text", "Ink", "Line", "Polygon", "PolyLine", "FileAttachment", "Sound", "Caret"]);
const RESIZABLE = new Set(["Square", "Circle", "FreeText", "Stamp"]);
const SVG = "http://www.w3.org/2000/svg";

class AnnotLayer implements PageLayer {
  z = 15;
  private roots = new Map<HTMLElement, LayerPage>();
  private cache = new Map<number, Promise<Annot[]>>();

  constructor(private tab: Tab) {}

  private annots(src: number) {
    let p = this.cache.get(src);
    if (!p) {
      p = api.annots(this.tab.info.value!.docId, src).catch(() => []);
      this.cache.set(src, p);
    }
    return p;
  }

  refresh() {
    this.cache.clear();
    for (const [root, page] of this.roots) void this.annots(page.src).then((list) => this.fill(root, page, list));
  }

  mount(page: LayerPage) {
    const root = document.createElement("div");
    root.className = "an-root";
    this.roots.set(root, page);
    root.addEventListener("pointerdown", (e) => this.onDown(e, page, root));
    void this.annots(page.src).then((list) => this.fill(root, page, list));
    return root;
  }

  unmount(_page: LayerPage, el: HTMLElement) {
    this.roots.delete(el);
  }

  /** Invisible hit areas over each annotation, plus the selection frame. */
  private fill(root: HTMLElement, page: LayerPage, list: Annot[]) {
    root.querySelectorAll(".an-hit, .an-sel").forEach((e) => e.remove());
    const sel = selectedAnnot.value;
    for (const a of list) {
      const hit = document.createElement("div");
      hit.className = "an-hit";
      place(hit, a.r);
      hit.title = [a.author, a.text].filter(Boolean).join(": ") || a.t;
      // A click (not a drag: that selects text) selects the annotation.
      hit.addEventListener("click", (e) => {
        if (annotTool.value || getSelection()?.toString()) return;
        e.stopPropagation();
        selectedAnnot.value = { tab: this.tab, src: page.src, a };
        this.refreshSelection();
      });
      if (a.t === "FreeText") hit.addEventListener("dblclick", () => this.editText(root, page, a));
      root.append(hit);
      const pend = pendingSelect;
      if (pend && pend.tab === this.tab && pend.src === page.src && pend.n === a.n) {
        pendingSelect = null;
        selectedAnnot.value = { tab: this.tab, src: page.src, a };
        root.append(this.frame(root, page, a));
      } else if (sel && sel.tab === this.tab && sel.src === page.src && sel.a.n === a.n) {
        // Fresh data for the selected one; setting it only when it changed avoids a redraw loop.
        if (JSON.stringify(sel.a) !== JSON.stringify(a)) selectedAnnot.value = { ...sel, a };
        root.append(this.frame(root, page, a));
      }
    }
  }

  /** Redraws hit areas and the selection frame (no new data needed). */
  redraw() {
    this.refreshSelection();
  }

  private refreshSelection() {
    for (const [root, page] of this.roots) void this.annots(page.src).then((list) => this.fill(root, page, list));
  }

  /** Selection frame: drag to move; corner handles resize boxes. */
  private frame(root: HTMLElement, page: LayerPage, a: Annot) {
    const f = document.createElement("div");
    f.className = "an-sel";
    place(f, a.r);
    const movable = RECT_TYPES.has(a.t);
    if (movable) f.classList.add("movable");
    f.addEventListener("pointerdown", (e) => {
      e.stopPropagation();
      e.preventDefault();
      if (!movable) return;
      this.drag(e, page, a, "move", f);
    });
    f.addEventListener("dblclick", () => a.t === "FreeText" && this.editText(root, page, a));
    if (RESIZABLE.has(a.t))
      for (const corner of ["nw", "ne", "sw", "se"] as const) {
        const h = document.createElement("span");
        h.className = `an-handle ${corner}`;
        h.addEventListener("pointerdown", (e) => {
          e.stopPropagation();
          e.preventDefault();
          this.drag(e, page, a, corner, f);
        });
        f.append(h);
      }
    return f;
  }

  private drag(e: PointerEvent, page: LayerPage, a: Annot, mode: "move" | "nw" | "ne" | "sw" | "se", f: HTMLElement) {
    const v = this.tab.viewer!;
    const start = v.clientToPage(page.view, e.clientX, e.clientY);
    if (!start) return;
    let [x0, y0, x1, y1] = a.r;
    let dx = 0, dy = 0;
    const move = (ev: PointerEvent) => {
      const p = v.clientToPage(page.view, ev.clientX, ev.clientY);
      if (!p) return;
      dx = p[0] - start[0];
      dy = p[1] - start[1];
      [x0, y0, x1, y1] = a.r;
      if (mode === "move") [x0, y0, x1, y1] = [x0 + dx, y0 + dy, x1 + dx, y1 + dy];
      else {
        if (mode.includes("w")) x0 = Math.min(x1 - 6, x0 + dx);
        if (mode.includes("e")) x1 = Math.max(x0 + 6, x1 + dx);
        if (mode.includes("n")) y0 = Math.min(y1 - 6, y0 + dy);
        if (mode.includes("s")) y1 = Math.max(y0 + 6, y1 + dy);
        if (a.t === "Stamp") {
          const aspect = (a.r[2] - a.r[0]) / Math.max(1e-3, a.r[3] - a.r[1]);
          const h = (x1 - x0) / aspect;
          if (mode.includes("n")) y0 = y1 - h;
          else y1 = y0 + h;
        }
      }
      place(f, [x0, y0, x1, y1]);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      if (Math.abs(dx) < 0.5 && Math.abs(dy) < 0.5) return;
      const props: AnnotProps = {};
      if (a.ink) props.ink = a.ink.map((s) => s.map((c, i) => c + (i % 2 ? dy : dx)));
      else if (a.line) props.line = [a.line[0] + dx, a.line[1] + dy, a.line[2] + dx, a.line[3] + dy];
      else {
        // The drawn box includes the border; the annotation's own rect doesn't grow with it.
        props.rect = [x0, y0, x1, y1];
      }
      if (a.line) props.lineEndings = a.le;
      void editAnnot(this.tab, page.src, { num: a.n, props });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  /** Typing in a text box: a textarea over it; the text is saved when you click away. */
  private editText(root: HTMLElement, page: LayerPage, a: Annot | null, rect?: number[]) {
    const r = a?.r ?? rect!;
    const ta = document.createElement("textarea");
    ta.className = "an-edit";
    place(ta, r);
    const st = styleOf("text");
    ta.style.fontSize = `${a?.fs || st.fontSize}px`;
    ta.style.color = a?.tc ? rgbToHex(a.tc) : st.color;
    ta.value = a?.text ?? "";
    ta.addEventListener("pointerdown", (e) => e.stopPropagation());
    ta.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") ta.blur();
    });
    let done = false;
    const commit = async () => {
      if (done) return;
      done = true;
      const text = ta.value.trim();
      ta.remove();
      if (a) {
        if (!text) await editAnnot(this.tab, page.src, { num: a.n, delete: true });
        else if (text !== a.text) await editAnnot(this.tab, page.src, { num: a.n, props: { contents: text } });
      } else if (text) {
        // Grow the box to fit the text.
        const lines = text.split("\n");
        const size = st.fontSize;
        const w = Math.max(40, Math.min(page.w - r[0] - 4, Math.max(...lines.map((l) => l.length)) * size * 0.55 + 8));
        const h = lines.length * size * 1.25 + 6;
        await editAnnot(this.tab, page.src, {
          kind: "FreeText",
          props: { rect: [r[0], r[1], r[0] + w, r[1] + h], contents: text, font: ["Helv", size, hexToRgb(st.color)] },
        });
      }
    };
    ta.addEventListener("blur", () => void commit());
    root.append(ta);
    requestAnimationFrame(() => ta.focus());
  }

  // ------------------------------------------------------------ drawing new ones

  private onDown(e: PointerEvent, page: LayerPage, root: HTMLElement) {
    const tool = annotTool.value;
    if (!tool || tool === "highlight" || tool === "underline" || tool === "strike" || e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    selectedAnnot.value = null;
    const v = this.tab.viewer!;
    const start = v.clientToPage(page.view, e.clientX, e.clientY);
    if (!start) return;
    const kind = toolStyle(tool);
    const st = styleOf(kind);
    const color = hexToRgb(st.color);

    // Click-to-place tools.
    if (tool === "note") {
      void (async () => {
        const n = await editAnnot(this.tab, page.src, { kind: "Text", props: { rect: [start[0] - 10, start[1] - 10, start[0] + 10, start[1] + 10], icon: "Comment", color } });
        if (n !== null) selectWhenShown(this.tab, page.src, n);
      })();
      annotTool.value = null;
      return;
    }
    if (tool === "text") {
      this.editText(root, page, null, [start[0], start[1] - st.fontSize * 0.8, start[0] + 220, start[1] + st.fontSize * 1.2]);
      annotTool.value = null;
      return;
    }
    const colon = tool.indexOf(":");
    const placer = colon > 0 ? placers.get(tool.slice(0, colon)) : undefined;
    if (placer) {
      void placer(this.tab, page.src, tool.slice(colon + 1), start);
      annotTool.value = null;
      return;
    }
    if (tool.startsWith("stamp:")) {
      const name = tool.slice(6);
      void placeStamp(this.tab, page.src, name, start);
      annotTool.value = null;
      return;
    }

    // Drag tools: preview in SVG, create on release.
    const svg = document.createElementNS(SVG, "svg");
    svg.setAttribute("class", "an-draft");
    svg.setAttribute("viewBox", `0 0 ${page.w} ${page.h}`);
    root.append(svg);
    const shape = document.createElementNS(SVG, tool === "pen" ? "polyline" : tool === "ellipse" ? "ellipse" : tool === "line" || tool === "arrow" ? "line" : "rect");
    shape.setAttribute("fill", tool === "area" ? st.color : "none");
    if (tool === "area") shape.setAttribute("fill-opacity", "0.4");
    shape.setAttribute("stroke", tool === "area" ? "none" : st.color);
    shape.setAttribute("stroke-width", String(st.width));
    shape.setAttribute("stroke-linecap", "round");
    shape.setAttribute("stroke-linejoin", "round");
    const boxer = tool.startsWith("box:") ? boxers.get(tool.slice(4)) : undefined;
    if (boxer) {
      shape.setAttribute("stroke", "rgb(43, 88, 209)");
      shape.setAttribute("stroke-width", "1");
      shape.setAttribute("stroke-dasharray", "4 3");
      shape.setAttribute("fill", "rgba(43, 88, 209, 0.08)");
    }
    svg.append(shape);
    const pts: number[] = [start[0], start[1]];
    let end = start;
    const draw = () => {
      const [x0, y0, x1, y1] = [Math.min(start[0], end[0]), Math.min(start[1], end[1]), Math.max(start[0], end[0]), Math.max(start[1], end[1])];
      if (tool === "pen") shape.setAttribute("points", pts.join(" "));
      else if (tool === "line" || tool === "arrow") {
        shape.setAttribute("x1", String(start[0]));
        shape.setAttribute("y1", String(start[1]));
        shape.setAttribute("x2", String(end[0]));
        shape.setAttribute("y2", String(end[1]));
      } else if (tool === "ellipse") {
        shape.setAttribute("cx", String((x0 + x1) / 2));
        shape.setAttribute("cy", String((y0 + y1) / 2));
        shape.setAttribute("rx", String((x1 - x0) / 2));
        shape.setAttribute("ry", String((y1 - y0) / 2));
      } else {
        shape.setAttribute("x", String(x0));
        shape.setAttribute("y", String(y0));
        shape.setAttribute("width", String(x1 - x0));
        shape.setAttribute("height", String(y1 - y0));
      }
    };
    const move = (ev: PointerEvent) => {
      const p = v.clientToPage(page.view, ev.clientX, ev.clientY);
      if (!p) return;
      end = p;
      if (tool === "pen") {
        const lx = pts[pts.length - 2], ly = pts[pts.length - 1];
        if (Math.hypot(p[0] - lx, p[1] - ly) >= 0.8) pts.push(p[0], p[1]);
      }
      draw();
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      const [x0, y0, x1, y1] = [Math.min(start[0], end[0]), Math.min(start[1], end[1]), Math.max(start[0], end[0]), Math.max(start[1], end[1])];
      const small = x1 - x0 < 3 && y1 - y0 < 3;
      if (boxer) {
        svg.remove();
        annotTool.value = null;
        void boxer(this.tab, page.src, [x0, y0, x1, y1]);
        return;
      }
      const req =
        tool === "pen"
          ? pts.length >= 4 ? { kind: "Ink", props: { ink: [smooth(pts)], color, border: st.width } } : null
          : small ? null
          : tool === "area" ? { kind: "Highlight", props: { quads: [x0, y0, x1, y0, x0, y1, x1, y1], color } }
          : tool === "rect" ? { kind: "Square", props: { rect: [x0, y0, x1, y1], color, border: st.width } }
          : tool === "ellipse" ? { kind: "Circle", props: { rect: [x0, y0, x1, y1], color, border: st.width } }
          : { kind: "Line", props: { line: [start[0], start[1], end[0], end[1]], lineEndings: [0, tool === "arrow" ? 4 : 0], color, border: st.width } };
      if (!req) {
        svg.remove();
        return;
      }
      // Keep the preview until the page has redrawn with the real annotation.
      void editAnnot(this.tab, page.src, req as { kind: string; props: AnnotProps }).finally(() => setTimeout(() => svg.remove(), 250));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }
}

/** Light smoothing of a pen stroke (moving average), keeping the ends. */
function smooth(pts: number[]): number[] {
  if (pts.length < 8) return pts;
  const out = [pts[0], pts[1]];
  for (let i = 2; i < pts.length - 2; i += 2) {
    out.push((pts[i - 2] + 2 * pts[i] + pts[i + 2]) / 4, (pts[i - 1] + 2 * pts[i + 1] + pts[i + 3]) / 4);
  }
  out.push(pts[pts.length - 2], pts[pts.length - 1]);
  return out;
}

function place(el: HTMLElement, r: number[]) {
  el.style.left = `${r[0]}px`;
  el.style.top = `${r[1]}px`;
  el.style.width = `${Math.max(2, r[2] - r[0])}px`;
  el.style.height = `${Math.max(2, r[3] - r[1])}px`;
}

// ---------------------------------------------------------------- stamps

export const STAMPS: [string, string][] = [
  ["Approved", "Approved"], ["NotApproved", "Not approved"], ["Draft", "Draft"], ["Final", "Final"], ["Confidential", "Confidential"],
  ["ForComment", "For comment"], ["Experimental", "Experimental"], ["Expired", "Expired"],
];

/** Fill & Sign marks are text boxes; named stamps are PDF stamps. */
export async function placeStamp(tab: Tab, src: number, name: string, at: [number, number]) {
  const [x, y] = at;
  if (name === "check" || name === "cross" || name === "dot") {
    const glyph = name === "check" ? "4" : name === "cross" ? "8" : "l"; // ZapfDingbats ✔ ✘ ●
    const s = 16;
    return editAnnot(tab, src, { kind: "FreeText", props: { rect: [x - s / 2, y - s / 2, x + s / 2, y + s / 2], contents: glyph, font: ["ZaDb", s * 0.8, [0, 0, 0]] } });
  }
  if (name === "date" || name === "name") {
    const text = name === "date" ? new Date().toLocaleDateString() : author || "Name";
    const size = 11;
    const w = text.length * size * 0.55 + 8;
    return editAnnot(tab, src, { kind: "FreeText", props: { rect: [x, y - size, x + w, y + size * 0.6], contents: text, font: ["Helv", size, [0, 0, 0]] } });
  }
  return editAnnot(tab, src, { kind: "Stamp", props: { rect: [x - 75, y - 20, x + 75, y + 20], icon: name } });
}

/** Layers by tab, for the sidebar's "go to annotation". */
export function annotLayerOf(tab: Tab) {
  return layers.get(tab) ?? null;
}
