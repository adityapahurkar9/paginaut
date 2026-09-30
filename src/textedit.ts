// Edit text: paragraphs outlined on the page; click one to change its words in place.
import { effect, signal } from "@preact/signals";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type Paragraph, type Picture } from "./api";
import { DOC_STEP } from "./edits";
import { formsChanged } from "./forms";
import type { LayerPage, PageLayer } from "./render/Viewer";
import { showToast, type Tab } from "./store";

/** The tab whose text is being edited (Tools › Edit text), or null. */
export const textEdit = signal<Tab | null>(null);

let layer: EditLayer | null = null;

type Run = [string, number];
const BOLD = 1, ITALIC = 2;

/** A paragraph's text in runs of one style (1 bold, 2 italic): as found, or all in the paragraph's style. */
function runsOfParagraph(p: Paragraph): Run[] {
  return p.u?.length ? p.u.map(([t, st]) => [t, st] as Run) : [[p.t, (p.b ? BOLD : 0) | (p.i ? ITALIC : 0)]];
}

/** The editor's content in runs of one style (<b>/<strong>, <i>/<em>, or inline weight and style); line breaks
 * and new lines (Enter makes <br>; a pasted block <div>) as "\n". Adjacent runs of a style are joined. */
function runsOfEditor(root: HTMLElement): Run[] {
  const out: Run[] = [];
  const push = (t: string, st: number) => {
    if (!t) return;
    const last = out[out.length - 1];
    if (last && last[1] === st) last[0] += t;
    else out.push([t, st]);
  };
  const walk = (n: Node, st: number) => {
    if (n.nodeType === Node.TEXT_NODE) return push((n as Text).data.replace(/\u00a0/g, " "), st);
    if (!(n instanceof HTMLElement)) return;
    if (n.tagName === "BR") return push("\n", st);
    let s = st;
    if (n.tagName === "B" || n.tagName === "STRONG") s |= BOLD;
    if (n.tagName === "I" || n.tagName === "EM") s |= ITALIC;
    const fw = n.style.fontWeight, fs = n.style.fontStyle;
    if (fw) s = fw === "bold" || Number(fw) >= 600 ? s | BOLD : s & ~BOLD;
    if (fs) s = fs === "italic" || fs === "oblique" ? s | ITALIC : s & ~ITALIC;
    if ((n.tagName === "DIV" || n.tagName === "P") && out.length && !out[out.length - 1][0].endsWith("\n")) push("\n", st);
    n.childNodes.forEach((c) => walk(c, s));
  };
  root.childNodes.forEach((c) => walk(c, 0));
  return out;
}

/** Runs compared by what they say and how: the text with whitespace squeezed, and each letter's style. */
function runsKey(runs: Run[]) {
  let text = "", styles = "";
  for (const [t, st] of runs)
    for (const ch of t) {
      if (/\s/.test(ch)) text += text.endsWith(" ") ? "" : " ";
      else (text += ch), (styles += st);
    }
  return `${text.trim()}|${styles}`;
}

export function toggleTextEdit(tab: Tab) {
  if (textEdit.value === tab) return stopTextEdit();
  if (!tab.info.value?.isPdf) return showToast("Only PDFs can be edited. Save this file as a PDF first (Save as).", 4500);
  if (tab.mode.value !== "pages") return showToast("Switch to Single or Double view to edit text.", 4000);
  stopTextEdit();
  textEdit.value = tab;
  layer = new EditLayer(tab);
  tab.viewer?.addLayer(layer);
  document.body.classList.add("text-edit");
  showToast("Click a paragraph to change its words, or a picture to move, resize or replace it · Esc to stop", 4000);
}

export function stopTextEdit() {
  const tab = textEdit.value;
  if (layer && tab) {
    layer.commit();
    tab.viewer?.removeLayer(layer);
  }
  layer = null;
  textEdit.value = null;
  document.body.classList.remove("text-edit");
}

