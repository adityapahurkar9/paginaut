// Filling PDF forms. The page is rendered without its fillable fields and
// this layer draws them as real inputs in their place (like Chrome's viewer):
// typing is instant and there's no flicker between your text and the PDF's.
// Every change goes to the engine, which runs the form's own scripts
// (formatting, validation, calculated totals) and records an undo step.
import { api, type FormChange, type Widget } from "./api";
import { bumpRev } from "./render/RenderManager";
import type { LayerPage, PageLayer } from "./render/Viewer";
import { DOC_STEP } from "./edits";
import { refreshAnnots } from "./annots";
import { settings, showToast, trackEdit, type Tab } from "./store";

let listIds = 0;
const FF_READONLY = 1, FF_REQUIRED = 2, FF_MULTILINE = 1 << 12, FF_PASSWORD = 1 << 13, FF_COMB = 1 << 24, FF_EDIT = 1 << 18;

type FieldEl = HTMLElement & { _w?: Widget; _src?: number };

/** What clicking a signature field does (set by the signing module). */
let signatureClick: ((tab: Tab, src: number, w: Widget) => void) | null = null;
export function onSignatureClick(fn: (tab: Tab, src: number, w: Widget) => void) {
  signatureClick = fn;
}

export class FormLayer implements PageLayer {
  z = 10;
  private roots = new Map<HTMLElement, LayerPage>();
  private cache = new Map<number, Promise<Widget[]>>();

  constructor(private tab: Tab) {}

  private get docId() {
    return this.tab.info.value!.docId;
  }

  private widgets(src: number): Promise<Widget[]> {
    let p = this.cache.get(src);
    if (!p) {
      p = api.widgets(this.docId, src).catch(() => []);
      this.cache.set(src, p);
    }
    return p;
  }

  mount(page: LayerPage): HTMLElement | null {
    const root = document.createElement("div");
    root.className = "fw-root";
    this.roots.set(root, page);
    void this.widgets(page.src).then((ws) => root.isConnected !== false && this.fill(root, page, ws));
    return root;
  }

  unmount(_page: LayerPage, el: HTMLElement) {
    this.roots.delete(el);
  }

  /** Values changed in the engine (a calculation, undo…): update the fields in view, in place. */
  refresh() {
    this.cache.clear();
    for (const [root, page] of this.roots) void this.widgets(page.src).then((ws) => this.fill(root, page, ws));
  }

  /** Creates the page's inputs, or updates existing ones without disturbing the one you're typing in. */
  private fill(root: HTMLElement, page: LayerPage, ws: Widget[]) {
    const existing = new Map<string, FieldEl>();
    for (const el of root.querySelectorAll<FieldEl>("[data-w]")) existing.set(el.dataset.w!, el);
    for (const w of ws) {
      if (w.t === "button" || w.t === "unknown") continue;
      const key = String(w.n);
      let el = existing.get(key);
      if (!el) {
        el = this.create(w, page.src);
        el.dataset.w = key;
        root.append(el);
      }
      existing.delete(key);
      this.update(el, w);
    }
    for (const el of existing.values()) el.remove();
  }

