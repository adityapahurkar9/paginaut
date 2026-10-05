// Getting content out of a page: pictures (right-click), any area (drag a box), attachments.
import { effect, signal } from "@preact/signals";
import { open, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api, type Picture, type Table } from "./api";
import * as A from "./actions";
import { DOC_STEP, metaChanged } from "./edits";
import { formsChanged } from "./forms";
import { onDocReady } from "./pages";
import { activeTab, baseName, showToast, toast, updateSettings, type Tab } from "./store";
import { menu, openMenuAt } from "./ui/Toolbar";
import type { MenuItem } from "./ui/Overlays";
import { testHook } from "./ui/PagesUI";
import type { Block } from "./reflow/model";
import { readAloud } from "./speech";

const stemOf = (tab: Tab) => baseName(tab.path).replace(/\.[^.]+$/, "");
const srcOf = (tab: Tab, view: number) => tab.edits.value?.order[view] ?? view;
const labelOf = (tab: Tab, view: number) => tab.labels.value?.[srcOf(tab, view)] || String(view + 1);

/** Asks where to save (dev builds: the test hook answers). */
async function askPath(title: string, name: string, ext: string, kind: string): Promise<string | null> {
  const tab = activeTab.value;
  const dir = tab ? tab.path.replace(/[^\\/]+$/, "") : "";
  return testHook()?.save ?? (await saveDialog({ title, defaultPath: `${dir}${name}.${ext}`, filters: [{ name: kind, extensions: [ext] }] }));
}

/**
 * Puts `data` on the clipboard. The write starts right away, inside the click, with the data
 * still on its way: started later, the click no longer counts and WebView2 asks for permission.
 */
function copyLater(type: "image/png" | "text/plain", data: Promise<ArrayBuffer | string>, done: string) {
  const blob = data.then((d) => {
    if (typeof d === "string" && !d.trim()) throw new Error("There's no text in that area (scans need text recognition first).");
    return new Blob([d], { type });
  });
  navigator.clipboard.write([new ClipboardItem({ [type]: blob })]).then(
    () => showToast(done),
    (e) => showToast(`Couldn't copy: ${e instanceof Error ? e.message : e}`, 5000),
  );
}

// ---------------------------------------------------------------- pictures

const pictureCache = new Map<string, Promise<Picture[]>>();

function picturesOf(tab: Tab, src: number): Promise<Picture[]> {
  const info = tab.info.value!;
  const key = `${info.docId}:${src}:${tab.contentRev.value}`;
  let p = pictureCache.get(key);
  if (!p) {
    if (pictureCache.size > 64) pictureCache.clear();
    p = api.pictures(info.docId, src).catch(() => []);
    pictureCache.set(key, p);
  }
  return p;
}

const KIND: Record<string, [string, string]> = { jpg: ["JPEG", "jpg"], jp2: ["JPEG 2000", "jp2"], png: ["PNG", "png"] };

async function savePicture(tab: Tab, view: number, idx: number, pic: Picture, fmt: 0 | 1 | 2) {
  const [kind, ext] = fmt === 2 ? ["PDF", "pdf"] : fmt === 1 ? ["PNG", "png"] : KIND[pic.k] ?? KIND.png;
  const path = await askPath("Save the picture", `${stemOf(tab)} page ${labelOf(tab, view)} picture`, ext, kind);
  if (!path) return;
  try {
    const out = await api.pictureSave(tab.info.value!.docId, srcOf(tab, view), idx, fmt, path);
    showToast(`Saved ${baseName(out)}`);
  } catch (e) {
    showToast(`Couldn't save the picture: ${e}`, 5000);
  }
}

function copyPicture(tab: Tab, view: number, idx: number) {
  copyLater("image/png", api.pictureBytes(tab.info.value!.docId, srcOf(tab, view), idx, 1), "Picture copied");
}

