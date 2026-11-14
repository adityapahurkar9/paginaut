// Turns the engine's per-page content (fo_reflow_json) into one continuous
// flow for Read and Book modes: headings, paragraphs and pictures in reading
// order, without running headers, footers or page numbers. Photos, tables and
// drawings stay pictures (cut from the page); only text reflows.

/** Box in page points: [x0, y0, x1, y1] relative to the page's top-left. */
export type Box = [number, number, number, number];

// ---------------------------------------------------------------- engine output

type RawSpan = [string, number, number]; // text, size, style bits
interface RawLine { x: Box; j?: 1; s: RawSpan[] }
type RawBlock =
  | { t: "p"; x: Box; l: RawLine[] }
  | { t: "i" | "tb" | "v"; x: Box };
export interface RawPage { w: number; h: number; b: RawBlock[] }

// ---------------------------------------------------------------- flow

export const BOLD = 1, ITALIC = 2, MONO = 4, SUP = 8;

export interface Run {
  t: string;
  /** Style bits: BOLD, ITALIC, MONO, SUP. */
  s: number;
}

export type Block =
  | { k: "p" | "li"; runs: Run[]; len: number; page: number }
  | { k: "h"; level: 1 | 2 | 3; runs: Run[]; len: number; page: number; /** Top of the heading on its page, in points. */ y?: number }
  | {
      k: "img";
      page: number;
      /** Page index in the file (for rendering the picture). */
      src: number;
      box: Box;
      /** Width of the picture relative to the page's text column (0–1). */
      rel: number;
      kind: "photo" | "table" | "figure" | "scan";
    };

export interface Flow {
  blocks: Block[];
  /** First block of each displayed page (blocks.length if the page has none). */
  pageStart: number[];
  /** Pages that are pictures only (scans without text). */
  scans: number;
  /** Most common text size in the document, in points. */
  bodySize: number;
}

export const textOf = (b: Block) => (b.k === "img" ? "" : b.runs.map((r) => r.t).join(""));

// ---------------------------------------------------------------- analysis

export interface TextItem {
  kind: "text";
  box: Box;
  runs: Run[];
  lines: number;
  /** Characters by size, for body-size and heading detection. */
  size: number;
  bold: boolean;
  chars: number;
}
export interface PicItem {
  kind: "pic";
  box: Box;
  pic: "photo" | "table" | "figure";
}
type Item = TextItem | PicItem;

const area = (b: Box) => Math.max(0, b[2] - b[0]) * Math.max(0, b[3] - b[1]);
const inter = (a: Box, b: Box) => a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3];
const union = (a: Box, b: Box): Box => [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.max(a[2], b[2]), Math.max(a[3], b[3])];
const grow = (b: Box, d: number): Box => [b[0] - d, b[1] - d, b[2] + d, b[3] + d];
const centreIn = (b: Box, r: Box) => {
  const cx = (b[0] + b[2]) / 2, cy = (b[1] + b[3]) / 2;
  return cx >= r[0] && cx <= r[2] && cy >= r[1] && cy <= r[3];
};

/** Joins a text block's lines into runs: dehyphenated lines join directly, others with a space. */
function blockText(block: { l: RawLine[] }): { runs: Run[]; sizes: Map<number, number>; bold: number; chars: number } {
  const runs: Run[] = [];
  const sizes = new Map<number, number>();
  let bold = 0, chars = 0;
  const push = (t: string, s: number) => {
    const last = runs[runs.length - 1];
    if (last && last.s === s) last.t += t;
    else runs.push({ t, s });
  };
  block.l.forEach((line, li) => {
    for (const [t, size, style] of line.s) {
      const n = t.trim().length;
      chars += n;
      if (!(style & SUP)) sizes.set(size, (sizes.get(size) ?? 0) + n);
      if (style & BOLD) bold += n;
      push(t.replace(/\s+/g, " "), style & (BOLD | ITALIC | MONO | SUP));
    }
    const last = runs[runs.length - 1];
    if (li < block.l.length - 1 && last && !line.j && !/\s$/.test(last.t)) last.t += " ";
  });
  // Trim the ends.
  if (runs.length) {
    runs[0].t = runs[0].t.replace(/^\s+/, "");
    runs[runs.length - 1].t = runs[runs.length - 1].t.replace(/\s+$/, "");
  }
  return { runs: runs.filter((r) => r.t), sizes, bold, chars };
}

