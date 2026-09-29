// Edit text: paragraphs outlined on the page; click one to change its words in place.
import { effect, signal } from "@preact/signals";
import { api, type Paragraph } from "./api";
import { DOC_STEP } from "./edits";
import { formsChanged } from "./forms";
import type { LayerPage, PageLayer } from "./render/Viewer";
import { showToast, type Tab } from "./store";

/** The tab whose text is being edited (Tools › Edit text), or null. */
export const textEdit = signal<Tab | null>(null);

let layer: EditLayer | null = null;

export function toggleTextEdit(tab: Tab) {
  if (textEdit.value === tab) return stopTextEdit();
  if (!tab.info.value?.isPdf) return showToast("Only PDFs can be edited. Save this file as a PDF first (Save as).", 4500);
  if (tab.mode.value !== "pages") return showToast("Switch to Single or Double view to edit text.", 4000);
  stopTextEdit();
  textEdit.value = tab;
  layer = new EditLayer(tab);
  tab.viewer?.addLayer(layer);
  document.body.classList.add("text-edit");
  showToast("Click a paragraph to change it · Esc to stop", 3500);
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

const cache = new Map<string, Promise<Paragraph[]>>();

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

class EditLayer implements PageLayer {
  z = 25;
  private open: { src: number; para: Paragraph; box: HTMLTextAreaElement } | null = null;
  constructor(private tab: Tab) {}

  mount(page: LayerPage): HTMLElement {
    const root = document.createElement("div");
    root.className = "te-root";
    void paragraphsOf(this.tab, page.src).then((paras) => {
      if (!paras.length && page.view === this.tab.page.value)
        showToast("No text to edit on this page. Scans need text recognition first (OCR).", 4500);
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
        root.append(b);
      }
    });
    return root;
  }

  unmount(page: LayerPage) {
    if (this.open?.src === page.src) this.commit();
  }

  private edit(root: HTMLElement, src: number, p: Paragraph) {
    this.commit();
    const box = document.createElement("textarea");
    box.className = "te-edit";
    box.value = p.t;
    box.spellcheck = true;
    const family = p.mono ? "monospace" : p.serif ? "'Times New Roman', serif" : "Arial, sans-serif";
    Object.assign(box.style, {
      left: `${p.r[0] - 2}px`,
      top: `${p.r[1] - 2}px`,
      width: `${p.r[2] - p.r[0] + p.s * 0.3 + 4}px`,
      minHeight: `${p.r[3] - p.r[1] + 4}px`,
      fontSize: `${p.s}px`,
      lineHeight: `${Math.max(p.s * 0.8, p.lead)}px`,
      fontFamily: family,
      fontWeight: p.b ? "700" : "400",
      fontStyle: p.i ? "italic" : "normal",
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
      }
    });
    box.addEventListener("pointerdown", (e) => e.stopPropagation());
    box.addEventListener("blur", () => this.commit());
    root.append(box);
    this.open = { src, para: p, box };
    requestAnimationFrame(() => {
      grow();
      box.focus();
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
    const text = o.box.value.replace(/\s+$/, "");
    const before = o.para.t.replace(/\s+/g, " ").trim();
    if (text.replace(/\s+/g, " ").trim() === before) return void o.box.remove();
    o.box.readOnly = true;
    const tab = this.tab;
    api
      .editText(tab.info.value!.docId, o.src, { para: o.para, text })
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