function pictureItems(tab: Tab, view: number, idx: number, pic: Picture): MenuItem[] {
  const [kind] = KIND[pic.k] ?? KIND.png;
  return [
    { head: `Picture · ${pic.w} × ${pic.h} pixels` },
    { label: "Copy picture", run: () => copyPicture(tab, view, idx) },
    { label: `Save picture (${kind}, as stored)…`, run: () => void savePicture(tab, view, idx, pic, 0) },
    ...(pic.k !== "png" ? [{ label: "Save picture as PNG…", run: () => void savePicture(tab, view, idx, pic, 1) }] : []),
    { label: "Save picture as PDF…", run: () => void savePicture(tab, view, idx, pic, 2) },
  ];
}

// ---------------------------------------------------------------- tables

const tableCache = new Map<string, Promise<Table[]>>();

function tablesOf(tab: Tab, src: number): Promise<Table[]> {
  const info = tab.info.value!;
  const key = `${info.docId}:${src}:${tab.contentRev.value}`;
  let p = tableCache.get(key);
  if (!p) {
    if (tableCache.size > 64) tableCache.clear();
    p = api.tables(info.docId, src).catch(() => []);
    tableCache.set(key, p);
  }
  return p;
}

/** The table's text in rows and columns; rows with nothing in them left out. */
/** The table's rows as cells with how many columns each spans (cells covered by a spanning one left out). A cell
 * spanning rows keeps its text in its first row and blanks below, so no row shifts (Excel merges them anyway). */
export function spansOf(t: Table): { s: string; cs: number }[][] {
  const h = t.ys.length - 1, w = t.xs.length - 1;
  const rows = Array.from({ length: h }, () => Array<{ s: string; cs: number } | null>(w).fill(null));
  const covered = Array.from({ length: h }, () => Array<boolean>(w).fill(false));
  for (const [r, c, , cs, s] of t.cells) {
    if (r >= h || c >= w) continue;
    const cell = rows[r][c];
    rows[r][c] = cell ? { s: `${cell.s} ${s}`, cs: Math.max(cell.cs, cs) } : { s, cs: Math.min(cs, w - c) };
    for (let x = c + 1; x < Math.min(w, c + cs); x++) covered[r][x] = true;
  }
  return rows
    .map((row, r) => row.flatMap((cell, c) => (covered[r][c] ? [] : [cell ?? { s: "", cs: 1 }])))
    .filter((row) => row.some((cell) => cell.s.trim()));
}

export function gridOf(t: Table): string[][] {
  const h = t.ys.length - 1, w = t.xs.length - 1;
  const g = Array.from({ length: h }, () => Array<string>(w).fill(""));
  for (const [r, c, , , s] of t.cells) if (r < h && c < w) g[r][c] = g[r][c] ? `${g[r][c]} ${s}` : s;
  return g.filter((row) => row.some((s) => s.trim()));
}

const figure = /^[\s(₹$€£-]*[\d.,]+%?\)?\s*$/;

/** Joins two pieces of a heading: "operatio" + "ns" (a word cut at a line) without a space. */
const joinText = (a: string, b: string) => (!a ? b : !b ? a : /\p{Ll}$/u.test(a) && /^\p{Ll}{1,3}\b/u.test(b) && b.length <= 3 ? a + b : `${a} ${b}`);

/**
 * Headings set over several lines come out as several rows. This joins the rows above the
 * first row of figures into one heading row, column by column. Rows with a heading spread over
 * several columns (a group heading like "Three months ended June 30") stay rows of their own above it.
 */
