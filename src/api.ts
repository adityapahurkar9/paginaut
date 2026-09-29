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
  cancel: (ids: number[]) => (ids.length ? invoke<void>("cancel", { ids }) : Promise.resolve()),
  systemInfo: () => invoke<SystemInfo>("system_info"),
  takeLaunchFiles: () => invoke<string[]>("take_launch_files"),
};

let nextId = 1;
/** Ids for cancellable requests (renders, searches). Never 0 (reserved for "anonymous"). */
export const newId = () => nextId++;

/** Base URL of the custom pixel protocol (Windows serves custom schemes over http://<scheme>.localhost). */
const RENDER_BASE = navigator.userAgent.includes("Windows") ? "http://folio.localhost/render" : "folio://localhost/render";

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
