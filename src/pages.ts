// Page tools: rotate, delete, reorder, undo/redo, save. Edits change the
// tab's Edits snapshot; the viewer and thumbnails redraw from it instantly
// and nothing touches the file until the user saves.
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api, type DocInfo, type Inserted, type InsertSource, type SaveOptions } from "./api";
import * as E from "./edits";
import { metaChanged } from "./edits";
import { formsChanged, initForms } from "./forms";
import { ask, baseName, rememberFile, samePath, setSaveHook, settings, showToast, tabs, type Tab } from "./store";

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

/** Each tab's own wait before its rotations are saved (one for all would let a second tab's rotation cancel the
 *  first tab's save). */
const autoSaveTimers = new WeakMap<Tab, number>();
function maybeAutoSave(tab: Tab) {
  clearTimeout(autoSaveTimers.get(tab));
  const e = tab.edits.value;
  // Only rotations: other unsaved changes (inserted pages, forms) wait for Save.
  if (!settings.value.autoSaveRotations || !e || !savesInPlace(tab) || tab.docChanged.value || !E.onlyRotations(e) || E.isIdentity(e)) return;
  autoSaveTimers.set(
    tab,
    window.setTimeout(() => {
      // Closed (or moved to another window) meanwhile: nothing to save here.
      if (tabs.value.includes(tab)) void save(tab);
    }, 1200),
  );
}

export function rotatePages(tab: Tab, by: 90 | -90 | 180, page?: number) {
  const e = tab.edits.value;
  if (!e) return;
  apply(tab, E.rotate(e, targets(tab, page), by));
  maybeAutoSave(tab);
}

/** Sets how far pages (by index in the file) are turned, e.g. to show sideways scans upright; one undo step. */
export function turnSources(tab: Tab, turns: [number, number][]) {
  const e = tab.edits.value;
  if (!e) return;
  const rot = e.rot.slice();
  for (const [src, deg] of turns) if (src < rot.length) rot[src] = ((deg % 360) + 360) % 360;
  if (rot.every((r, i) => r === e.rot[i])) return;
  apply(tab, { order: e.order, rot });
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
    // The step may have been recognised text, bookmarks or page labels.
    tab.textRev.value++;
    formsChanged(tab, r.changed);
    if (info.isPdf) {
      api.outline(info.docId).then((o) => (tab.outline.value = o)).catch(() => {});
      api.pageLabels(info.docId).then((l) => (tab.labels.value = l)).catch(() => {});
      api.attachments(info.docId).then((a) => (tab.attachments.value = a)).catch(() => {});
      api.properties(info.docId).then((p) => {
        const cur = tab.info.value;
        if (cur?.docId === info.docId && (cur.title !== p.title || cur.author !== p.author)) tab.info.value = { ...cur, title: p.title, author: p.author };
      }).catch(() => {});
    }
  } catch (e) {
    showToast(`Couldn't ${redo ? "redo" : "undo"}: ${e}`);
  }
}

/**
 * What gets inserted: pages of files, blank pages of a size in points, or pages
 * of another open document (with the rotations they're shown with there).
 */
export type InsertWhat =
  | { sources: InsertSource[]; fit?: [number, number] | null }
  | { blank: [number, number]; count?: number }
  | { fromDoc: number; pages: number[]; rot?: number[] };

/**
 * Inserts pages at a gap in the displayed order (0 = before the first page) as
 * one undo step. Errors are thrown (a file may need a password: "PASSWORD:<path>").
 */
export async function insertPages(tab: Tab, at: number, what: InsertWhat): Promise<Inserted> {
  const info = tab.info.value;
  if (!info || !tab.edits.value) throw new Error("no document");
  if (!info.isPdf) throw new Error("Pages can only be added to a PDF. Save this file as a PDF first (Save as PDF).");
  const r =
    "blank" in what
      ? await api.insertBlank(info.docId, what.blank[0], what.blank[1], what.count ?? 1)
      : "fromDoc" in what
        ? await api.insertFromOpen(info.docId, what.fromDoc, what.pages)
        : await api.insertPages(info.docId, what.sources, what.fit ?? null);
  const e = tab.edits.value;
  if (!e || tab.info.value !== info) return r;
  const added = Array.from({ length: r.count }, (_, k) => r.first + k);
  const rot = e.rot.slice();
  while (rot.length < r.first + r.count) rot.push(0);
  if ("fromDoc" in what && what.rot) what.rot.forEach((d, k) => k < r.count && (rot[r.first + k] = d));
  const pos = Math.max(0, Math.min(at, e.order.length));
  const next = { order: [...e.order.slice(0, pos), ...added, ...e.order.slice(pos)], rot };
  if (tab.sizes.value) {
    const sizes = tab.sizes.value.slice();
    r.sizes.forEach((s, k) => (sizes[r.first + k] = s));
    tab.sizes.value = sizes;
  } else {
    tab.sizes.value = await api.pageSizes(info.docId);
  }
  tab.history.push({ doc: e });
  tab.historyVersion.value++;
  setEdits(tab, next, pos);
  tab.pageSel.value = new Set(added.map((_, k) => pos + k));
  pagesChanged(tab);
  return r;
}