// Paragraphs come back fresh after every change to the page.
effect(() => {
  const tab = textEdit.value;
  if (!tab) return;
  void tab.contentRev.value;
  queueMicrotask(() => layer && tab.viewer?.refreshLayer(layer));
});

window.addEventListener(
  "keydown",
  (e) => {
    if (e.key === "Escape" && textEdit.value && !(e.target as HTMLElement).closest?.(".te-edit")) {
      e.stopPropagation();
      stopTextEdit();
    }
  },
  true,
);

type Box = [number, number, number, number];

const PICTURE_FILES = [{ name: "Pictures", extensions: ["png", "jpg", "jpeg", "jfif", "gif", "bmp", "webp"] }];

/** Asks for a picture file; returns it with its size in pixels. */
async function pickPicture(title: string): Promise<{ path: string; w: number; h: number } | null> {
  const path = (window as unknown as { paginautTest?: { pick?: string[] } }).paginautTest?.pick?.[0] ?? (await open({ title, filters: PICTURE_FILES }));
  if (typeof path !== "string") return null;
  try {
    const bmp = await createImageBitmap(new Blob([await api.readPicture(path)]));
    const out = { path, w: bmp.width, h: bmp.height };
    bmp.close();
    return out;
  } catch (e) {
    showToast(`Couldn't read that picture: ${e}`, 5000);
    return null;
  }
}

/** A w×h picture as large as fits in `box`, keeping its shape, centred. */
function fitIn(box: Box, w: number, h: number): Box {
  const bw = box[2] - box[0], bh = box[3] - box[1];
  const k = Math.min(bw / w, bh / h);
  const [nw, nh] = [w * k, h * k];
  const x = box[0] + (bw - nw) / 2, y = box[1] + (bh - nh) / 2;
  return [x, y, x + nw, y + nh];
}

/** Changes a picture (one undo step) and redraws the page. */
async function changePicture(tab: Tab, src: number, target: Box, action: 0 | 1 | 2 | 3, dest: Box, path: string | null, done: string) {
  try {
    await api.pictureEdit(tab.info.value!.docId, src, target, action, dest, path);
    tab.history.push(DOC_STEP);
    tab.historyVersion.value++;
    formsChanged(tab, true);
    showToast(`${done} · Ctrl+Z to undo`);
  } catch (e) {
    showToast(`Couldn't change the picture: ${e}`, 6000);
  }
}

/** Tools › Insert › Picture: on the page in view, as large as fits in the middle 60% of it. */
export async function addPicture(tab: Tab) {
  if (!tab.info.value?.isPdf) return showToast("Only PDFs can be edited. Save this file as a PDF first (Save as).", 4500);
  const pic = await pickPicture("Add a picture");
  if (!pic) return;
  const view = tab.page.value;
  const src = tab.edits.value?.order[view] ?? view;
  const [w, h] = (await api.pageSizes(tab.info.value.docId))[src] ?? [595, 842];
  // At most its own size at 96 dpi, so a small logo stays small.
  const natural: Box = [0, 0, (pic.w * 72) / 96, (pic.h * 72) / 96];
  const room: Box = [w * 0.2, h * 0.2, w * 0.8, h * 0.8];
  let dest = fitIn(room, pic.w, pic.h);
  if (dest[2] - dest[0] > natural[2]) dest = fitIn([(w - natural[2]) / 2, (h - natural[3]) / 2, (w + natural[2]) / 2, (h + natural[3]) / 2], pic.w, pic.h);
  await changePicture(tab, src, [0, 0, 0, 0], 3, dest, pic.path, "Picture added");
  if (textEdit.value !== tab) toggleTextEdit(tab);
}

const cache = new Map<string, Promise<Paragraph[]>>();
const picCache = new Map<string, Promise<Picture[]>>();

