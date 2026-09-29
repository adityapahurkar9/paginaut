// "Fix layout": the reading layout drawn over the real pages, so you can see
// what Read and Book modes will do and correct it where it goes wrong:
// numbered boxes in reading order (blue text, orange pictures, grey left out),
// click a box to change what it is, click boxes in turn to set the order,
// or draw a rectangle to make a region a picture, text, or left out.
import { signal } from "@preact/signals";
import type { LayerPage, PageLayer, Viewer } from "../render/Viewer";
import type { Tab } from "../store";
import { overridesOf, addRegion, setKind, updatePage } from "./overrides";
import type { Analysis, Box, ItemKind, LayoutItem } from "./model";

export type LayoutTool = "select" | "order" | "picture" | "ignore" | "text";

export interface LayoutEditState {
  tab: Tab;
  /** Mode to return to when done. */
  back: "read" | "book";
  tool: LayoutTool;
}

export const layoutEdit = signal<LayoutEditState | null>(null);

const KIND_LABEL: Record<ItemKind, string> = { text: "Text", picture: "Picture", ignore: "Leave out" };

export class LayoutLayer implements PageLayer {
  z = 20;
  /** Order being clicked, per file page. */
  private clicking = new Map<number, string[]>();
  private menu: HTMLElement | null = null;

  constructor(private tab: Tab, private viewer: Viewer, private analysis: Analysis) {}

  refresh() {
    this.closeMenu();
    this.viewer.refreshLayer(this);
  }

  private get tool(): LayoutTool {
    return layoutEdit.value?.tool ?? "select";
  }

  mount(page: LayerPage): HTMLElement | null {
    const ov = overridesOf(this.tab).value;
    const lay = this.analysis.layout(page.src, ov.pages?.[page.src], ov.columns);
    if (!lay) return null;
    const root = document.createElement("div");
    root.className = `lay-root tool-${this.tool}`;
    if (lay.scan) {
      const note = document.createElement("div");
      note.className = "lay-note";
      note.textContent = "Scanned page: shown as a picture until it has been through OCR";
      root.append(note);
    }
    const clicked = this.clicking.get(page.src);
    lay.items.forEach((it, i) => root.append(this.box(page, it, clicked ? clicked.indexOf(it.key) : i, !!clicked)));
    for (const it of lay.dropped) root.append(this.box(page, it, -1, false));
    // Drawing a region: drag on the page.
    if (this.tool === "picture" || this.tool === "ignore" || this.tool === "text") {
      root.classList.add("drawing");
      root.addEventListener("pointerdown", (e) => this.draw(e, page, root));
    }
    return root;
  }

  private box(page: LayerPage, it: LayoutItem, n: number, ordering: boolean) {
    const [x0, y0, x1, y1] = it.box;
    const el = document.createElement("div");
    el.className = `lay-box ${it.dropped ? "dropped" : it.kind === "pic" ? "pic" : "text"} ${it.manual ? "manual" : ""} ${ordering && n < 0 && !it.dropped ? "pending" : ""}`;
    el.style.left = `${x0}px`;
    el.style.top = `${y0}px`;
    el.style.width = `${Math.max(4, x1 - x0)}px`;
    el.style.height = `${Math.max(4, y1 - y0)}px`;
    const badge = document.createElement("span");
    badge.className = "lay-badge";
    badge.textContent = it.dropped ? (it.dropped === "header" ? "header/footer" : "left out") : n >= 0 ? String(n + 1) : "·";
    el.append(badge);
    el.title = it.dropped === "header"
      ? "Running header, footer or page number: left out automatically. Click to keep it as text."
      : it.dropped ? "Left out. Click to change." : it.kind === "pic" ? "Kept as a picture. Click to change." : "Text. Click to change.";
    el.addEventListener("pointerdown", (e) => {
      if (this.tool !== "select" && this.tool !== "order") return;
      e.stopPropagation();
      e.preventDefault();
      if (this.tool === "order") this.orderClick(page, it);
      else this.openMenu(page, it, el);
    });
    return el;
  }