/** Crops and/or resizes pages (document indices) as one undo step. */
export async function resizePages(
  tab: Tab,
  ops: { page: number; crop: [number, number, number, number]; size?: [number, number] }[],
  opts: { fit: number; anchor: [number, number]; remove: boolean; fill: number; color: [number, number, number] },
) {
  const info = tab.info.value;
  if (!info || !tab.edits.value) return;
  const sizes = await api.pageSize(info.docId, ops, opts);
  const e = tab.edits.value;
  applySizes(tab, ops.map((o) => o.page), sizes);
  tab.history.push({ doc: e, resized: ops.map((o) => o.page) });
  tab.historyVersion.value++;
  setEdits(tab, e);
  pagesChanged(tab);
}

function applySizes(tab: Tab, pages: number[], sizes: [number, number][]) {
  const next = (tab.sizes.value ?? []).slice();
  pages.forEach((p, k) => (next[p] = sizes[k]));
  tab.sizes.value = next;
}

/** After undo/redo of a size change: the pages' sizes as they are now. */
async function refreshSizes(tab: Tab, pages: number[] | undefined) {
  const info = tab.info.value;
  if (!pages?.length || !info) return;
  applySizes(tab, pages, await api.pageSizesOf(info.docId, pages));
  if (tab.edits.value) setEdits(tab, tab.edits.value);
}

/** The document's own pages changed (inserted, or that undone): text, fields and drawings are stale. */
function pagesChanged(tab: Tab) {
  tab.textRev.value++;
  formsChanged(tab, true);
  void initForms(tab);
}

export function undo(tab: Tab) {
  const e = tab.edits.value;
  const prev = e && tab.history.undo(e);
  if (!prev) return;
  tab.historyVersion.value++;
  if (prev === E.DOC_STEP) return void docStep(tab, false);
  if (E.withDoc(prev)) {
    // Drop the pages from view first, then from the document.
    setEdits(tab, prev.doc);
    tab.pageSel.value = new Set();
    return void docStep(tab, false).then(() => {
      void refreshSizes(tab, prev.resized);
      void initForms(tab);
    });
  }
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
  if (E.withDoc(next)) {
    // Put the pages back in the document first, then in view.
    return void docStep(tab, true).then(async () => {
      await refreshSizes(tab, next.resized);
      setEdits(tab, next.doc);
      tab.pageSel.value = new Set();
      void initForms(tab);
    });
  }
  setEdits(tab, next);
  tab.pageSel.value = new Set();
  maybeAutoSave(tab);
}

/** The tab shows the file just saved (or signed): fresh pages, no undo history. */
export async function adoptSaved(tab: Tab, saved: DocInfo, page?: number) {
  const sizes = await api.pageSizes(saved.docId);
  if (page === undefined) page = tab.viewer?.viewState().page;
  tab.viewer?.reset(saved);
  tab.info.value = saved;
  // A PDF made from an Office file goes by the Office file's name until it's saved as a PDF of its own.
  tab.path = saved.converted?.from ?? saved.path;
  tab.sizes.value = sizes;
  tab.history.clear();
  tab.historyVersion.value++;
  tab.pageSel.value = new Set();
  setEdits(tab, E.identity(saved.pageCount), page);
  tab.docChanged.value = false;
  void initForms(tab);
  afterOpenHooks.forEach((f) => f(tab));
  rememberFile(tab.path, { pages: saved.pageCount });
  api.outline(saved.docId).then((o) => (tab.outline.value = o)).catch(() => {});
  api.pageLabels(saved.docId).then((l) => (tab.labels.value = l)).catch(() => {});
}

/** Run when a document is ready or was saved (e.g. to check its signatures). */
const afterOpenHooks: ((tab: Tab) => void)[] = [];
export function onDocReady(fn: (tab: Tab) => void) {
  afterOpenHooks.push(fn);
}
export const runDocReady = (tab: Tab) => afterOpenHooks.forEach((f) => f(tab));