function picturesOf(tab: Tab, src: number): Promise<Picture[]> {
  const info = tab.info.value!;
  const key = `${info.docId}:${src}:${tab.contentRev.value}`;
  let p = picCache.get(key);
  if (!p) {
    if (picCache.size > 64) picCache.clear();
    p = api.pictures(info.docId, src, true).catch(() => []);
    picCache.set(key, p);
  }
  return p;
}

function paragraphsOf(tab: Tab, src: number): Promise<Paragraph[]> {
  const info = tab.info.value!;
  const key = `${info.docId}:${src}:${tab.contentRev.value}`;
  let p = cache.get(key);
  if (!p) {
    if (cache.size > 64) cache.clear();
    p = api.paragraphs(info.docId, src).catch(() => []);
    cache.set(key, p);
  }
  return p;
}

function place(el: HTMLElement, r: Box) {
  Object.assign(el.style, { left: `${r[0]}px`, top: `${r[1]}px`, width: `${r[2] - r[0]}px`, height: `${r[3] - r[1]}px` });
}

class EditLayer implements PageLayer {
  z = 25;
  private open: { src: number; para: Paragraph; box: HTMLDivElement } | null = null;
  constructor(private tab: Tab) {}

  mount(page: LayerPage): HTMLElement {
    const root = document.createElement("div");
    root.className = "te-root";
    void Promise.all([paragraphsOf(this.tab, page.src), picturesOf(this.tab, page.src)]).then(([paras, pics]) => {
      if (!paras.length && !pics.length && page.view === this.tab.page.value)
        showToast("Nothing to edit on this page. Scans need text recognition first (OCR).", 4500);
      // Stacked as the page draws them: picture i after the paragraphs with z <= i.
      const boxes: { z: number; el: HTMLElement }[] = [];
      for (const [i, pic] of pics.entries()) {
        const b = document.createElement("div");
        b.className = "te-pic";
        place(b, pic.r);
        b.title = "Click to move, resize, replace or delete";
        b.addEventListener("pointerdown", (e) => {
          if (e.button !== 0) return;
          e.stopPropagation();
          e.preventDefault();
          this.commit();
          this.select(root, page, pic.r, e);
        });
        boxes.push({ z: i + 0.5, el: b });
      }
      for (const p of paras) {
        const b = document.createElement("div");
        b.className = "te-para";
        Object.assign(b.style, { left: `${p.r[0] - 2}px`, top: `${p.r[1] - 2}px`, width: `${p.r[2] - p.r[0] + 4}px`, height: `${p.r[3] - p.r[1] + 4}px` });
        b.title = "Click to edit";
        b.addEventListener("pointerdown", (e) => {
          if (e.button !== 0) return;
          e.stopPropagation();
          e.preventDefault();
          this.edit(root, page.src, p);
        });
        boxes.push({ z: p.z, el: b });
      }
      boxes.sort((a, b) => a.z - b.z);
      root.append(...boxes.map((b) => b.el));
    });
    return root;
  }

