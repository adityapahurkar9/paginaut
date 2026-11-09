// Page numbers, headers and footers, Bates numbers and watermarks: laid out
// here on each page as it's shown, then drawn by the engine as removable stamps.
import { api, type StampPart } from "./api";
import { DOC_STEP } from "./edits";
import { formsChanged } from "./forms";
import { parsePages } from "./ocr";
import { pageGeom } from "./pagesize";
import type { LayerPage, PageLayer, Viewer } from "./render/Viewer";
import { baseName, settings, showToast, updateSettings, type Tab } from "./store";

type M = [number, number, number, number, number, number];
const MM = 72 / 25.4;

/** a then b. */
const concat = (a: M, b: M): M => [
  a[0] * b[0] + a[1] * b[2],
  a[0] * b[1] + a[1] * b[3],
  a[2] * b[0] + a[3] * b[2],
  a[2] * b[1] + a[3] * b[3],
  a[4] * b[0] + a[5] * b[2] + b[4],
  a[4] * b[1] + a[5] * b[3] + b[5],
];
const translate = (x: number, y: number): M => [1, 0, 0, 1, x, y];
/** Turns counter-clockwise on screen by `deg` (y points down). */
const rotate = (deg: number): M => {
  const r = (deg * Math.PI) / 180;
  return [Math.cos(r), -Math.sin(r), Math.sin(r), Math.cos(r), 0, 0];
};

/** From the page as shown (turned by `rot`) to the page's own coordinates (w × h). */
function shownToPage(rot: number, w: number, h: number): M {
  switch (rot) {
    case 90: return [0, -1, 1, 0, 0, h];
    case 180: return [-1, 0, 0, -1, w, h];
    case 270: return [0, 1, -1, 0, w, 0];
    default: return [1, 0, 0, 1, 0, 0];
  }
}

export type Font = "sans-serif" | "serif" | "monospace";

export interface HeaderSettings {
  /** Header left, centre, right; footer left, centre, right. */
  boxes: [string, string, string, string, string, string];
  font: Font;
  size: number;
  color: string;
  bold: boolean;
  /** Margins in mm: top, bottom, sides. */
  margins: [number, number, number];
  start: number;
  bates: { prefix: string; digits: number; start: number; suffix: string };
}

export interface WatermarkSettings {
  kind: "text" | "picture";
  text: string;
  font: Font;
  size: number;
  color: string;
  bold: boolean;
  picture: string;
  /** Picture width, % of the page width. */
  scale: number;
  angle: number;
  opacity: number;
  position: "center" | "top" | "bottom";
  behind: boolean;
}

export const HEADER_DEFAULTS: HeaderSettings = {
  boxes: ["", "", "", "", "Page {page} of {pages}", ""],
  font: "sans-serif", size: 9, color: "#333333", bold: false, margins: [10, 10, 15], start: 1,
  bates: { prefix: "", digits: 6, start: 1, suffix: "" },
};

export const WATERMARK_DEFAULTS: WatermarkSettings = {
  kind: "text", text: "CONFIDENTIAL", font: "sans-serif", size: 60, color: "#c0392b", bold: true, picture: "", scale: 50,
  angle: 45, opacity: 20, position: "center", behind: false,
};

export const headerSettings = () => ({ ...HEADER_DEFAULTS, ...settings.value.headers });
export const watermarkSettings = () => ({ ...WATERMARK_DEFAULTS, ...settings.value.watermark });
export const setHeaders = (p: Partial<HeaderSettings>) => updateSettings({ headers: { ...headerSettings(), ...p } });
export const setWatermark = (p: Partial<WatermarkSettings>) => updateSettings({ watermark: { ...watermarkSettings(), ...p } });

const rgb = (hex: string): [number, number, number] => {
  const n = parseInt(hex.replace("#", ""), 16) || 0;
  return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
};

/** Display indices for "all" or a range like "2-" (null if the range isn't valid). */
export function pagesOf(tab: Tab, range: string | null): number[] | null {
  const n = tab.edits.value?.order.length ?? 0;
  return range === null || !range.trim() ? Array.from({ length: n }, (_, i) => i) : parsePages(range, n);
}