  /** Set order: each click makes that item the next one. */
  private orderClick(page: LayerPage, it: LayoutItem) {
    if (it.dropped) return;
    const seq = this.clicking.get(page.src) ?? [];
    if (!seq.includes(it.key)) seq.push(it.key);
    this.clicking.set(page.src, seq);
    updatePage(this.tab, page.src, (p) => ({ ...p, order: [...seq] }));
    this.refresh();
  }

  /** Starts a fresh order on a page (next clicks set it from 1). */
  startOrder(src: number) {
    this.clicking.set(src, []);
    updatePage(this.tab, src, (p) => ({ ...p, order: undefined }));
    this.refresh();
  }

  endOrder() {
    this.clicking.clear();
    this.refresh();
  }

  private openMenu(page: LayerPage, it: LayoutItem, el: HTMLElement) {
    this.closeMenu();
    const menu = document.createElement("div");
    menu.className = "lay-menu";
    const current: ItemKind = it.dropped ? "ignore" : it.kind === "pic" ? "picture" : "text";
    for (const kind of ["text", "picture", "ignore"] as ItemKind[]) {
      const b = document.createElement("button");
      b.textContent = KIND_LABEL[kind];
      if (kind === current) b.className = "on";
      b.addEventListener("pointerdown", (e) => e.stopPropagation());
      b.addEventListener("click", (e) => {
        e.stopPropagation();
        setKind(this.tab, page.src, it.key, kind);
        this.refresh();
      });
      menu.append(b);
    }
    if (it.manual) {
      const b = document.createElement("button");
      b.textContent = "Automatic";
      b.addEventListener("pointerdown", (e) => e.stopPropagation());
      b.addEventListener("click", (e) => {
        e.stopPropagation();
        setKind(this.tab, page.src, it.key, null);
        // A region you drew is removed instead.
        updatePage(this.tab, page.src, (p) => ({ ...p, regions: (p.regions ?? []).filter((r) => keyOfBox(r.box) !== it.key) }));
        this.refresh();
      });
      menu.append(b);
    }
    menu.style.left = el.style.left;
    menu.style.top = `calc(${el.style.top} + ${el.style.height} + 4px)`;
    el.parentElement!.append(menu);
    this.menu = menu;
    const away = (e: PointerEvent) => {
      if (!menu.contains(e.target as Node)) this.closeMenu();
    };
    setTimeout(() => window.addEventListener("pointerdown", away, { once: true }), 0);
  }

  private closeMenu() {
    this.menu?.remove();
    this.menu = null;
  }

  private draw(e: PointerEvent, page: LayerPage, root: HTMLElement) {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    const start = this.viewer.clientToPage(page.view, e.clientX, e.clientY);
    if (!start) return;
    const rect = document.createElement("div");
    rect.className = `lay-draw ${this.tool}`;
    root.append(rect);
    let box: Box = [start[0], start[1], start[0], start[1]];
    const move = (ev: PointerEvent) => {
      const p = this.viewer.clientToPage(page.view, ev.clientX, ev.clientY);
      if (!p) return;
      box = [Math.min(start[0], p[0]), Math.min(start[1], p[1]), Math.max(start[0], p[0]), Math.max(start[1], p[1])];
      rect.style.left = `${box[0]}px`;
      rect.style.top = `${box[1]}px`;
      rect.style.width = `${box[2] - box[0]}px`;
      rect.style.height = `${box[3] - box[1]}px`;
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      rect.remove();
      if (box[2] - box[0] > 6 && box[3] - box[1] > 6) {
        addRegion(this.tab, page.src, box, this.tool as ItemKind);
        this.refresh();
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }
}

const keyOfBox = (b: Box) => `${Math.round(b[0])},${Math.round(b[1])},${Math.round(b[2])},${Math.round(b[3])}`;