  /** A picture picked: a box with handles to move and resize it, and a bar for the rest. */
  private select(root: HTMLElement, page: LayerPage, r: Box, start?: PointerEvent) {
    root.querySelector(".te-sel")?.remove();
    const tab = this.tab;
    const sel = document.createElement("div");
    sel.className = "te-sel";
    place(sel, r);
    let box: Box = [...r];
    const handles = ["tl", "t", "tr", "r", "br", "b", "bl", "l"];
    for (const h of handles) {
      const el = document.createElement("span");
      el.className = `te-h te-${h}`;
      el.addEventListener("pointerdown", (e) => this.drag(e, page, sel, h, () => box, (b) => (box = b), r));
      sel.append(el);
    }
    const bar = document.createElement("div");
    bar.className = "te-bar";
    const button = (label: string, run: () => void) => {
      const b = document.createElement("button");
      b.textContent = label;
      b.addEventListener("pointerdown", (e) => e.stopPropagation());
      b.addEventListener("click", (e) => {
        e.stopPropagation();
        run();
      });
      bar.append(b);
    };
    button("Replace…", async () => {
      const pic = await pickPicture("Replace the picture with");
      if (pic) void changePicture(tab, page.src, r, 2, fitIn(r, pic.w, pic.h), pic.path, "Picture replaced");
    });
    button("Delete", () => void changePicture(tab, page.src, r, 0, [0, 0, 0, 0], null, "Picture deleted"));
    button("Done", () => sel.remove());
    sel.append(bar);
    sel.addEventListener("pointerdown", (e) => this.drag(e, page, sel, "move", () => box, (b) => (box = b), r));
    sel.tabIndex = -1;
    sel.addEventListener("keydown", (e) => {
      if (e.key === "Delete" || e.key === "Backspace") {
        e.stopPropagation();
        void changePicture(tab, page.src, r, 0, [0, 0, 0, 0], null, "Picture deleted");
      } else if (e.key === "Escape") {
        e.stopPropagation();
        sel.remove();
      }
    });
    root.append(sel);
    sel.focus();
    if (start) this.drag(start, page, sel, "move", () => box, (b) => (box = b), r);
  }