export function joinHeading(t: Table): Table {
  const h = t.ys.length - 1, w = t.xs.length - 1;
  const g = Array.from({ length: h }, () => Array<string>(w).fill(""));
  for (const [r, c, , , s] of t.cells) if (r < h && c < w) g[r][c] = joinText(g[r][c], s);
  const d = g.findIndex((row) => {
    const full = row.filter((s) => s.trim());
    // Years (2025) head columns; they aren't figures.
    return full.length >= 2 && full.filter((s) => figure.test(s) && !/^\s*(19|20)\d\d\s*$/.test(s)).length * 2 >= full.length;
  });
  if (d < 2) return t;
  const group = [...new Set(t.cells.filter(([r, , , cs, s]) => r < d && cs > 1 && s.trim()).map(([r]) => r))].sort((a, b) => a - b);
  const rest = Array.from({ length: d }, (_, r) => r).filter((r) => !group.includes(r));
  const head = Array.from({ length: w }, (_, c) => rest.reduce((a, r) => joinText(a, g[r][c].trim()), ""));
  const cells: Table["cells"] = [];
  group.forEach((r, i) => {
    for (const [cr, c, , cs, s] of t.cells) if (cr === r) cells.push([i, c, 1, cs, s]);
  });
  const G = group.length;
  head.forEach((s, c) => s && cells.push([G, c, 1, 1, s]));
  for (const [r, c, rs, cs, s] of t.cells) if (r >= d) cells.push([r - d + G + 1, c, rs, cs, s]);
  return { ...t, ys: [t.ys[0], ...group.map((r) => t.ys[r + 1]), ...t.ys.slice(d)], cells };
}

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** Copies as a real table (cells in Excel, Word) and as tab-separated text. */
export function copyTable(t: Table) {
  const g = spansOf(t);
  const html = `<table>${g.map((row) => `<tr>${row.map((c) => `<td${c.cs > 1 ? ` colspan="${c.cs}"` : ""}>${esc(c.s)}</td>`).join("")}</tr>`).join("")}</table>`;
  const tsv = g.map((row) => row.map((c) => c.s.replace(/[\t\n]/g, " ") + "\t".repeat(c.cs - 1)).join("\t")).join("\r\n");
  navigator.clipboard
    .write([new ClipboardItem({ "text/html": new Blob([html], { type: "text/html" }), "text/plain": new Blob([tsv], { type: "text/plain" }) })])
    .then(() => showToast(`Table copied · ${g.length} rows · paste it into Excel or Word`), (e) => showToast(`Couldn't copy: ${e}`, 5000));
}

export async function saveTable(tab: Tab, view: number, t: Table, kind: "xlsx" | "csv") {
  const path = await askPath(kind === "xlsx" ? "Save the table as Excel" : "Save the table as CSV", `${stemOf(tab)} page ${labelOf(tab, view)} table`, kind, kind === "xlsx" ? "Excel workbook" : "CSV");
  if (!path) return;
  try {
    await api.tableSave(t, path);
    showToast(`Saved ${baseName(path)}`);
  } catch (e) {
    showToast(`Couldn't save the table: ${e}`, 5000);
  }
}

/** The table dialog: the table over its picture, with column lines to move. */
export const tableDialog = signal<{ tab: Tab; view: number; area: [number, number, number, number]; table: Table | null } | null>(null);

/** Opens the largest table on the page in view (Organize menu, command palette). */
export async function openPageTable(tab: Tab) {
  const view = tab.page.value;
  const tables = await tablesOf(tab, srcOf(tab, view));
  if (!tables.length) return showToast("No table found on this page. Select an area round one and choose Read as a table.", 5000);
  const t = tables.reduce((a, b) => (b.cells.length > a.cells.length ? b : a));
  tableDialog.value = { tab, view, area: t.r, table: t };
}

function tableItems(tab: Tab, view: number, found: Table): MenuItem[] {
  const t = joinHeading(found);
  const rows = gridOf(t).length, cols = t.xs.length - 1;
  return [
    { head: `Table · ${rows} rows × ${cols} columns` },
    { label: "Copy table", run: () => copyTable(t) },
    { label: "Save table as Excel…", run: () => void saveTable(tab, view, t, "xlsx") },
    { label: "Save table as CSV…", run: () => void saveTable(tab, view, t, "csv") },
    { label: "Check and adjust columns…", run: () => (tableDialog.value = { tab, view, area: found.r, table: found }) },
  ];
}

// ---------------------------------------------------------------- the page's right-click menu