  private create(w: Widget, src: number): FieldEl {
    const [x0, y0, x1, y1] = w.r;
    const h = y1 - y0;
    let el: FieldEl;
    const ro = !!w.ro || (w.ff & FF_READONLY) !== 0;
    switch (w.t) {
      case "text": {
        const multi = (w.ff & FF_MULTILINE) !== 0;
        const input = document.createElement(multi ? "textarea" : "input") as HTMLInputElement | HTMLTextAreaElement;
        if (input instanceof HTMLInputElement) input.type = w.ff & FF_PASSWORD ? "password" : "text";
        if (w.max && w.max > 0) input.maxLength = w.max;
        input.readOnly = ro;
        input.spellcheck = false;
        const size = w.fs > 0 ? w.fs : multi ? 11 : Math.max(6, Math.min(12, h * 0.62));
        input.style.fontSize = `${size}px`;
        input.style.textAlign = ["left", "center", "right"][w.q] ?? "left";
        if (w.ff & FF_COMB && w.max) {
          // One character per box.
          const cell = (x1 - x0) / w.max;
          input.style.letterSpacing = `${cell - size * 0.6}px`;
          input.style.paddingLeft = `${Math.max(0, (cell - size * 0.6) / 2)}px`;
          input.style.fontFamily = "Consolas, monospace";
        }
        const commit = () => {
          const cur = (el as FieldEl)._w;
          if (!cur || input.value === cur.v || input.value === cur.fv) return;
          void this.set(src, cur, input.value, () => (input.value = cur.v));
        };
        // Edit the plain value; show it formatted the rest of the time.
        input.addEventListener("focus", () => {
          const cur = (el as FieldEl)._w;
          if (cur?.fv !== undefined && input.value === cur.fv) input.value = cur.v;
        });
        input.addEventListener("blur", () => {
          const cur = (el as FieldEl)._w;
          if (cur?.fv !== undefined && input.value === cur.v) input.value = cur.fv;
        });
        input.addEventListener("change", commit);
        input.addEventListener("keydown", (e) => {
          const ke = e as KeyboardEvent;
          if (ke.key === "Enter" && (!multi || ke.ctrlKey)) {
            ke.preventDefault();
            commit();
          } else if (ke.key === "Escape") {
            input.value = (el as FieldEl)._w?.v ?? "";
            input.blur();
          }
        });
        el = input as FieldEl;
        break;
      }
      case "checkbox":
      case "radio": {
        const b = document.createElement("button");
        b.type = "button";
        b.disabled = ro;
        b.setAttribute("role", w.t);
        b.addEventListener("click", () => {
          const cur = (el as FieldEl)._w;
          if (cur) void this.toggle(src, cur);
        });
        el = b as FieldEl;
        break;
      }
      case "combo":
      case "list": {
        const editable = w.t === "combo" && (w.ff & FF_EDIT) !== 0;
        if (editable) {
          const input = document.createElement("input");
          input.readOnly = ro;
          const list = document.createElement("datalist");
          // One list per field shown: a field's number repeats from page to page.
          list.id = `fw-list-${listIds++}`;
          input.setAttribute("list", list.id);
          input.addEventListener("change", () => {
            const cur = (el as FieldEl)._w;
            if (cur && input.value !== cur.v) void this.set(src, cur, input.value, () => (input.value = cur.v));
          });
          const wrap = document.createElement("div");
          wrap.append(input, list);
          el = wrap as FieldEl;
        } else {
          const sel = document.createElement("select");
          sel.disabled = ro;
          if (w.t === "list") sel.size = Math.max(2, Math.floor(h / 13));
          sel.addEventListener("change", () => {
            const cur = (el as FieldEl)._w;
            if (cur) void this.set(src, cur, sel.value, () => (sel.value = cur.v));
          });
          el = sel as FieldEl;
        }
        el.style.fontSize = `${w.fs > 0 ? w.fs : Math.max(6, Math.min(12, h * 0.62))}px`;
        break;
      }
      case "signature": {
        const b = document.createElement("button");
        b.type = "button";
        b.textContent = w.signed ? "" : "Sign here";
        b.title = w.signed ? "Signed: click for details" : "Click to sign with your certificate (DSC token)";
        b.addEventListener("click", () => signatureClick?.(this.tab, src, (b as FieldEl)._w ?? w));
        el = b as FieldEl;
        break;
      }
      default:
        el = document.createElement("div") as FieldEl;
    }
    el.classList.add("fw", `fw-${w.t}`);
    if (ro) el.classList.add("ro");
    if (w.ff & FF_REQUIRED) el.classList.add("req");
    el.style.left = `${x0}px`;
    el.style.top = `${y0}px`;
    el.style.width = `${x1 - x0}px`;
    el.style.height = `${h}px`;
    el.title = w.name + (w.ff & FF_REQUIRED ? " (required)" : "");
    el._src = src;
    return el;
  }