function dominant(sizes: Map<number, number>) {
  let best = 0, n = -1;
  for (const [s, c] of sizes) if (c > n) [best, n] = [s, c];
  return best;
}

/** Normalised text of a header/footer candidate: digits don't matter ("Page 12" = "Page 7"). */
const zoneKey = (t: string) => t.toLowerCase().replace(/\d+/g, "#").replace(/\s+/g, " ").trim();
const PAGE_NUMBER = /^(page\s*)?([a-z]?-?\d+)(\s*(of|\/)\s*\d+)?$/i;
// A Roman page number as front matter carries them: a real numeral, small (i to lxxxix), all in one case. Any word
// of the Roman letters used to pass, so "CV", "Civil", "Mid" or "DVD" at the head of a page was left out.
const ROMAN = /^(?=.)(?:lx{0,3}|xl|x{0,3})(?:ix|iv|v?i{0,3})$/;
/** A line that is only a page number ("12", "Page 3 of 9", "A-4", "xii"). */
export function isPageNumber(text: string): boolean {
  const t = text.trim();
  if (PAGE_NUMBER.test(t)) return true;
  const w = t.replace(/^page\s*/i, "");
  return (w === w.toLowerCase() || w === w.toUpperCase()) && ROMAN.test(w.toLowerCase());
}

/**
 * Picture regions on a page: images, tables, and clusters of vector drawing
 * (charts, diagrams, ruled boxes). Overlapping regions merge.
 */
function pictures(raw: RawPage): PicItem[] {
  const pageArea = raw.w * raw.h;
  const out: PicItem[] = [];
  for (const b of raw.b) {
    if (b.t === "tb") out.push({ kind: "pic", box: b.x, pic: "table" });
    else if (b.t === "i") {
      const w = b.x[2] - b.x[0], h = b.x[3] - b.x[1];
      // Ignore bullets, rules and tiny icons; full-page backgrounds are handled by the caller.
      if (w >= 24 && h >= 24 && area(b.x) < 0.9 * pageArea) out.push({ kind: "pic", box: b.x, pic: "photo" });
    }
  }
  // Vector drawing: cluster the pieces, keep clusters big enough to be a figure.
  const vecs = raw.b
    .filter((b) => b.t === "v" && area(b.x) < 0.6 * pageArea && !out.some((p) => p.pic === "table" && centreIn(b.x, p.box)))
    .map((b) => b.x);
  const clusters: Box[] = [];
  for (const v of vecs) {
    let box = v;
    for (let i = clusters.length - 1; i >= 0; i--) {
      if (inter(grow(clusters[i], 4), box)) {
        box = union(box, clusters[i]);
        clusters.splice(i, 1);
      }
    }
    clusters.push(box);
  }
  for (const c of clusters) {
    const w = c[2] - c[0], h = c[3] - c[1];
    if (w >= 40 && h >= 30 && area(c) >= 0.02 * pageArea && area(c) <= 0.8 * pageArea) out.push({ kind: "pic", box: c, pic: "figure" });
  }
  // Merge overlaps (a photo inside a figure, a table with a drawn border…).
  let merged = true;
  while (merged) {
    merged = false;
    for (let i = 0; i < out.length && !merged; i++)
      for (let j = i + 1; j < out.length && !merged; j++)
        if (inter(out[i].box, out[j].box)) {
          const kind = out[i].pic === "table" || out[j].pic === "table" ? "table" : out[i].pic === "photo" || out[j].pic === "photo" ? "photo" : "figure";
          out[i] = { kind: "pic", box: union(out[i].box, out[j].box), pic: kind };
          out.splice(j, 1);
          merged = true;
        }
  }
  return out;
}

/**
 * Reading order by recursive XY-cut: split at empty vertical gutters first
 * (columns), otherwise at horizontal gaps (bands), then recurse. Handles a
 * full-width title above two columns, pictures between sections, etc.
 */