/** Opens the menu for a right-click on page `view` at page point `pt`. */
async function pageMenu(tab: Tab, view: number, pt: [number, number], x: number, y: number) {
  const src = srcOf(tab, view);
  const [pics, tables] = await Promise.all([picturesOf(tab, src), tablesOf(tab, src)]);
  const table = tables.find((t) => pt[0] >= t.r[0] && pt[0] <= t.r[2] && pt[1] >= t.r[1] && pt[1] <= t.r[3]);
  // The smallest picture under the pointer (a photo on a scan, not the scan).
  let hit = -1;
  for (let i = 0; i < pics.length; i++) {
    const r = pics[i].r;
    if (pt[0] < r[0] || pt[0] > r[2] || pt[1] < r[1] || pt[1] > r[3]) continue;
    if (hit < 0 || (r[2] - r[0]) * (r[3] - r[1]) < (pics[hit].r[2] - pics[hit].r[0]) * (pics[hit].r[3] - pics[hit].r[1])) hit = i;
  }
  const items: MenuItem[] = [
    { label: "Copy", hint: "Ctrl+C", disabled: !tab.hasSelection.value, run: A.copySelection },
    { label: "Select paragraph", run: () => void tab.viewer?.selectParagraph(x, y) },
    { label: "Select page", hint: "Ctrl+A", run: () => tab.viewer?.selectAllOnPage(view) },
    { sep: true },
  ];
  if (table) items.push(...tableItems(tab, view, table), { sep: true });
  if (hit >= 0 && !table) items.push(...pictureItems(tab, view, hit, pics[hit]), { sep: true });
  items.push(
    { label: "Select an area to copy or save…", run: () => startArea(tab) },
    { label: "Save all pictures…", run: () => (picturesDialog.value = tab) },
  );
  if (tab.attachments.value?.length) items.push({ label: `Attachments (${tab.attachments.value.length})`, run: showAttachments });
  openMenuAt(x, y, items, 280);
}

window.addEventListener("contextmenu", (e) => {
  const tab = activeTab.value;
  const v = tab?.viewer;
  const t = e.target as HTMLElement;
  if (!tab || !v || tab.status.value !== "ready" || !v.el.contains(t) || t.closest("input, textarea, .fw")) return;
  const pg = t.closest<HTMLElement>(".pg");
  if (!pg) return;
  const view = Number(pg.dataset.index);
  const pt = v.clientToPage(view, e.clientX, e.clientY);
  if (!pt) return;
  e.preventDefault();
  void pageMenu(tab, view, pt, e.clientX, e.clientY);
});

// ---------------------------------------------------------------- selecting text

/** Selects the shown text of the blocks `want` picks (a paragraph may be shown in several pieces). */
function selectBlocks(root: HTMLElement, want: (b: number) => boolean) {
  const els = [...root.querySelectorAll<HTMLElement>("[data-b]")].filter((e) => want(Number(e.dataset.b)));
  if (!els.length) return;
  const r = document.createRange();
  r.setStartBefore(els[0]);
  r.setEndAfter(els[els.length - 1]);
  getSelection()?.removeAllRanges();
  getSelection()?.addRange(r);
}

// ---------------------------------------------------------------- Read and Book modes' right-click menu

type Pic = Extract<Block, { k: "img" }>;

/** How much of the smaller of two boxes the other covers (0–1). */
function overlap(a: [number, number, number, number], b: [number, number, number, number]) {
  const w = Math.min(a[2], b[2]) - Math.max(a[0], b[0]), h = Math.min(a[3], b[3]) - Math.max(a[1], b[1]);
  if (w <= 0 || h <= 0) return 0;
  const area = (r: typeof a) => Math.max(1, (r[2] - r[0]) * (r[3] - r[1]));
  return (w * h) / Math.min(area(a), area(b));
}

/** A right-click in Read or Book mode: a table's menu on a table (they're shown as pictures of the page), and
 * reading aloud from the paragraph clicked. */
