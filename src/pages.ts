// Page tools: rotate, delete, reorder, undo/redo, save. Edits change the
// tab's Edits snapshot; the viewer and thumbnails redraw from it instantly
// and nothing touches the file until the user saves.
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api, type DocInfo } from "./api";
import * as E from "./edits";
import { formsChanged, initForms } from "./forms";
import { baseName, rememberFile, setSaveHook, settings, showToast, type Tab } from "./store";

/** Pages an action applies to: the thumbnail selection if it includes `page`, else just `page`. */
export function targets(tab: Tab, page?: number): number[] {
  const sel = tab.pageSel.value;
  const p = page ?? tab.page.value;
  if (sel.size > 1 && sel.has(p)) return [...sel].sort((a, b) => a - b);
  if (page === undefined && sel.size > 0) return [...sel].sort((a, b) => a - b);
  return [p];
}

function apply(tab: Tab, next: E.Edits, keepPage?: number) {
  const cur = tab.edits.value;
  if (!cur) return;
  tab.history.push(cur);
  tab.historyVersion.value++;
  setEdits(tab, next, keepPage);
}

function setEdits(tab: Tab, next: E.Edits, keepPage?: number) {
  tab.edits.value = next;
  tab.pageCount.value = next.order.length;
  const specs = E.specs(next, tab.sizes.value ?? []);
  tab.viewer?.setPages(specs, keepPage ?? Math.min(tab.page.value, specs.length - 1));
}

let autoSaveTimer = 0;
function maybeAutoSave(tab: Tab) {
  clearTimeout(autoSaveTimer);
  const e = tab.edits.value;
  if (!settings.value.autoSaveRotations || !e || !tab.info.value?.isPdf || !E.onlyRotations(e) || E.isIdentity(e)) return;
  autoSaveTimer = window.setTimeout(() => void save(tab), 1200);
}

export function rotatePages(tab: Tab, by: 90 | -90 | 180, page?: number) {
  const e = tab.edits.value;
  if (!e) return;
  apply(tab, E.rotate(e, targets(tab, page), by));
  maybeAutoSave(tab);
}

export function deletePages(tab: Tab, page?: number) {
  const e = tab.edits.value;
  if (!e) return;
  const views = targets(tab, page);
  if (views.length >= e.order.length) {
    showToast("A document needs at least one page");
    return;
  }
  const first = Math.min(...views);
  apply(tab, E.remove(e, views), Math.min(first, e.order.length - views.length - 1));
  tab.pageSel.value = new Set();
  showToast(views.length === 1 ? `Page deleted · Ctrl+Z to undo` : `${views.length} pages deleted · Ctrl+Z to undo`);
}

export function movePages(tab: Tab, views: number[], to: number) {
  const e = tab.edits.value;
  if (!e || !views.length) return;
  const next = E.move(e, views, to);
  if (next.order.every((p, i) => p === e.order[i])) return;
  // New position of the first moved page = number of unmoved pages before the gap.
  const set = new Set(views);
  let start = 0;
  for (let i = 0; i < to; i++) if (!set.has(i)) start++;
  apply(tab, next, start);
  tab.pageSel.value = new Set(views.map((_, k) => start + k));
}

/** Undo/redo of a form or annotation edit: the engine steps its journal. */
async function docStep(tab: Tab, redo: boolean) {
  const info = tab.info.value;
  if (!info) return;
  try {
    const r = await api.docUndo(info.docId, redo);
    // The step may have been recognised text.
    tab.textRev.value++;
    formsChanged(tab, r.changed);
  } catch (e) {
    showToast(`Couldn't ${redo ? "redo" : "undo"}: ${e}`);
  }
}

export function undo(tab: Tab) {
  const e = tab.edits.value;
  const prev = e && tab.history.undo(e);
  if (!prev) return;
  tab.historyVersion.value++;
  if (prev === E.DOC_STEP) return void docStep(tab, false);
  setEdits(tab, prev);
  tab.pageSel.value = new Set();
  maybeAutoSave(tab);
}

export function redo(tab: Tab) {
  const e = tab.edits.value;
  const next = e && tab.history.redo(e);
  if (!next) return;
  tab.historyVersion.value++;
  if (next === E.DOC_STEP) return void docStep(tab, true);
  setEdits(tab, next);
  tab.pageSel.value = new Set();
  maybeAutoSave(tab);
}

