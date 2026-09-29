// Typed wrappers over the Rust backend.
import { Channel, invoke } from "@tauri-apps/api/core";

export type Quad = [number, number, number, number, number, number, number, number];

export interface DocInfo {
  docId: number;
  path: string;
  pageCount: number;
  needsPassword: boolean;
  isPdf: boolean;
  reflowable: boolean;
  format: string;
  title: string;
  author: string;
  fileSize: number;
  firstSize: [number, number];
}

/** A file to take pages from: all of them, or `pages` (0-based). */
export interface InsertSource {
  path: string;
  pages?: number[];
  password?: string;
}

/** One piece of a stamp: text laid out in a box, or a picture filling it; matrix places the box on the page. */
export interface StampPart {
  text?: string;
  image?: string;
  font?: "sans-serif" | "serif" | "monospace";
  size?: number;
  color?: [number, number, number];
  bold?: boolean;
  align?: "left" | "center" | "right";
  width: number;
  height?: number;
  /** Text: the box's top (0), middle (1) or bottom (2) is at y = 0. */
  valign?: 0 | 1 | 2;
  matrix: [number, number, number, number, number, number];
  opacity?: number;
}

/** A file's page count and kind; `locked`: it needs a password (none or a wrong one given). */
export interface Peek {
  pages: number;
  isPdf: boolean;
  locked: boolean;
}

/** Pages added at the end of a document: source indices first..first + count - 1. */
export interface Inserted {
  first: number;
  count: number;
  sizes: [number, number][];
}

/** Page labels from page index s: style D (1, 2), r (i, ii), R (I, II), a (a, b), A (A, B) or "" (prefix only). */
export interface LabelRange {
  s: number;
  st: "D" | "r" | "R" | "a" | "A" | "";
  p: string;
  n: number;
}

/** A picture on a page: r (relative to the page origin), size in pixels, how it's stored
 *  (k "jpg"/"jp2"/"png"), m: has transparency, n: stored bytes, id: same picture, same id. */
export interface Picture {
  r: [number, number, number, number];
  w: number;
  h: number;
  k: string;
  m: number;
  n: number;
  id: string;
}

/** A file embedded in a PDF: on the document (page -1) or pinned to a page; modified: seconds since 1970, or -1. */
export interface Attachment {
  i: number;
  page: number;
  size: number;
  modified: number;
  name: string;
  mime: string;
  desc: string;
}

/** A PDF's protection: method "AES"/"RC4" (with bits) or "" when none; perms: PDF permission bits;
 *  owner: its protection can be changed (opened with the permissions password, or it has none). */
export interface Security {
  method: string;
  bits: number;
  perms: number;
  owner: boolean;
  opens: boolean;
  repaired: boolean;
}

/** Beyond a plain save: new protection, compression (0 lossless, 1 smaller, 2 smallest), or a clean rewrite. */
export interface SaveOptions {
  protect?: { remove?: boolean; user?: string; owner?: string; perms?: number };
  compress?: 0 | 1 | 2;
  rewrite?: boolean;
}

export interface Properties {
  title: string;
  author: string;
  subject: string;
  keywords: string;
  creator?: string;
  producer?: string;
  created?: string;
  modified?: string;
}

export interface OutlineItem {
  t: string;
  p: number;
  y: number;
  k?: OutlineItem[];
}

export interface LinkItem {
  r: [number, number, number, number];
  u?: string;
  p?: number;
  y?: number;
}

export interface SystemInfo {
  cpus: number;
  workers: number;
  totalMemory: number;
  busy: number;
  queued: number;
  processMemory: number;
  uptimeMs: number;
}

export type SearchEvent =
  | { kind: "hits"; page: number; hits: number[][]; snippets: string[] }
  | { kind: "scanned"; from: number; to: number }
  | { kind: "done"; complete: boolean; error: string | null; limited: boolean };

/** A form field on a page (see fo_widgets_json). */
export interface Widget {
  n: number;
  t: "text" | "checkbox" | "radio" | "combo" | "list" | "signature" | "button" | "unknown";
  r: [number, number, number, number];
  name: string;
  v: string;
  ff: number;
  ro?: 1;
  fs: number;
  q: number;
  max?: number;
  fmt?: number;
  /** The value as the form's format script shows it (when different). */
  fv?: string;
  on?: number;
  opts?: [string, string][];
  multi?: 1;
  signed?: 1;
}

/** An annotation on a page (see fo_annots_json); coordinates relative to the page's top-left. */
export interface Annot {
  n: number;
  t: string;
  r: [number, number, number, number];
  c?: number[];
  ic?: number[];
  o: number;
  bw?: number;
  text: string;
  author?: string;
  date: number;
  icon?: string;
  q?: number[];
  ink?: number[][];
  line?: [number, number, number, number];
  le?: [number, number];
  fs?: number;
  tc?: number[];
}

