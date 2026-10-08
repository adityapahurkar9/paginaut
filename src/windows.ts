// Several windows: a tab moves to another window (or a new one) with its open document, unsaved changes, undo
// history and place. All windows share the one engine (appwindows.rs), so nothing is saved or opened again: this
// window lets go of the tab without closing its document, and the other takes it up.
import { batch } from "@preact/signals";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, type Attachment, type DocInfo, type OutlineItem } from "./api";
import { History, type Edits, type HistoryData } from "./edits";
import { ocrState } from "./ocr";
import { setViewMode } from "./modes";
import { redactTab } from "./redact";
import { textEdit } from "./textedit";
import { pageSizeEdit } from "./pagesize";
import { layoutEdit } from "./reflow/LayoutEditor";
import type { FitMode, LayoutMode, ViewState } from "./render/Viewer";
import { adoptTab, closeTabNow, openFiles, showToast, Tab } from "./store";

/** A tab on its way to another window: its own record of itself, as plain data. */
export interface TabTransfer {
  path: string;
  /** Absent: the document wasn't open yet (a password to give, or it couldn't be opened): the other window opens it. */
  doc?: {
    info: DocInfo;
    edits: Edits;
    history: HistoryData;
    docChanged: boolean;
    forms: { fields: number; xfa: boolean } | null;
    sizes: [number, number][] | null;
    labels: string[] | null;
    outline: OutlineItem[] | null;
    attachments: Attachment[] | null;
    mode: "pages" | "read";
    layout: LayoutMode;
    reflowBlock: number;
    page: number;
    zoom: number;
    fit: FitMode;
    view: ViewState | null;
  };
}

/** Why the tab can't move now, if it can't. */
function busy(tab: Tab): string | null {
  if (tab.status.value === "loading") return `${tab.name} is still opening.`;
  if (tab.saving.value) return `${tab.name} is being saved.`;
  if (ocrState(tab).value?.phase === "running") return `Text is being recognized in ${tab.name}: stop it or let it finish first.`;
  if (textEdit.value === tab) return "Finish editing the text first.";
  if (redactTab.value === tab) return "Close the Redact panel first.";
  if (pageSizeEdit.value?.tab === tab) return "Finish changing the page size first.";
  if (layoutEdit.value?.tab === tab) return "Finish correcting the layout first.";
  return null;
}

/** The tab as plain data (the document stays open), or null when it can't move now (says why). */
async function pack(tab: Tab): Promise<TabTransfer | null> {
  const why = busy(tab);
  if (why) {
    showToast(why, 4000);
    return null;
  }
  // Book mode is this window's (full screen): it ends here, and the tab goes on in Read mode.
  if (tab.mode.value === "book") await setViewMode(tab, "read");
  const info = tab.info.value;
  if (tab.status.value !== "ready" || !info) return { path: tab.path };
  return {
    path: tab.path,
    doc: {
      info,
      edits: tab.edits.value!,
      history: tab.history.toJSON(),
      docChanged: tab.docChanged.value,
      forms: tab.forms.value,
      sizes: tab.sizes.value,
      labels: tab.labels.value,
      outline: tab.outline.value,
      attachments: tab.attachments.value,
      mode: tab.mode.value === "pages" ? "pages" : "read",
      layout: tab.layout.value,
      reflowBlock: tab.reflowBlock,
      page: tab.page.value,
      zoom: tab.zoom.value,
      fit: tab.fit.value,
      view: tab.viewer?.viewState() ?? null,
    },
  };
}

/** Lets go of a packed tab: its document stays open for the window it goes to (one not yet open is closed). */
function release(tab: Tab, t: TabTransfer) {
  closeTabNow(tab, !!t.doc);
}

/** Takes up a tab moved from another window, at `index` in the strip (the end without one). */
export function attachTab(t: TabTransfer, index?: number) {
  const d = t.doc;
  if (!d) {
    void openFiles([t.path]);
    return;
  }
  const tab = new Tab(t.path);
  batch(() => {
    tab.info.value = d.info;
    tab.pageCount.value = d.info.pageCount;
    tab.edits.value = d.edits;
    tab.history = History.from(d.history);
    tab.docChanged.value = d.docChanged;
    tab.forms.value = d.forms;
    tab.sizes.value = d.sizes;
    tab.labels.value = d.labels;
    tab.outline.value = d.outline;
    tab.attachments.value = d.attachments;
    tab.mode.value = d.mode;
    tab.layout.value = d.layout;
    tab.reflowBlock = d.reflowBlock;
    tab.page.value = d.page;
    tab.zoom.value = d.zoom;
    tab.fit.value = d.fit;
    tab.carriedView = d.view;
    tab.status.value = "ready";
  });
  adoptTab(tab, index);
}

/** Where along this window's tab strip `x` (page pixels) falls: the index a dropped tab goes in at. */
export function stripIndexAt(x: number): number {
  let i = 0;
  for (const el of document.querySelectorAll<HTMLElement>(".titlebar .tab[data-tab]")) {
    const r = el.getBoundingClientRect();
    if (x > r.left + r.width / 2) i++;
  }
  return i;
}

/** Moves a tab into a new window: where the mouse is (`atCursor`, a tab dropped there) or beside this one. */
export async function moveToNewWindow(tab: Tab, atCursor = false) {
  const t = await pack(tab);
  if (!t) return;
  release(tab, t);
  try {
    await api.windowNew(t, atCursor);
  } catch (e) {
    // No window: the tab stays here.
    attachTab(t);
    showToast(`Couldn't open a new window: ${e}`, 5000);
  }
}

/** Tabs in this window's strip (documents, the comparison, Home tabs). */
export const stripCount = () => document.querySelectorAll(".titlebar .tab").length;

/** Moves a tab into another window (`atX`: where along its strip it was dropped). A window left with nothing
 * (its only tab moved) closes. */
export async function moveToWindow(tab: Tab, label: string, atX: number | null = null) {
  // That window has the file open already: it comes forward at its copy (two copies would overwrite each other).
  if ((await api.windowWith(tab.path).catch(() => null)) === label) {
    showToast(`That window already has ${tab.name} open.`, 4000);
    void api.tabFocus(label, tab.path).catch(() => {});
    return;
  }
  const alone = stripCount() === 1;
  const t = await pack(tab);
  if (!t) return;
  release(tab, t);
  try {
    await api.tabSend(label, t, atX);
  } catch (e) {
    attachTab(t);
    showToast(String(e), 5000);
    return;
  }
  if (alone) void getCurrentWindow().destroy();
}