  /** Shows the engine's current value (formatted by the form's scripts). */
  private update(el: FieldEl, w: Widget) {
    const old = el._w;
    el._w = w;
    const focused = document.activeElement === el || el.contains(document.activeElement);
    if (w.t === "text") {
      const input = el as unknown as HTMLInputElement;
      // A focused field keeps what you're typing, unless it still shows the old value (undo, a calculation).
      const untouched = !old || input.value === old.v || input.value === old.fv;
      if (!focused) input.value = w.fv ?? w.v;
      else if (untouched) input.value = w.v;
    }
    else if (w.t === "checkbox" || w.t === "radio") {
      el.classList.toggle("on", !!w.on);
      el.setAttribute("aria-checked", String(!!w.on));
    } else if (w.t === "combo" || w.t === "list") {
      const sel = el.querySelector("datalist") ? null : (el as unknown as HTMLSelectElement);
      const opts = w.opts ?? [];
      if (sel) {
        const sig = JSON.stringify(opts);
        if (sel.dataset.opts !== sig) {
          sel.replaceChildren(...opts.map(([label, exp]) => new Option(label, exp || label)));
          sel.dataset.opts = sig;
        }
        // A value that isn't one of the choices (typed into an editable list elsewhere).
        if (w.v && ![...sel.options].some((o) => o.value === w.v)) sel.append(new Option(w.v, w.v));
        if (!focused) sel.value = w.v;
      } else {
        const input = el.querySelector("input")!;
        el.querySelector("datalist")!.replaceChildren(...opts.map(([label]) => new Option(label)));
        if (!focused) input.value = w.v;
      }
    }
  }

  private set(src: number, w: Widget, value: string, revert: () => void) {
    return trackEdit(this.setNow(src, w, value, revert));
  }

  private async setNow(src: number, w: Widget, value: string, revert: () => void) {
    let r: FormChange;
    try {
      r = await api.widgetSet(this.docId, src, w.n, value);
    } catch (e) {
      revert();
      showToast(`Couldn't fill in ${w.name}: ${e}`);
      return;
    }
    if (!r.accepted) {
      revert();
      showToast(`That isn't a valid value for "${w.name}"`);
    }
    this.changed(r);
  }

  private async toggle(src: number, w: Widget) {
    try {
      this.changed(await api.widgetToggle(this.docId, src, w.n));
    } catch (e) {
      showToast(`Couldn't change ${w.name}: ${e}`);
    }
  }

  /** After an edit: one undo step, the pages and fields show the new values. */
  private changed(r: FormChange) {
    if (!r.accepted) return;
    const tab = this.tab;
    tab.history.push(DOC_STEP);
    tab.historyVersion.value++;
    formsChanged(tab, r.changed);
  }
}

const layers = new WeakMap<Tab, FormLayer>();

/** Everything drawn from the document is stale (a form edit, undo/redo). */
export function formsChanged(tab: Tab, docChanged: boolean) {
  tab.docChanged.value = docChanged;
  bumpRev(tab.info.value!.docId);
  tab.contentRev.value++;
  tab.viewer?.schedule(true);
  layers.get(tab)?.refresh();
  refreshAnnots(tab);
}

/** Checks a freshly opened (or re-opened after saving) PDF for form fields and attaches the layer. */
export async function initForms(tab: Tab) {
  const info = tab.info.value;
  if (!info?.isPdf) return;
  let res;
  try {
    res = await api.formsInit(info.docId);
  } catch {
    return;
  }
  tab.forms.value = res;
  const viewer = tab.viewer;
  const old = layers.get(tab);
  if (old) viewer?.removeLayer(old);
  layers.delete(tab);
  if (!res.fields || !viewer) {
    viewer?.setFieldsDrawn(false);
    return;
  }
  const layer = new FormLayer(tab);
  layers.set(tab, layer);
  viewer.setFieldsDrawn(true);
  viewer.addLayer(layer);
}

export async function clearForm(tab: Tab) {
  const info = tab.info.value;
  if (!info) return;
  try {
    const r = await api.formReset(info.docId);
    tab.history.push(DOC_STEP);
    tab.historyVersion.value++;
    formsChanged(tab, r.changed);
    showToast("Form cleared · Ctrl+Z to undo");
  } catch (e) {
    showToast(`Couldn't clear the form: ${e}`);
  }
}

/** Field highlight follows the setting. */
export function fieldHighlightClass() {
  return settings.value.highlightFields ? "fields-hl" : "";
}