/** Properties sent when creating or changing an annotation. */
export interface AnnotProps {
  rect?: number[];
  quads?: number[];
  ink?: number[][];
  line?: number[];
  lineEndings?: number[];
  color?: number[];
  interior?: number[];
  opacity?: number;
  border?: number;
  icon?: string;
  font?: [string, number, number[]];
  /** Base64 PNG/JPEG. */
  image?: string;
  contents?: string;
}

export interface FormChange {
  accepted: boolean;
  /** The document differs from the file on disk. */
  changed: boolean;
}

export type ReflowEvent =
  | { kind: "page"; page: number; data: string }
  | { kind: "done"; complete: boolean; error: string | null };

export type OcrEvent =
  | { kind: "progress"; done: number; total: number }
  | { kind: "done"; end: import("./ocr").OcrEnd };

export interface EngineSearchOptions {
  matchCase: boolean;
  regex: boolean;
  snippets: boolean;
  /** Page to start from; results near it arrive first. */
  start: number;
}

export const api = {
  open: (path: string) => invoke<DocInfo>("open_document", { path }),
  unlock: (doc: number, password: string) => invoke<DocInfo | null>("unlock_document", { doc, password }),
  close: (doc: number) => invoke<void>("close_document", { doc }),
  trim: (doc: number) => invoke<void>("trim_document", { doc }),
  memoryLevel: (low: boolean) => invoke<void>("memory_level", { low }),
  save: (doc: number, order: number[], rot: number[], target: string | null) =>
    invoke<DocInfo>("save_document", { doc, order, rot, target }),
  pageSizes: (doc: number) => invoke<[number, number][]>("page_sizes", { doc }),
  /** Adds pages of other files at the end of the document (one undo step); the page order puts them in place. */
  insertPages: (doc: number, sources: InsertSource[], fit: [number, number] | null) => invoke<Inserted>("insert_pages", { doc, sources, fit }),
  insertBlank: (doc: number, w: number, h: number, count: number) => invoke<Inserted>("insert_blank", { doc, w, h, count }),
  /** Combines files into a new PDF at `target`; returns its page count. */
  mergeFiles: (sources: InsertSource[], fit: [number, number] | null, bookmarks: boolean, target: string) =>
    invoke<number>("merge_files", { sources, fit, bookmarks, target }),
  /** Copies pages of another open document to the end of this one (one undo step). */
  insertFromOpen: (doc: number, from: number, pages: number[]) => invoke<Inserted>("insert_from_open", { doc, from, pages }),
  /** Writes pages (document indices) to new PDFs; `rot` per document page. Returns each file's size. */
  saveParts: (doc: number, parts: { pages: number[]; path: string }[], rot: number[]) => invoke<number[]>("save_parts", { doc, parts, rot }),
  /** Groups pages (document indices, in order) into parts of about maxBytes. */
  splitBySize: (doc: number, order: number[], maxBytes: number) => invoke<number[][]>("split_by_size", { doc, order, maxBytes }),
  /** Crops and/or resizes pages (one undo step); returns their new sizes. */
  pageSize: (
    doc: number,
    ops: { page: number; crop: [number, number, number, number]; size?: [number, number] }[],
    opts: { fit: number; anchor: [number, number]; remove: boolean; fill: number; color: [number, number, number] },
  ) => invoke<[number, number][]>("page_size", { doc, ops, opts }),
  contentBox: (doc: number, page: number) => invoke<[number, number, number, number] | null>("content_box", { doc, page }),
  pageSizesOf: (doc: number, pages: number[]) => invoke<[number, number][]>("page_sizes_of", { doc, pages }),
  /** Stamps pages (page numbers, headers, watermarks) as one undo step. */
  stampPages: (doc: number, pages: { page: number; parts: StampPart[] }[], kind: string, behind: boolean, replace: boolean) =>
    invoke<number>("stamp_pages", { doc, pages, kind, behind, replace }),
  /** Takes stamps of one kind off the pages; returns how many pages had some. */
  removeStamps: (doc: number, kind: string, pages: number[] | null) => invoke<number>("remove_stamps", { doc, kind, pages }),
  firstPageSize: (path: string) => invoke<[number, number]>("first_page_size", { path }),
  /** Replaces the bookmarks (one undo step). */
  setOutline: (doc: number, items: OutlineItem[]) => invoke<void>("set_outline", { doc, items }),
  pageLabelRanges: (doc: number) => invoke<LabelRange[]>("page_label_ranges", { doc }),
  /** Replaces the page labels (one undo step). */
  setPageLabels: (doc: number, ranges: LabelRange[]) => invoke<void>("set_page_labels", { doc, ranges }),
  properties: (doc: number) => invoke<Properties>("properties", { doc }),
  /** Sets title, author, subject and keywords (one undo step). */
  setProperties: (doc: number, props: Properties) => invoke<void>("set_properties", { doc, props }),
  saveWith: (doc: number, order: number[], rot: number[], target: string | null, opts: SaveOptions) =>
    invoke<DocInfo>("save_with", { doc, order, rot, target, opts }),
  security: (doc: number) => invoke<Security>("security", { doc }),
  pictures: (doc: number, page: number) => invoke<Picture[]>("pictures", { doc, page }),
  /** fmt 0 as stored, 1 PNG, 2 PDF. */
  pictureBytes: (doc: number, page: number, idx: number, fmt: number) => invoke<ArrayBuffer>("picture_bytes", { doc, page, idx, fmt }),
  /** Returns the path used (its extension follows the format). */
  pictureSave: (doc: number, page: number, idx: number, fmt: number, path: string) => invoke<string>("picture_save", { doc, page, idx, fmt, path }),
  savePictures: (doc: number, pages: number[], names: string[], min: number, fmt: number, dir: string, stem: string) =>
    invoke<{ saved: number; small: number; repeats: number }>("save_pictures", { doc, pages, names, min, fmt, dir, stem }),
  areaBytes: (doc: number, page: number, area: [number, number, number, number], rot: number, kind: "png" | "svg" | "pdf", dpi: number) =>
    invoke<ArrayBuffer>("area_bytes", { doc, page, area, rot, kind, dpi }),
  areaSave: (doc: number, page: number, area: [number, number, number, number], rot: number, kind: "png" | "svg" | "pdf", dpi: number, path: string) =>
    invoke<void>("area_save", { doc, page, area, rot, kind, dpi, path }),
  areaText: (doc: number, page: number, area: [number, number, number, number]) => invoke<string>("area_text", { doc, page, area }),
  attachments: (doc: number) => invoke<Attachment[]>("attachments", { doc }),
  /** Saves at `path`, or into folder `dir` under its own name; returns the path used. */
  attachmentSave: (doc: number, i: number, path: string | null, dir: string | null) => invoke<string>("attachment_save", { doc, i, path, dir }),
  attachmentOpen: (doc: number, i: number) => invoke<void>("attachment_open", { doc, i }),
  /** Checks the permissions password; when right the app keeps it, so the protection can be changed. */
  checkOwner: (doc: number, password: string) => invoke<boolean>("check_owner", { doc, password }),
  existingFiles: (paths: string[]) => invoke<string[]>("existing_files", { paths }),
  peekFile: (path: string, password?: string) => invoke<Peek>("peek_file", { path, password: password ?? null }),
  pageLabel: (doc: number, page: number) => invoke<string>("page_label", { doc, page }),
  pageLabels: (doc: number) => invoke<string[] | null>("page_labels", { doc }),
  outline: (doc: number) => invoke<OutlineItem[]>("outline", { doc }),
  links: (doc: number, page: number) => invoke<LinkItem[]>("page_links", { doc, page }),
  images: (doc: number, page: number) => invoke<[number, number, number, number][]>("page_images", { doc, page }),
  select: (doc: number, page: number, a: [number, number], b: [number, number], mode: 0 | 1 | 2) =>
    invoke<Quad[]>("text_select", { doc, page, a, b, mode }),
  copy: (doc: number, page: number, a: [number, number], b: [number, number]) =>
    invoke<string>("text_copy", { doc, page, a, b }),
  pageText: (doc: number, page: number) => invoke<string>("page_text", { doc, page }),
  textStats: (doc: number, page: number) => invoke<{ chars: number }>("page_text_stats", { doc, page }),
  search: (doc: number, needle: string, opts: EngineSearchOptions, sid: number, onEvent: (e: SearchEvent) => void) => {
    const ch = new Channel<SearchEvent>();
    ch.onmessage = onEvent;
    return invoke<void>("search", { doc, needle, opts, sid, onEvent: ch });
  },
  /** Streams every page's reflow content (JSON per page), starting at `start`. */
  reflow: (doc: number, start: number, sid: number, onEvent: (e: ReflowEvent) => void) => {
    const ch = new Channel<ReflowEvent>();
    ch.onmessage = onEvent;
    return invoke<void>("reflow", { doc, start, sid, onEvent: ch });
  },
  annots: (doc: number, page: number) => invoke<string>("annots", { doc, page }).then((j) => JSON.parse(j) as Annot[]),
  docAnnots: (doc: number) =>
    invoke<string>("doc_annots", { doc }).then((j) => JSON.parse(j) as { p: number; n: number; t: string; text: string; author: string; date: number }[]),
  annotEdit: (doc: number, page: number, req: { num?: number; kind?: string; props?: AnnotProps; delete?: boolean; author?: string }) =>
    invoke<{ num: number; changed: boolean }>("annot_edit", { doc, page, ...req }),
  userName: () => invoke<string>("user_name"),
  formsInit: (doc: number) => invoke<{ fields: number; xfa: boolean }>("forms_init", { doc }),
  widgets: (doc: number, page: number) => invoke<string>("widgets", { doc, page }).then((j) => JSON.parse(j) as Widget[]),
  widgetSet: (doc: number, page: number, num: number, value: string) => invoke<FormChange>("widget_set", { doc, page, num, value }),
  widgetToggle: (doc: number, page: number, num: number) => invoke<FormChange>("widget_toggle", { doc, page, num }),
  docUndo: (doc: number, redo: boolean) => invoke<FormChange>("doc_undo", { doc, redo }),
  formReset: (doc: number) => invoke<FormChange>("form_reset", { doc }),
  saveFlattened: (doc: number, target: string, order: number[], rot: number[], annots: boolean) =>
    invoke<void>("save_flattened", { doc, target, order, rot, annots }),
  ocrLanguages: () => invoke<import("./ocr").OcrLang[]>("ocr_languages"),
  ocrDownload: (code: string, quality: string, onEvent: (e: { kind: "progress"; done: number; total: number }) => void) => {
    const ch = new Channel<{ kind: "progress"; done: number; total: number }>();
    ch.onmessage = onEvent;
    return invoke<void>("ocr_download", { code, quality, onEvent: ch });
  },
  ocrDownloadStop: (code: string, quality: string) => invoke<void>("ocr_download_stop", { code, quality }),
  ocrDelete: (code: string, quality: string) => invoke<void>("ocr_delete", { code, quality }),
  ocrCheck: (doc: number) => invoke<{ sampled: number; scans: number }>("ocr_check", { doc }),
  /** Recognises `pages` (indices in the file); cancelling `sid` stops it and keeps what was read. */
  ocrRun: (doc: number, pages: number[], langs: string[], quality: string, sid: number, onEvent: (e: OcrEvent) => void) => {
    const ch = new Channel<OcrEvent>();
    ch.onmessage = onEvent;
    return invoke<void>("ocr_run", { doc, pages, langs, quality, sid, onEvent: ch });
  },
  ocrRemove: (doc: number) => invoke<{ pages: number; dirty: boolean }>("ocr_remove", { doc }),
  cancel: (ids: number[]) => (ids.length ? invoke<void>("cancel", { ids }) : Promise.resolve()),
  systemInfo: () => invoke<SystemInfo>("system_info"),
  takeLaunchFiles: () => invoke<string[]>("take_launch_files"),
};