/** The text of a header box on one page: {page} {pages} {label} {date} {file} {bates}. */
export function fill(template: string, tab: Tab, view: number, k: number, count: number, s: HeaderSettings): string {
  const src = tab.edits.value?.order[view] ?? view;
  const label = tab.labels.value?.[src] || String(view + 1);
  const bates = `${s.bates.prefix}${String(s.bates.start + k).padStart(Math.max(1, s.bates.digits), "0")}${s.bates.suffix}`;
  return template
    .replace(/\{page\}/g, String(s.start + k))
    .replace(/\{pages\}/g, String(s.start + count - 1))
    .replace(/\{label\}/g, label)
    .replace(/\{date\}/g, new Date().toLocaleDateString())
    .replace(/\{file\}/g, baseName(tab.path).replace(/\.[^.]+$/, ""))
    .replace(/\{bates\}/g, bates);
}

/** After a stamp step: one undo entry, and everything drawn from the pages is stale. */
function stamped(tab: Tab) {
  tab.history.push(DOC_STEP);
  tab.historyVersion.value++;
  tab.textRev.value++;
  formsChanged(tab, true);
}

/** The header and footer boxes of one page (display index `v`, the `k`th of `count` pages stamped). */
function headerParts(tab: Tab, v: number, k: number, count: number, s: HeaderSettings): StampPart[] {
  const [top, bottom, side] = s.margins.map((m) => m * MM);
  const style = { font: s.font, size: s.size, color: rgb(s.color), bold: s.bold };
  const aligns = ["left", "center", "right"] as const;
  const g = pageGeom(tab, v);
  const toPage = shownToPage(g.rot, g.w, g.h);
  const parts: StampPart[] = [];
  s.boxes.forEach((tpl, i) => {
    if (!tpl.trim()) return;
    const footer = i >= 3;
    parts.push({
      ...style,
      text: fill(tpl, tab, v, k, count, s),
      align: aligns[i % 3],
      width: g.W - 2 * side,
      valign: footer ? 2 : 0,
      matrix: concat(translate(side, footer ? g.H - bottom : top), toPage),
    });
  });
  return parts;
}

/** Where a watermark sits on one page: its centre, turned by its angle, in the page's own coordinates. */
function watermarkPlace(tab: Tab, v: number, s: WatermarkSettings) {
  const g = pageGeom(tab, v);
  const toPage = shownToPage(g.rot, g.w, g.h);
  const cy = s.position === "top" ? g.H * 0.2 : s.position === "bottom" ? g.H * 0.8 : g.H / 2;
  return { g, around: concat(rotate(s.angle), concat(translate(g.W / 2, cy), toPage)) };
}

/** A text watermark on one page. */
function watermarkText(tab: Tab, v: number, s: WatermarkSettings): StampPart {
  const { g, around } = watermarkPlace(tab, v, s);
  const width = Math.hypot(g.W, g.H);
  return {
    text: s.text, font: s.font, size: s.size, color: rgb(s.color), bold: s.bold, align: "center",
    width, valign: 1, matrix: concat(translate(-width / 2, 0), around), opacity: s.opacity / 100,
  };
}

/** Headers and footers on the pages (display indices), replacing earlier ones. */
export async function applyHeaders(tab: Tab, views: number[]) {
  const s = headerSettings();
  const info = tab.info.value!;
  const pages = views.map((v, k) => ({ page: pageGeom(tab, v).src, parts: headerParts(tab, v, k, views.length, s) }));
  await api.stampPages(info.docId, pages, "Header", false, true);
  stamped(tab);
}

/** The watermark on the pages (display indices), replacing an earlier one. */
export async function applyWatermark(tab: Tab, views: number[]) {
  const s = watermarkSettings();
  const info = tab.info.value!;
  let aspect = 1;
  if (s.kind === "picture") {
    if (!s.picture) throw new Error("Choose a picture first.");
    const [pw, ph] = await api.firstPageSize(s.picture);
    aspect = ph / pw;
  }
  const pages = views.map((v) => {
    let part: StampPart;
    if (s.kind === "picture") {
      const { g, around } = watermarkPlace(tab, v, s);
      const w = (g.W * s.scale) / 100, h = w * aspect;
      part = { image: s.picture, width: w, height: h, matrix: concat(translate(-w / 2, -h / 2), around), opacity: s.opacity / 100 };
    } else part = watermarkText(tab, v, s);
    return { page: pageGeom(tab, v).src, parts: [part] };
  });
  await api.stampPages(info.docId, pages, "Watermark", s.behind, true);
  stamped(tab);
}

// ---------------------------------------------------------------- preview on the pages