async function reflowMenu(tab: Tab, target: HTMLElement, x: number, y: number) {
  const view = tab.reflowView!;
  const at = target.closest<HTMLElement>("[data-b]");
  const block = at && !target.closest("figure") ? Number(at.dataset.b) : null;
  const sel = getSelection()?.toString() ?? "";
  const items: MenuItem[] = [
    { label: "Copy", hint: "Ctrl+C", disabled: !sel.trim(), run: () => void navigator.clipboard.writeText(sel).then(() => showToast("Copied")) },
    { label: "Select paragraph", disabled: !block, run: () => selectBlocks(view.el, (b) => b === block) },
    {
      label: "Select page",
      disabled: !block,
      // The document page the paragraph comes from (Read and Book reflow the pages).
      run: () => selectBlocks(view.el, (b) => view.flow.blocks[b]?.page === view.flow.blocks[block!]?.page),
    },
    { sep: true },
  ];
  const pic = (target.closest("figure.rf-img") as (HTMLElement & { _pic?: Block }) | null)?._pic;
  if (pic?.k === "img") {
    const p = pic as Pic;
    const tables = await tablesOf(tab, p.src);
    const table = tables.map((t) => [t, overlap(t.r, p.box)] as const).filter(([, o]) => o > 0.5).sort((a, b) => b[1] - a[1])[0]?.[0];
    if (table) items.push(...tableItems(tab, p.page, table), { sep: true });
    else if (p.kind === "table" || p.kind === "figure")
      items.push({ label: "Read as a table…", run: () => (tableDialog.value = { tab, view: p.page, area: p.box, table: null }) }, { sep: true });
  }
  if (block !== null) items.push({ label: "Read aloud from here", run: () => void readAloud(tab, block) });
  while (items.length && "sep" in items[items.length - 1]) items.pop();
  if (items.length) openMenuAt(x, y, items, 260);
}

window.addEventListener("contextmenu", (e) => {
  const tab = activeTab.value;
  const view = tab?.reflowView;
  const t = e.target as HTMLElement;
  if (!tab || !view || tab.status.value !== "ready" || !view.el.contains(t)) return;
  e.preventDefault();
  void reflowMenu(tab, t, e.clientX, e.clientY);
});

// ---------------------------------------------------------------- any area

export const picturesDialog = signal<Tab | null>(null);
export const areaMode = signal<Tab | null>(null);
let box: HTMLDivElement | null = null;
let boxShown = false;

let watching = false;

export function startArea(tab: Tab) {
  // Set up on first use (the toolbar's menu may not exist yet while modules load).
  if (!watching) {
    watching = true;
    // The box goes when its menu closes.
    effect(() => {
      if (!menu.value && boxShown) stopArea();
    });
  }
  areaMode.value = tab;
  document.body.classList.add("area-mode");
  showToast("Drag a box round what you want to copy or save · Esc to stop", 4000);
}

function stopArea() {
  areaMode.value = null;
  document.body.classList.remove("area-mode");
  box?.remove();
  box = null;
  boxShown = false;
}

window.addEventListener(
  "keydown",
  (e) => {
    if (e.key === "Escape" && areaMode.value && !menu.value) {
      e.stopPropagation();
      stopArea();
    }
  },
  true,
);

window.addEventListener(
  "pointerdown",
  (e) => {
    const tab = areaMode.value;
    const v = tab?.viewer;
    if (!tab || !v || e.button !== 0 || boxShown) return;
    const pg = (e.target as HTMLElement).closest<HTMLElement>(".pg");
    if (!pg || !v.el.contains(pg)) return;
    e.preventDefault();
    e.stopPropagation();
    const view = Number(pg.dataset.index);
    const sx = e.clientX, sy = e.clientY;
    const bounds = pg.getBoundingClientRect();
    const clampX = (x: number) => Math.max(bounds.left, Math.min(bounds.right, x));
    const clampY = (y: number) => Math.max(bounds.top, Math.min(bounds.bottom, y));
    box?.remove();
    box = document.createElement("div");
    box.className = "area-box";
    document.body.appendChild(box);
    const draw = (x: number, y: number) => {
      const [x0, x1] = [Math.min(sx, clampX(x)), Math.max(sx, clampX(x))];
      const [y0, y1] = [Math.min(sy, clampY(y)), Math.max(sy, clampY(y))];
      Object.assign(box!.style, { left: `${x0}px`, top: `${y0}px`, width: `${x1 - x0}px`, height: `${y1 - y0}px` });
    };
    draw(sx, sy);
    const move = (ev: PointerEvent) => draw(ev.clientX, ev.clientY);
    const up = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up, true);
      const a = v.clientToPage(view, sx, sy);
      const b = v.clientToPage(view, clampX(ev.clientX), clampY(ev.clientY));
      if (!a || !b || Math.abs(a[0] - b[0]) < 4 || Math.abs(a[1] - b[1]) < 4) {
        box?.remove();
        box = null;
        return;
      }
      const area: [number, number, number, number] = [Math.min(a[0], b[0]), Math.min(a[1], b[1]), Math.max(a[0], b[0]), Math.max(a[1], b[1])];
      boxShown = true;
      toast.value = null;
      openMenuAt(ev.clientX, ev.clientY, areaItems(tab, view, area), 250);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up, true);
  },
  true,
);