export function readingOrder<T extends { box: Box }>(items: T[]): T[] {
  if (items.length <= 1) return items;
  const cut = (axis: 0 | 1, minGap: number): T[][] | null => {
    const lo = axis, hi = axis + 2;
    const sorted = [...items].sort((a, b) => a.box[lo] - b.box[lo]);
    const groups: T[][] = [[sorted[0]]];
    let end = sorted[0].box[hi];
    for (let i = 1; i < sorted.length; i++) {
      const it = sorted[i];
      if (it.box[lo] - end >= minGap) groups.push([it]);
      else groups[groups.length - 1].push(it);
      end = Math.max(end, it.box[hi]);
    }
    return groups.length > 1 ? groups : null;
  };
  // Columns need a real gutter and real width on both sides (a column of list
  // numbers beside their paragraphs is not a column); bands just need a gap.
  const cols = cut(0, 9);
  const width = (g: T[]) => Math.max(...g.map((it) => it.box[2])) - Math.min(...g.map((it) => it.box[0]));
  const total = width(items);
  const colsOk = cols && cols.every((g) => width(g) >= 0.18 * total && g.some((it) => it.box[3] - it.box[1] > 20 || g.length > 1));
  const parts = colsOk ? cols : cut(1, 0.5);
  if (!parts) return [...items].sort((a, b) => a.box[1] - b.box[1] || a.box[0] - b.box[0]);
  return parts.flatMap((p) => readingOrder(p));
}

const MARKER = /^(\(?[\divxlcA-Za-z]{1,3}[.)]|[•●▪■◦○►▶–\-*·])$/;

// ---------------------------------------------------------------- your corrections

/** What a block or region should be treated as. */
export type ItemKind = "text" | "picture" | "ignore";

/** Corrections for one page (keyed by file page index). */
export interface PageOverride {
  /** Columns on this page (0 = automatic). */
  columns?: number;
  /** Reading order: item keys in the order you set. */
  order?: string[];
  /** Per item: treat as text, a picture, or leave it out. */
  kinds?: Record<string, ItemKind>;
  /** Rectangles you drew: everything inside becomes one picture, is left out, or is kept as text. */
  regions?: { box: Box; kind: ItemKind }[];
}

export interface LayoutOverrides {
  /** Columns for the whole document (0 = automatic). */
  columns?: number;
  pages?: Record<number, PageOverride>;
}

/** An item on a page as the layout sees it (for Read/Book and the layout editor). */
export interface LayoutItem {
  key: string;
  box: Box;
  kind: "text" | "pic";
  pic?: "photo" | "table" | "figure";
  text?: TextItem;
  /** Why an item isn't in the flow. */
  dropped?: "header" | "ignored";
  /** Set by a correction rather than detected. */
  manual?: boolean;
}

export interface PageLayout {
  scan: boolean;
  w: number;
  h: number;
  /** Items in the flow, in reading order. */
  items: LayoutItem[];
  /** Items left out (running headers/footers, or ignored by you). */
  dropped: LayoutItem[];
}

const keyOf = (b: Box) => `${Math.round(b[0])},${Math.round(b[1])},${Math.round(b[2])},${Math.round(b[3])}`;

/**
 * Forced column count: items that span columns (titles, wide pictures) split
 * the page into bands; within a band, each column is read top to bottom.
 */
function columnOrder<T extends { box: Box }>(items: T[], n: number): T[] {
  if (items.length <= 1) return items;
  const x0 = Math.min(...items.map((i) => i.box[0])), x1 = Math.max(...items.map((i) => i.box[2]));
  const colW = (x1 - x0) / n;
  const spans = (it: T) => it.box[2] - it.box[0] > colW * 1.35;
  const byY = [...items].sort((a, b) => a.box[1] - b.box[1] || a.box[0] - b.box[0]);
  if (n <= 1) return byY;
  const out: T[] = [];
  let band: T[] = [];
  const flush = () => {
    const cols: T[][] = Array.from({ length: n }, () => []);
    for (const it of band) {
      const cx = (it.box[0] + it.box[2]) / 2;
      cols[Math.min(n - 1, Math.max(0, Math.floor((cx - x0) / colW)))].push(it);
    }
    for (const c of cols) out.push(...c);
    band = [];
  };
  for (const it of byY) {
    if (spans(it)) {
      flush();
      out.push(it);
    } else band.push(it);
  }
  flush();
  return out;
}