let nextId = 1;
/** Ids for cancellable requests (renders, searches). Never 0 (reserved for "anonymous"). */
export const newId = () => nextId++;

/** Base URL of the custom pixel protocol (Windows serves custom schemes over http://<scheme>.localhost). */
const RENDER_BASE = navigator.userAgent.includes("Windows") ? "http://paginaut.localhost/render" : "paginaut://localhost/render";

export interface RenderParams {
  doc: number;
  page: number;
  scale: number;
  rotate: number;
  clip?: [number, number, number, number];
  /** false: page contents only; true (default): with annotations and fields; "nofields": without fillable fields. */
  extras?: boolean | "nofields";
  id: number;
  prio: number;
}

export interface RenderedBitmap {
  image: ImageData;
  width: number;
  height: number;
}

/** Fetches rendered pixels. Resolves null if the render was cancelled.
 *  Pixels stay as plain ImageData: drawing them with putImageData into CPU
 *  canvases uses far less memory in WebView2 than GPU-backed ImageBitmaps. */
export async function fetchRender(p: RenderParams, signal?: AbortSignal): Promise<RenderedBitmap | null> {
  let url = `${RENDER_BASE}?d=${p.doc}&p=${p.page}&s=${p.scale.toFixed(4)}&r=${p.rotate}&e=${p.extras === false ? 0 : p.extras === "nofields" ? 2 : 1}&id=${p.id}&q=${p.prio}`;
  if (p.clip) url += `&c=${p.clip.map((v) => Math.round(v)).join(",")}`;
  const res = await fetch(url, { signal, cache: "no-store" });
  if (res.status === 204) return null;
  if (!res.ok) throw new Error(await res.text());
  const buf = await res.arrayBuffer();
  const head = new DataView(buf, 0, 8);
  const width = head.getUint32(0, true);
  const height = head.getUint32(4, true);
  const image = new ImageData(new Uint8ClampedArray(buf, 8, width * height * 4), width, height);
  return { image, width, height };
}