  /** Moves (or with a handle, resizes) the picked picture; written into the page when let go. */
  private drag(e: PointerEvent, page: LayerPage, sel: HTMLElement, handle: string, get: () => Box, set: (b: Box) => void, target: Box) {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    const v = this.tab.viewer;
    const p0 = v?.clientToPage(page.view, e.clientX, e.clientY);
    if (!v || !p0) return;
    const b0 = get();
    const aspect = (b0[2] - b0[0]) / Math.max(0.01, b0[3] - b0[1]);
    let moved = false;
    const move = (ev: PointerEvent) => {
      const p = v.clientToPage(page.view, ev.clientX, ev.clientY);
      if (!p) return;
      const dx = p[0] - p0[0], dy = p[1] - p0[1];
      if (Math.abs(dx) + Math.abs(dy) > 0.5) moved = true;
      let [x0, y0, x1, y1] = b0;
      if (handle === "move") {
        const w = x1 - x0, h = y1 - y0;
        x0 = Math.max(-w / 2, Math.min(page.w - w / 2, x0 + dx));
        y0 = Math.max(-h / 2, Math.min(page.h - h / 2, y0 + dy));
        [x1, y1] = [x0 + w, y0 + h];
      } else {
        if (handle.includes("l")) x0 = Math.min(x1 - 4, x0 + dx);
        if (handle.includes("r")) x1 = Math.max(x0 + 4, x1 + dx);
        if (handle.includes("t")) y0 = Math.min(y1 - 4, y0 + dy);
        if (handle.includes("b")) y1 = Math.max(y0 + 4, y1 + dy);
        // Corners keep the picture's shape (Shift frees them).
        if (handle.length === 2 && !ev.shiftKey) {
          const w = x1 - x0, h = w / aspect;
          if (handle.includes("t")) y0 = y1 - h;
          else y1 = y0 + h;
        }
      }
      set([x0, y0, x1, y1]);
      place(sel, [x0, y0, x1, y1]);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      if (moved) void changePicture(this.tab, page.src, target, 1, get(), null, handle === "move" ? "Picture moved" : "Picture resized");
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  unmount(page: LayerPage) {
    if (this.open?.src === page.src) this.commit();
  }

  private edit(root: HTMLElement, src: number, p: Paragraph) {
    this.commit();
    // Rich text: bold and italic words (Ctrl+B, Ctrl+I) are kept as they're found and can be changed.
    const box = document.createElement("div");
    box.className = "te-edit";
    box.contentEditable = "true";
    box.spellcheck = true;
    box.title = "Ctrl+B bold · Ctrl+I italic · Ctrl+Enter done · Esc cancel";
    for (const [t, st] of runsOfParagraph(p)) {
      let node: Node = document.createTextNode(t);
      if (st & ITALIC) {
        const el = document.createElement("i");
        el.append(node);
        node = el;
      }
      if (st & BOLD) {
        const el = document.createElement("b");
        el.append(node);
        node = el;
      }
      box.append(node);
    }
    const family = p.mono ? "monospace" : p.serif ? "'Times New Roman', serif" : "Arial, sans-serif";
    Object.assign(box.style, {
      left: `${p.r[0] - 2}px`,
      top: `${p.r[1] - 2}px`,
      width: `${p.r[2] - p.r[0] + p.s * 0.3 + 4}px`,
      minHeight: `${p.r[3] - p.r[1] + 4}px`,
      fontSize: `${p.s}px`,
      lineHeight: `${Math.max(p.s * 0.8, p.lead)}px`,
      fontFamily: family,
      fontWeight: "400",
      fontStyle: "normal",
      color: p.c,
      textAlign: ({ l: "left", r: "right", c: "center", j: "justify" } as const)[p.a] ?? "left",
    });
    const grow = () => {
      box.style.height = "auto";
      box.style.height = `${box.scrollHeight}px`;
    };
    box.addEventListener("input", grow);
    box.addEventListener("keydown", (e) => {
      e.stopPropagation();
      if (e.key === "Escape") {
        e.preventDefault();
        this.cancel();
      } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        this.commit();
      } else if (e.key === "Enter") {
        // A line break, not a new block.
        e.preventDefault();
        document.execCommand("insertLineBreak");
      } else if ((e.ctrlKey || e.metaKey) && !e.altKey && (e.key.toLowerCase() === "b" || e.key.toLowerCase() === "i")) {
        e.preventDefault();
        document.execCommand(e.key.toLowerCase() === "b" ? "bold" : "italic");
      }
    });
    // Pasted text comes in plain (its letters, not the other document's look).
    box.addEventListener("paste", (e) => {
      e.preventDefault();
      document.execCommand("insertText", false, e.clipboardData?.getData("text/plain") ?? "");
    });
    box.addEventListener("pointerdown", (e) => e.stopPropagation());
    box.addEventListener("blur", () => this.commit());
    root.append(box);
    this.open = { src, para: p, box };
    requestAnimationFrame(() => {
      grow();
      box.focus();
      // The caret at the end, as a text box would have it.
      const range = document.createRange();
      range.selectNodeContents(box);
      range.collapse(false);
      getSelection()?.removeAllRanges();
      getSelection()?.addRange(range);
    });
  }

  private cancel() {
    const o = this.open;
    this.open = null;
    o?.box.remove();
  }

  /** Writes the open editor's text into the page (nothing if it wasn't changed). */
  commit() {
    const o = this.open;
    if (!o) return;
    this.open = null;
    const runs = runsOfEditor(o.box);
    const text = runs.map((r) => r[0]).join("").replace(/\s+$/, "");
    if (runsKey(runs) === runsKey(runsOfParagraph(o.para))) return void o.box.remove();
    o.box.contentEditable = "false";
    const tab = this.tab;
    // Runs only when the styles are mixed or changed; plain text keeps the paragraph's own weight and style.
    const base = (o.para.b ? BOLD : 0) | (o.para.i ? ITALIC : 0);
    const styled = runs.some(([t, st]) => t.trim() && st !== base);
    api
      .editText(tab.info.value!.docId, o.src, { para: o.para, text, runs: styled ? runs : undefined })
      .then(() => {
        tab.history.push(DOC_STEP);
        tab.historyVersion.value++;
        formsChanged(tab, true);
        showToast(text.trim() ? "Text changed · Ctrl+Z to undo" : "Paragraph removed · Ctrl+Z to undo");
      })
      .catch((e) => showToast(`Couldn't change the text: ${e}`, 6000))
      .finally(() => o.box.remove());
  }
}