/**
 * Saving writes back to the file itself: a PDF opened as it is. Other files (and PDFs made from Office
 * files, which live in the app's cache) are saved as new PDFs.
 */
export const savesInPlace = (tab: Tab) => !!tab.info.value?.isPdf && !tab.info.value.converted;

/** Set by ocr.ts: runs before saving (e.g. to stop text recognition); false cancels the save. */
let beforeSave: (tab: Tab) => Promise<boolean> = async () => true;
export function setBeforeSave(fn: (tab: Tab) => Promise<boolean>) {
  beforeSave = fn;
}

/** Set by markdown.ts: saves a Markdown file's edited source instead (null: save as usual). */
let saveOverride: (tab: Tab, saveAs: boolean) => Promise<boolean | null> = async () => null;
export function setSaveOverride(fn: typeof saveOverride) {
  saveOverride = fn;
}

/**
 * Saves the tab. PDFs save in place (only changes are appended). Other formats (and PDFs made from Office files) ask
 * for a file name and write a new PDF, which the tab then shows ("Save as PDF"). `saveAs` on a PDF is "Save a copy":
 * a copy is written and the tab stays on its file, edits still to save there. Resolves true on success.
 */
export async function save(tab: Tab, saveAs = false): Promise<boolean> {
  // One save of a tab at a time, from its very start: the questions below take time, and a second save let in
  // meanwhile (Ctrl+S, the rotations' timer) would apply the same page changes to the file just saved.
  if (entered.has(tab) || tab.saving.value) return false;
  entered.add(tab);
  try {
    return await saveNow(tab, saveAs);
  } finally {
    entered.delete(tab);
  }
}
const entered = new WeakSet<Tab>();