interface PageBase {
  raw: RawPage;
  scan: boolean;
  texts: (TextItem & { key: string })[];
  pics: (PicItem & { key: string })[];
}

/**
 * Document analysis: per-page items and document statistics (body text size,
 * repeated headers/footers), computed once. Layouts and the flow are then
 * cheap to recompute when you correct something.
 */
export class Analysis {
  private base: (PageBase | undefined)[];
  readonly bodySize: number;
  private zoneCount = new Map<string, number>();
  private repeatMin: number;

  constructor(pages: (RawPage | undefined)[]) {
    const sizeChars = new Map<number, number>();
    this.base = pages.map((raw) => {
      if (!raw) return undefined;
      const pageArea = raw.w * raw.h;
      const texts: PageBase["texts"] = [];
      for (const b of raw.b) {
        if (b.t !== "p") continue;
        const { runs, sizes, bold, chars } = blockText(b);
        if (!chars) continue;
        for (const [s, c] of sizes) sizeChars.set(s, (sizeChars.get(s) ?? 0) + c);
        texts.push({ kind: "text", box: b.x, runs, lines: b.l.length, size: dominant(sizes), bold: bold >= 0.85 * chars, chars, key: keyOf(b.x) });
      }
      const chars = texts.reduce((a, t) => a + t.chars, 0);
      // A scan: (almost) no text and a picture covering most of the page.
      const scan = chars < 30 && raw.b.some((b) => b.t === "i" && area(b.x) >= 0.5 * pageArea);
      // Text over a full-page image is an OCR'd scan: the text reflows, the image goes.
      const pics = pictures(raw)
        .filter((p) => !(p.pic === "photo" && area(p.box) >= 0.5 * pageArea && chars >= 30))
        .map((p) => ({ ...p, key: keyOf(p.box) }));
      if (!scan) {
        for (const t of texts) {
          if (pics.some((p) => centreIn(t.box, p.box))) continue;
          const z = this.zone(raw, t);
          if (z) this.zoneCount.set(z, (this.zoneCount.get(z) ?? 0) + 1);
        }
      }
      return { raw, scan, texts, pics };
    });
    this.bodySize = dominant(sizeChars) || 10;
    this.repeatMin = Math.max(3, Math.round(pages.length * 0.12));
  }

  /** Header/footer key of a text item near the top or bottom edge, or "". */
  private zone(raw: RawPage, t: TextItem) {
    const cy = (t.box[1] + t.box[3]) / 2;
    if (cy >= 0.09 * raw.h && cy <= 0.91 * raw.h) return "";
    return (cy < raw.h / 2 ? "T:" : "B:") + zoneKey(t.runs.map((r) => r.t).join(""));
  }

  private isRunning(raw: RawPage, t: TextItem) {
    const z = this.zone(raw, t);
    if (!z) return false;
    if (isPageNumber(t.runs.map((r) => r.t).join(""))) return true;
    return (this.zoneCount.get(z) ?? 0) >= this.repeatMin;
  }

