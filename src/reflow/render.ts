// Flow blocks → DOM, shared by Read and Book modes, plus the pictures cut
// from the page (photos, tables, drawings, scans) as cached image URLs.
import { fetchRender, newId } from "../api";
import { BOLD, ITALIC, MONO, SUP, type Block, type Run } from "./model";

/** Runs between character offsets [from, to) of a block. */
export function sliceRuns(runs: Run[], from: number, to: number): Run[] {
  const out: Run[] = [];
  let pos = 0;
  for (const r of runs) {
    const end = pos + r.t.length;
    if (end > from && pos < to) out.push({ t: r.t.slice(Math.max(0, from - pos), Math.min(r.t.length, to - pos)), s: r.s });
    pos = end;
    if (pos >= to) break;
  }
  return out;
}

function appendRuns(el: HTMLElement, runs: Run[]) {
  for (const r of runs) {
    if (!r.s) {
      el.append(r.t);
      continue;
    }
    let node: HTMLElement = document.createElement(r.s & SUP ? "sup" : "span");
    if (r.s & BOLD) node.classList.add("b");
    if (r.s & ITALIC) node.classList.add("i");
    if (r.s & MONO) node.classList.add("m");
    node.textContent = r.t;
    el.append(node);
  }
}

export interface Pictures {
  /** Resolves to an object URL for the picture, rendered sharp for `cssWidth` pixels. */
  url(b: Extract<Block, { k: "img" }>, cssWidth: number): Promise<string>;
  dispose(): void;
}

/**
 * Element for block `b`, or for characters [from, to) of it (pages split
 * paragraphs). `index` is stored so positions can be found from the DOM.
 */
export function blockEl(b: Block, index: number, pics: Pictures, from = 0, to = Infinity): HTMLElement {
  let el: HTMLElement;
  if (b.k === "img") {
    el = document.createElement("figure");
    el.className = `rf-img ${b.kind}`;
    const [x0, y0, x1, y1] = b.box;
    const img = document.createElement("img");
    img.alt = "";
    img.decoding = "async";
    img.draggable = false;
    img.style.aspectRatio = `${x1 - x0} / ${y1 - y0}`;
    el.style.setProperty("--rel", String(b.rel));
    el.style.setProperty("--ar", String((x1 - x0) / (y1 - y0)));
    el.append(img);
    (el as HTMLElement & { _pic?: Block })._pic = b;
  } else {
    el = document.createElement(b.k === "h" ? `h${b.level + 1}` : "p");
    if (b.k === "li") el.className = "li";
    const partial = from > 0 || to < b.len;
    appendRuns(el, partial ? sliceRuns(b.runs, from, to) : b.runs);
    if (from > 0) el.classList.add("cont"); // continues from the previous page: no indent
    if (to < b.len) el.classList.add("split"); // continues on the next page: no justify-last quirks
  }
  el.dataset.b = String(index);
  if (from > 0) el.dataset.o = String(from);
  return el;
}

/** Starts loading pictures inside `root` that haven't been loaded yet. */
export function loadPictures(root: HTMLElement, pics: Pictures) {
  for (const fig of root.querySelectorAll<HTMLElement>("figure.rf-img")) {
    const img = fig.querySelector("img")!;
    if (img.src) continue;
    const b = (fig as HTMLElement & { _pic?: Block })._pic;
    if (!b || b.k !== "img") continue;
    const w = fig.clientWidth || 600;
    pics
      .url(b, w)
      .then((u) => {
        if (!img.src) img.src = u;
      })
      .catch(() => fig.classList.add("failed"));
  }
}

/** Renders page regions through the engine and keeps them as JPEG object URLs. */
export function createPictures(docId: number): Pictures {
  const cache = new Map<string, { width: number; url: Promise<string> }>();
  const urls: string[] = [];
  let disposed = false;
  return {
    url(b, cssWidth) {
      if (b.k !== "img") return Promise.reject();
      const key = `${b.src}|${b.box.join(",")}`;
      const want = Math.min(2400, Math.ceil(cssWidth * (window.devicePixelRatio || 1)));
      const hit = cache.get(key);
      if (hit && hit.width >= want * 0.8) return hit.url;
      const [x0, y0, x1, y1] = b.box;
      const scale = Math.min(4, Math.max(0.5, want / (x1 - x0)));
      const url = fetchRender({
        doc: docId, page: b.src, scale, rotate: 0, extras: false, id: newId(), prio: 1,
        clip: [x0 * scale, y0 * scale, x1 * scale, y1 * scale],
      }).then(async (r) => {
        if (!r || disposed) throw new Error("gone");
        const canvas = new OffscreenCanvas(r.width, r.height);
        canvas.getContext("2d")!.putImageData(r.image, 0, 0);
        const blob = await canvas.convertToBlob({ type: "image/jpeg", quality: 0.9 });
        const u = URL.createObjectURL(blob);
        urls.push(u);
        return u;
      });
      url.catch(() => cache.delete(key));
      cache.set(key, { width: want, url });
      return url;
    },
    dispose() {
      disposed = true;
      for (const u of urls) URL.revokeObjectURL(u);
      urls.length = 0;
      cache.clear();
    },
  };
}