const CSS_FONT: Record<Font, string> = {
  "sans-serif": "Helvetica, Arial, \"Nimbus Sans\", \"Liberation Sans\", sans-serif",
  serif: "\"Times New Roman\", Times, \"Nimbus Roman\", \"Liberation Serif\", serif",
  monospace: "\"Courier New\", Courier, \"Nimbus Mono PS\", \"Liberation Mono\", monospace",
};
const cssMatrix = (m: M) => `matrix(${m.join(",")})`;

/**
 * Headers and footers, or the watermark, drawn over the pages while they're being set up: laid out as the engine
 * will draw them (the same boxes and matrices), so what's shown is what Add puts there.
 */
export class StampPreview implements PageLayer {
  z = 25;
  private at = new Map<number, number>();
  private count = 0;
  private pic = "";
  private picUrl = "";
  constructor(readonly viewer: Viewer, private tab: Tab, private kind: "Header" | "Watermark") {
    viewer.addLayer(this);
  }

  /** The pages it goes on (display indices, null for none) and the settings changed: drawn again. */
  update(views: number[] | null) {
    this.at = new Map((views ?? []).map((v, k) => [v, k]));
    this.count = views?.length ?? 0;
    this.viewer.refreshLayer(this);
  }

  dispose() {
    this.viewer.removeLayer(this);
    if (this.picUrl) URL.revokeObjectURL(this.picUrl);
    this.pic = this.picUrl = "";
  }

  mount(page: LayerPage): HTMLElement | null {
    const k = this.at.get(page.view);
    if (k === undefined) return null;
    const root = document.createElement("div");
    root.className = "stamp-preview";
    if (this.kind === "Header") {
      const s = headerSettings();
      for (const p of headerParts(this.tab, page.view, k, this.count, s)) root.append(this.text(p));
    } else {
      const s = watermarkSettings();
      if (s.kind === "text") {
        if (s.text.trim()) root.append(this.text(watermarkText(this.tab, page.view, s)));
      } else if (s.picture) {
        const url = this.picture(s.picture);
        if (url) {
          const { g, around } = watermarkPlace(this.tab, page.view, s);
          const img = document.createElement("img");
          img.src = url;
          img.alt = "";
          Object.assign(img.style, {
            width: `${(g.W * s.scale) / 100}px`, opacity: String(s.opacity / 100),
            transform: `${cssMatrix(around)} translate(-50%, -50%)`,
          });
          root.append(img);
        }
      }
    }
    return root;
  }

  private text(p: StampPart) {
    const d = document.createElement("div");
    d.className = "stamp-preview-text";
    d.textContent = p.text ?? "";
    const [r, g, b] = (p.color ?? [0, 0, 0]).map((c) => Math.round(c * 255));
    Object.assign(d.style, {
      width: `${p.width}px`, fontFamily: CSS_FONT[p.font ?? "sans-serif"], fontSize: `${p.size ?? 9}px`,
      fontWeight: p.bold ? "bold" : "normal", color: `rgb(${r},${g},${b})`, textAlign: p.align ?? "left",
      opacity: String(p.opacity ?? 1), transform: `${cssMatrix(p.matrix)} translateY(${-(p.valign ?? 0) * 50}%)`,
    });
    return d;
  }

  /** The watermark picture's address for an <img>, once it's been read (drawn again then). */
  private picture(path: string) {
    if (path === this.pic) return this.picUrl;
    if (this.picUrl) URL.revokeObjectURL(this.picUrl);
    this.pic = path;
    this.picUrl = "";
    api.readPicture(path).then(
      (bytes) => {
        if (this.pic !== path) return;
        this.picUrl = URL.createObjectURL(new Blob([bytes]));
        this.viewer.refreshLayer(this);
      },
      () => {},
    );
    return "";
  }
}

/** Takes headers and footers (kind "Header") or watermarks off every page. */
export async function removeStamps(tab: Tab, kind: "Header" | "Watermark") {
  const info = tab.info.value;
  if (!info?.isPdf) return;
  const what = kind === "Watermark" ? "watermarks" : "headers, footers and page numbers";
  try {
    const n = await api.removeStamps(info.docId, kind, null);
    if (!n) return showToast(`There are no ${what} added by Paginaut in this document`);
    stamped(tab);
    showToast(`Removed ${what} from ${n === 1 ? "1 page" : `${n} pages`} · Ctrl+Z to undo`);
  } catch (e) {
    showToast(`Couldn't remove them: ${e}`, 5000);
  }
}