function areaItems(tab: Tab, view: number, area: [number, number, number, number]): MenuItem[] {
  const info = tab.info.value!;
  const src = srcOf(tab, view);
  const rot = tab.edits.value?.rot[src] ?? 0;
  const name = `${stemOf(tab)} page ${labelOf(tab, view)} area`;
  const save = async (kind: "pdf" | "svg" | "png", dpi = 72) => {
    const label = { pdf: "PDF", svg: "SVG", png: "PNG" }[kind];
    const path = await askPath(`Save the area as ${label}`, name, kind, label);
    if (!path) return;
    try {
      await api.areaSave(info.docId, src, area, rot, kind, dpi, path);
      showToast(`Saved ${baseName(path)}`);
    } catch (e) {
      showToast(`Couldn't save the area: ${e}`, 5000);
    }
  };
  return [
    { label: "Copy text", run: () => copyLater("text/plain", api.areaText(info.docId, src, area), "Text copied") },
    { label: "Copy as picture", run: () => copyLater("image/png", api.areaBytes(info.docId, src, area, rot, "png", 200), "Picture copied") },
    { label: "Read as a table…", run: () => (tableDialog.value = { tab, view, area, table: null }) },
    { sep: true },
    ...(info.isPdf ? [{ label: "Save as PDF (sharp at any size)…", run: () => void save("pdf") }] : []),
    { label: "Save as SVG (for drawings, charts)…", run: () => void save("svg") },
    { label: "Save as PNG · 150 dpi…", run: () => void save("png", 150) },
    { label: "Save as PNG · 300 dpi…", run: () => void save("png", 300) },
    { label: "Save as PNG · 600 dpi…", run: () => void save("png", 600) },
  ];
}

// ---------------------------------------------------------------- attachments

export function showAttachments() {
  updateSettings({ sidebarOpen: true, sidebarTab: "files" });
}

/** Reads the attachment list again (after a change, undo or redo). */
export async function refreshAttachments(tab: Tab) {
  const info = tab.info.value;
  if (!info?.isPdf) return;
  tab.attachments.value = await api.attachments(info.docId).catch(() => []);
}

/** Attaches files the user picks to the document (one undo step each). */
export async function attachFiles(tab: Tab) {
  const info = tab.info.value;
  if (!info?.isPdf) return showToast("Only PDFs can have attachments. Save this file as a PDF first (Save as).", 4500);
  const hook = testHook()?.pick;
  const picked = hook ?? (await open({ title: "Attach files", multiple: true }));
  const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
  if (!paths.length) return;
  let done = 0;
  for (const path of paths) {
    try {
      await api.attachmentAdd(info.docId, path);
      metaChanged(tab);
      done++;
    } catch (e) {
      showToast(`Couldn't attach ${baseName(path)}: ${e}`, 6000);
    }
  }
  if (!done) return;
  await refreshAttachments(tab);
  showAttachments();
  showToast(`Attached ${done === 1 ? baseName(paths[0]) : `${done} files`} · Ctrl+Z to undo`);
}

/** Removes an attachment (a pinned one takes its pin off the page, so the page is redrawn). */
export async function removeAttachment(tab: Tab, i: number, name: string, pinned: boolean) {
  try {
    await api.attachmentRemove(tab.info.value!.docId, i);
    if (pinned) {
      tab.history.push(DOC_STEP);
      tab.historyVersion.value++;
      formsChanged(tab, true);
    } else metaChanged(tab);
    await refreshAttachments(tab);
    showToast(`Removed ${name} · Ctrl+Z to undo`);
  } catch (e) {
    showToast(`Couldn't remove it: ${e}`, 5000);
  }
}

onDocReady((tab) => {
  const info = tab.info.value;
  tab.attachments.value = null;
  if (!info?.isPdf) return;
  api.attachments(info.docId).then((a) => (tab.attachments.value = a), () => (tab.attachments.value = []));
});