  /** Items of file page `src` after your corrections, in reading order. */
  layout(src: number, ov: PageOverride | undefined, docColumns = 0): PageLayout | null {
    const b = this.base[src];
    if (!b) return null;
    const { raw } = b;
    const regions = ov?.regions ?? [];
    const kinds = ov?.kinds ?? {};
    const inRegion = (box: Box, kind: ItemKind) => regions.some((r) => r.kind === kind && centreIn(box, r.box));
    if (b.scan && !regions.length) return { scan: true, w: raw.w, h: raw.h, items: [], dropped: [] };

    const dropped: LayoutItem[] = [];
    // Pictures: detected ones (unless you said text/ignore), your picture regions, and text you marked as a picture.
    const pics: LayoutItem[] = [];
    for (const p of b.pics) {
      const k = kinds[p.key];
      if (k === "text" || inRegion(p.box, "text")) continue;
      const item: LayoutItem = { key: p.key, box: p.box, kind: "pic", pic: p.pic };
      if (k === "ignore" || inRegion(p.box, "ignore")) dropped.push({ ...item, dropped: "ignored", manual: true });
      else if (!inRegion(p.box, "picture")) pics.push(item);
    }
    for (const r of regions) if (r.kind === "picture") pics.push({ key: keyOf(r.box), box: r.box, kind: "pic", pic: "figure", manual: true });
    for (const t of b.texts) if (kinds[t.key] === "picture") pics.push({ key: t.key, box: t.box, kind: "pic", pic: "figure", manual: true });

    const texts: LayoutItem[] = [];
    for (const t of b.texts) {
      const k = kinds[t.key];
      if (k === "picture") continue;
      const item: LayoutItem = { key: t.key, box: t.box, kind: "text", text: t };
      const forcedText = k === "text" || inRegion(t.box, "text");
      if (!forcedText && pics.some((p) => centreIn(t.box, p.box))) continue; // part of a picture
      if (k === "ignore" || inRegion(t.box, "ignore")) dropped.push({ ...item, dropped: "ignored", manual: true });
      else if (!forcedText && this.isRunning(raw, t)) dropped.push({ ...item, dropped: "header" });
      else texts.push(k === "text" ? { ...item, manual: true } : item);
    }

    const body = [...texts, ...pics];
    let items: LayoutItem[];
    const cols = ov?.columns || docColumns;
    const auto = cols ? columnOrder(body, cols) : readingOrder(body);
    if (ov?.order?.length) {
      const rank = new Map(ov.order.map((k, i) => [k, i]));
      const known = auto.filter((i) => rank.has(i.key)).sort((a, c) => rank.get(a.key)! - rank.get(c.key)!);
      items = [...known, ...auto.filter((i) => !rank.has(i.key))];
    } else items = auto;
    return { scan: false, w: raw.w, h: raw.h, items, dropped };
  }