async function saveNow(tab: Tab, saveAs: boolean): Promise<boolean> {
  const own = await saveOverride(tab, saveAs);
  if (own !== null) return own;
  if (!(await beforeSave(tab))) return false;
  const info = tab.info.value;
  const e = tab.edits.value;
  if (!info || !e || tab.saving.value) return false;
  let target: string | null = null;
  const inPlace = savesInPlace(tab);
  // "Save a copy as" writes a copy and stays on this file (its edits still to save here); "Save as PDF" of a file
  // that can't be saved in place moves the tab onto the new PDF.
  let copy = saveAs && inPlace;
  if (saveAs || !inPlace) {
    const stem = baseName(tab.path).replace(/\.[^.]+$/, "");
    target = await saveDialog({
      title: inPlace ? "Save a copy as" : "Save as PDF",
      defaultPath: tab.path.replace(/[^\\/]+$/, `${stem}${saveAs && inPlace ? " (copy)" : ""}.pdf`),
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (!target) return false;
    if (copy && samePath(target, tab.path)) {
      // A copy over the file itself is an ordinary save.
      target = null;
      copy = false;
      if (E.isIdentity(e) && !tab.docChanged.value) return true;
    } else if (tabs.value.some((t) => t !== tab && samePath(t.path, target!))) {
      showToast(`${baseName(target)} is open in Paginaut. Close it first, or choose another name.`, 5000);
      return false;
    }
  } else if (E.isIdentity(e) && !tab.docChanged.value) {
    return true;
  }
  // A file that says it's PDF/A: changes that would break that are said first, and can be put right as it saves.
  let keep = 0;
  if (info.isPdf) {
    const ch = await api.pdfaChanges(info.docId).catch(() => null);
    if (ch?.issues.length) {
      const go = await ask(
        `Keep it PDF/A-${ch.part}b?`,
        `This file says it's PDF/A-${ch.part}b, and your changes would break that:\n${ch.issues.map((i) => `• ${i.text}`).join("\n")}\n\n` +
          "Paginaut can put that right as it saves, with the same fixes as Save as PDF/A (signatures stay valid).",
        [
          { label: "Cancel", value: "cancel" },
          { label: "Save anyway", value: "anyway" },
          { label: "Keep it PDF/A", value: "keep", primary: true },
        ],
      );
      if (go !== "keep" && go !== "anyway") return false;
      if (go === "keep") keep = ch.part;
    }
  }
  tab.saving.value = true;
  const t0 = performance.now();
  try {
    const view = tab.viewer?.viewState();
    const kept = keep ? await api.pdfaKeep(info.docId, keep) : null;
    // The fixes are a step in the document's journal: one in the tab's history too, so a save that then fails
    // leaves Undo in step (it takes the fixes back, not your last edit).
    if (kept) metaChanged(tab);
    if (copy) {
      await api.saveParts(info.docId, [{ pages: e.order, path: target! }], e.rot);
      showToast(
        kept?.substitutes.length
          ? `Saved a copy as ${baseName(target!)} (PDF/A-${keep}b). Fonts stood in: ${kept.substitutes.join("; ")}`
          : `Saved a copy as ${baseName(target!)} · ${Math.round(performance.now() - t0)} ms`,
        kept?.substitutes.length ? 9000 : undefined,
      );
      return true;
    }
    const saved = await api.save(info.docId, e.order, e.rot, target);
    await adoptSaved(tab, saved, view?.page);
    if (keptNotSaved(tab, saved)) return false;
    const lost = e.order.length - saved.pageCount;
    // Checked again as saved: what Paginaut couldn't put right is said.
    const left = keep ? (await api.pdfaCheck(saved.docId, keep).catch(() => null))?.issues ?? [] : [];
    showToast(
      lost > 0
        ? `Saved ${baseName(saved.path)}. The file was damaged: ${lost} ${lost === 1 ? "page" : "pages"} couldn't be read and ${lost === 1 ? "was" : "were"} left out.`
        : left.length
          ? `Saved ${baseName(saved.path)}, but it's no longer PDF/A-${keep}b: ${left.map((i) => i.text).join("; ")}`
          : kept?.substitutes.length
            ? `Saved ${baseName(saved.path)} as PDF/A-${keep}b. Fonts stood in: ${kept.substitutes.join("; ")}`
            : `Saved ${baseName(saved.path)} · ${Math.round(performance.now() - t0)} ms`,
      lost > 0 || left.length || kept?.substitutes.length ? 9000 : undefined,
    );
    return true;
  } catch (err) {
    failedSave(tab);
    showToast(`Couldn't save: ${err}`, 5000);
    return false;
  } finally {
    tab.saving.value = false;
  }
}

/** A save failed: the document is as it was before (the engine took the save's own changes back), but what could be
 *  redone is gone. */
function failedSave(tab: Tab) {
  tab.history.dropRedo();
  tab.historyVersion.value++;
}

/**
 * A save that wrote everything into a copy beside the file but couldn't replace the file: the tab goes on from that
 * copy (as after a save), still with changes to save. True when that's what happened (said to the user).
 */
function keptNotSaved(tab: Tab, saved: DocInfo): boolean {
  if (!saved.notSaved) return false;
  tab.docChanged.value = true;
  showToast(`Couldn't save: ${saved.notSaved}`, 12000);
  return true;
}

/** Saves a repaired e-book (an EPUB made from a PDF): as that PDF, pages exactly as made, or as a
 *  flowing EPUB that any e-book reader shows properly (text and pictures, not the page layout). */
export async function saveRepaired(tab: Tab, pdf: boolean) {
  const info = tab.info.value;
  if (!info?.repaired) return;
  // As a PDF it's an ordinary Save as PDF: comments, filled-in fields and page changes go with it, and the tab
  // then shows the saved PDF.
  if (pdf) return void save(tab);
  const stem = baseName(tab.path).replace(/\.[^.]+$/, "");
  const target = await saveDialog({
    title: "Save as a repaired e-book",
    defaultPath: tab.path.replace(/[^\\/]+$/, `${stem} (repaired).epub`),
    filters: [{ name: "EPUB e-book", extensions: ["epub"] }],
  });
  if (!target) return;
  try {
    await api.saveRepaired(info.docId, target, false);
    showToast(`Saved ${baseName(target)}`, 4000);
  } catch (err) {
    showToast(`Couldn't save: ${err}`, 5000);
  }
}

/**
 * Saves with new protection, compression or a clean rewrite: to `target`, or in place when null.
 * The tab follows the saved file, as with Save as. Returns the saved file, or null (message shown).
 */
export async function saveWithOptions(tab: Tab, opts: SaveOptions, target: string | null): Promise<DocInfo | null> {
  if (!(await beforeSave(tab))) return null;
  const info = tab.info.value;
  const e = tab.edits.value;
  if (!info || !e || tab.saving.value) return null;
  tab.saving.value = true;
  try {
    const view = tab.viewer?.viewState();
    const saved = await api.saveWith(info.docId, e.order, e.rot, target, opts);
    await adoptSaved(tab, saved, view?.page);
    if (keptNotSaved(tab, saved)) return null;
    return saved;
  } catch (err) {
    failedSave(tab);
    showToast(`Couldn't save: ${err}`, 6000);
    return null;
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