/**
 * Saves the tab. PDFs save in place (only changes are appended); other formats,
 * or "Save as", ask for a file name and write a new PDF. Resolves true on success.
 */
/** The tab shows the file just saved (or signed): fresh pages, no undo history. */
export async function adoptSaved(tab: Tab, saved: DocInfo, page?: number) {
  const sizes = await api.pageSizes(saved.docId);
  if (page === undefined) page = tab.viewer?.viewState().page;
  tab.viewer?.reset(saved);
  tab.info.value = saved;
  tab.path = saved.path;
  tab.sizes.value = sizes;
  tab.history.clear();
  tab.historyVersion.value++;
  tab.pageSel.value = new Set();
  setEdits(tab, E.identity(saved.pageCount), page);
  tab.docChanged.value = false;
  void initForms(tab);
  afterOpenHooks.forEach((f) => f(tab));
  rememberFile(saved.path, { pages: saved.pageCount });
  api.outline(saved.docId).then((o) => (tab.outline.value = o)).catch(() => {});
  api.pageLabels(saved.docId).then((l) => (tab.labels.value = l)).catch(() => {});
}

/** Run when a document is ready or was saved (e.g. to check its signatures). */
const afterOpenHooks: ((tab: Tab) => void)[] = [];
export function onDocReady(fn: (tab: Tab) => void) {
  afterOpenHooks.push(fn);
}
export const runDocReady = (tab: Tab) => afterOpenHooks.forEach((f) => f(tab));

/** Set by ocr.ts: runs before saving (e.g. to stop text recognition); false cancels the save. */
let beforeSave: (tab: Tab) => Promise<boolean> = async () => true;
export function setBeforeSave(fn: (tab: Tab) => Promise<boolean>) {
  beforeSave = fn;
}

export async function save(tab: Tab, saveAs = false): Promise<boolean> {
  if (!(await beforeSave(tab))) return false;
  const info = tab.info.value;
  const e = tab.edits.value;
  if (!info || !e || tab.saving.value) return false;
  let target: string | null = null;
  if (saveAs || !info.isPdf) {
    const stem = baseName(tab.path).replace(/\.[^.]+$/, "");
    target = await saveDialog({
      title: info.isPdf ? "Save a copy as" : "Save as PDF",
      defaultPath: tab.path.replace(/[^\\/]+$/, `${stem}${saveAs && info.isPdf ? " (copy)" : ""}.pdf`),
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (!target) return false;
  } else if (E.isIdentity(e) && !tab.docChanged.value) {
    return true;
  }
  tab.saving.value = true;
  const t0 = performance.now();
  try {
    const view = tab.viewer?.viewState();
    const saved = await api.save(info.docId, e.order, e.rot, target);
    await adoptSaved(tab, saved, view?.page);
    showToast(`Saved ${baseName(saved.path)} · ${Math.round(performance.now() - t0)} ms`);
    return true;
  } catch (err) {
    showToast(`Couldn't save: ${err}`, 5000);
    return false;
  } finally {
    tab.saving.value = false;
  }
}

/**
 * A copy that can't be changed by ordinary editing: form fields become part of
 * the page, and with `annots` so do comments and placed signatures.
 */
export async function saveFlattened(tab: Tab, annots = false) {
  const info = tab.info.value;
  const e = tab.edits.value;
  if (!info || !e) return;
  const stem = baseName(tab.path).replace(/\.[^.]+$/, "");
  const target = await saveDialog({
    title: annots ? "Save a locked copy" : "Save a flattened copy",
    defaultPath: tab.path.replace(/[^\\/]+$/, `${stem} (${annots ? "locked" : "flattened"}).pdf`),
    filters: [{ name: "PDF", extensions: ["pdf"] }],
  });
  if (!target) return;
  try {
    await api.saveFlattened(info.docId, target, e.order, e.rot, annots);
    showToast(`Saved ${baseName(target)}`);
  } catch (err) {
    showToast(`Couldn't save: ${err}`, 5000);
  }
}

setSaveHook((tab) => save(tab));