  /**
   * Builds the flow. `order` is the displayed page order (after page edits);
   * `ov` holds your corrections.
   */
  flow(order: number[], ov: LayoutOverrides = {}): Flow {
    const blocks: Block[] = [];
    const pageStart: number[] = [];
    let scans = 0;
    const bodySize = this.bodySize;
    const lastText = () => {
      const b = blocks[blocks.length - 1];
      return b && (b.k === "p" || b.k === "li") ? b : null;
    };
    /** Where the last paragraph sat on its page, to spot a paragraph split with no gap. */
    let prevGeom: { page: number; box: Box; lineH: number } | null = null;
    let hGeom: { box: Box; lineH: number } | null = null;
    order.forEach((src, page) => {
      pageStart.push(blocks.length);
      const lay = this.layout(src, ov.pages?.[src], ov.columns);
      if (!lay) return;
      if (lay.scan) {
        scans++;
        blocks.push({ k: "img", page, src, box: [0, 0, lay.w, lay.h], rel: 1, kind: "scan" });
        return;
      }
      const ordered = lay.items;
      // Text column width on this page, for sizing pictures relative to it.
      const textW = Math.max(1, ...ordered.filter((i) => i.kind === "text" && i.text!.lines > 1).map((i) => i.box[2] - i.box[0]), lay.w * 0.6);
      for (let i = 0; i < ordered.length; i++) {
        const it = ordered[i];
        if (it.kind === "pic") {
          blocks.push({ k: "img", page, src, box: it.box, rel: Math.min(1, (it.box[2] - it.box[0]) / textW), kind: it.pic ?? "figure" });
          continue;
        }
        const t = it.text!;
        let runs = t.runs;
        const text = runs.map((r) => r.t).join("");
        // "6." or "•" set apart from its paragraph: join them.
        const next = ordered[i + 1];
        if (MARKER.test(text.trim()) && next && next.kind === "text" && Math.abs(next.box[1] - it.box[1]) < 4 && next.box[0] >= it.box[2] - 1) {
          ordered[i + 1] = { ...next, box: union(it.box, next.box), text: { ...next.text!, runs: [{ t: text.trim() + " ", s: runs[0]?.s ?? 0 }, ...next.text!.runs] } };
          continue;
        }
        const len = text.length;
        const ratio = t.size / bodySize;
        const short = t.lines <= 3 && len <= 200;
        const level = short && ratio >= 1.12 ? (ratio >= 1.6 ? 1 : ratio >= 1.3 ? 2 : 3) : short && t.bold && len <= 160 && ratio >= 0.95 && !/[.,;]$/.test(text) ? 3 : 0;
        if (level) {
          // A heading the engine split in two (no gap, first part unfinished) is one heading.
          const last = blocks[blocks.length - 1];
          if (last && last.k === "h" && last.level === level && last.page === page && hGeom &&
              it.box[1] - hGeom.box[3] < 0.6 * hGeom.lineH && !/[.:;?!]$/.test(textOf(last).trimEnd())) {
            const lt = textOf(last);
            const sep = /\s$/.test(lt) ? "" : " ";
            blocks[blocks.length - 1] = { ...last, runs: mergeRuns([...last.runs.slice(0, -1), { ...last.runs[last.runs.length - 1], t: last.runs[last.runs.length - 1].t + sep }, ...runs]), len: lt.length + sep.length + len };
            hGeom = { box: union(hGeom.box, it.box), lineH: hGeom.lineH };
            continue;
          }
          blocks.push({ k: "h", level: level as 1 | 2 | 3, runs, len, page, y: it.box[1] });
          hGeom = { box: it.box, lineH: (it.box[3] - it.box[1]) / Math.max(1, t.lines) };
          continue;
        }
        // A paragraph cut by a page or column break (or split by the engine
        // after a list item's first line) continues where it left off: the
        // previous one stops mid-sentence and this one doesn't start a new one.
        const prev = lastText();
        const prevText = prev ? textOf(prev) : "";
        const lineH = (it.box[3] - it.box[1]) / Math.max(1, t.lines);
        // Split with no gap between them on the same page (the engine sometimes
        // splits a list item after its first lines): still the same paragraph.
        const tight = !!prevGeom && prevGeom.page === page && lastText() !== null && blocks[blocks.length - 1] === prev &&
          it.box[1] - prevGeom.box[3] < 0.45 * prevGeom.lineH && it.box[1] >= prevGeom.box[1] && Math.abs(it.box[0] - prevGeom.box[0]) < 40;
        // Joined when the previous one didn't end a sentence and this one
        // starts mid-sentence (lowercase, a number), or sits right under it.
        const prevEnd = prevText.trimEnd();
        const unfinished = !/[.!?:;]["”’)]?$/.test(prevEnd);
        const lowerStart = /^[\p{Ll}\p{N}(]/u.test(text);
        if (prev && unfinished && (lowerStart || (tight && /[\p{L}\p{N},\p{Sc}]$/u.test(prevEnd))) && !MARKER.test(text.split(" ")[0]) && Math.abs(ratio - 1) < 0.2) {
          const joined = /-$/.test(prevText) ? "" : " ";
          runs = [...prev.runs.slice(0, -1), { ...prev.runs[prev.runs.length - 1], t: prev.runs[prev.runs.length - 1].t + joined }, ...runs];
          blocks[blocks.length - 1] = { ...prev, runs: mergeRuns(runs), len: prevText.length + joined.length + len };
          prevGeom = { page, box: prevGeom && prevGeom.page === page ? union(prevGeom.box, it.box) : it.box, lineH };
          continue;
        }
        const li = MARKER.test(text.split(" ")[0]);
        blocks.push({ k: li ? "li" : "p", runs, len, page });
        prevGeom = { page, box: it.box, lineH };
      }
    });
    return { blocks, pageStart, scans, bodySize };
  }
}

/** One-shot flow without corrections. */
export function buildFlow(order: number[], pages: (RawPage | undefined)[]): Flow {
  return new Analysis(pages).flow(order);
}

function mergeRuns(runs: Run[]): Run[] {
  const out: Run[] = [];
  for (const r of runs) {
    const last = out[out.length - 1];
    if (last && last.s === r.s) last.t += r.t;
    else out.push({ ...r });
  }
  return out;
}
