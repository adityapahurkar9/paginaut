import { render } from "preact";
import { effect } from "@preact/signals";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import * as A from "./actions";
import { api } from "./api";
import { activeKey, activeTab, closeTab, dialog, findOpen, openFiles, openHome, paletteOpen, persistPositions, setMinimized, tabs, openSettings, settingsOpen } from "./store";
import * as P from "./pages";
import * as C from "./compare";
import "./themes";
import "./skin";
import "./update";
import { renderer } from "./render/RenderManager";
import { App, dropping, tool } from "./ui/App";
import { bookScreen, exitBook } from "./modes";
import { layoutEdit } from "./reflow/LayoutEditor";
import { annotTool, deleteSelected, selectedAnnot, setAuthor } from "./annots";
import { endLayoutEdit } from "./ui/LayoutPanel";
import { creating, droppedPicture } from "./sign";
import { fileMenuOpen } from "./ui/Toolbar";
import { toggleGrid } from "./ui/PageGrid";
import { startProperties } from "./ui/MetaUI";
import "./extract";
import { endPageSize, pageSizeEdit } from "./pagesize";
import { initOffice } from "./office";
import { pauseOrResume, speaking, toggleReadAloud } from "./speech";
import { printTab } from "./print";
import { attachTab, stripIndexAt, type TabTransfer } from "./windows";
import { switcherKey } from "./ui/TabSwitcher";

// Before any file opens: which program converts Office files.
initOffice();

render(<App />, document.getElementById("app")!);

// Comments are signed with the Windows user name.
api.userName().then(setAuthor).catch(() => {});

// Diagnostics hook for tools/cdp.mjs.
(window as unknown as { paginaut: unknown }).paginaut = { openFiles, tabs, activeTab, renderer, actions: A, pages: P, compare: C };

// Show the window once the first frame is painted (no white flash). WebKitGTK (Linux) paints no frames for a
// window that isn't shown yet, so a moment later it's shown regardless.
const showWindow = () => void getCurrentWindow().show();
requestAnimationFrame(() => requestAnimationFrame(showWindow));
setTimeout(showWindow, 300);

// Minimizing sends a resize too: give memory back while minimized.
getCurrentWindow().onResized(() => {
  getCurrentWindow().isMinimized().then(setMinimized).catch(() => {});
});

// Files passed on the command line or "Open with", or a tab moved here from another window; later, files from
// a second launch. (Events for this window alone: the global listen would hear every window's.)
// Listening first: files that arrive before then come with the start instead (appwindows.rs forward_files).
const thisWindow = getCurrentWebviewWindow();
void Promise.all([
  thisWindow.listen<string[]>("open-files", (e) => openFiles(e.payload)),
  thisWindow.listen<{ tab: TabTransfer; atX: number | null }>("tab-arrive", (e) => attachTab(e.payload.tab, e.payload.atX == null ? undefined : stripIndexAt(e.payload.atX))),
  // A file asked for in another window, open here: its tab comes forward.
  thisWindow.listen<string>("focus-path", (e) => {
    const t = tabs.value.find((x) => x.path.toLowerCase() === e.payload.toLowerCase());
    if (t) activeKey.value = t.key;
  }),
]).then(async () => {
  const s = await api.windowStart();
  if (s.files.length) void openFiles(s.files);
  if (s.tab) attachTab(s.tab as TabTransfer);
});

// The window's title names its document (for the taskbar and Windows' Alt+Tab); unsaved changes are reported too,
// so an update waits for them in every window.
effect(() => {
  const name = activeTab.value?.name ?? "";
  const n = tabs.value.length;
  // How other windows' "Move tab to window" names this one.
  const summary = !n ? "Home" : n === 1 ? tabs.value[0].name : `${name || tabs.value[0].name} and ${n - 1} more`;
  void api.windowReport(name, summary, tabs.value.map((t) => t.path), tabs.value.some((x) => x.dirty.value)).catch(() => {});
});

// Drag and drop from Explorer.
let dragPaths: string[] = [];
const PICTURE = /\.(png|jpe?g|jfif|bmp|gif|webp)$/i;
getCurrentWebview().onDragDropEvent((e) => {
  const t = e.payload.type;
  // While making a signature, a dropped picture goes to it (anything else opens as usual).
  if (t === "enter") dragPaths = e.payload.paths;
  if (creating.value && PICTURE.test(dragPaths[0] ?? "")) {
    if (t === "drop") droppedPicture.value = dragPaths[0];
    return;
  }
  if (t === "enter" || t === "over") dropping.value = true;
  else if (t === "leave") dropping.value = false;
  else if (t === "drop") {
    dropping.value = false;
    if (e.payload.paths.length) openFiles(e.payload.paths);
  }
});

// Closing the window: ask about unsaved page edits, tab by tab. Its documents close with it (the other
// windows carry on with theirs).
getCurrentWindow().onCloseRequested(async (event) => {
  persistPositions();
  event.preventDefault();
  for (const t of tabs.value.filter((t) => t.dirty.value)) if (!(await closeTab(t))) return;
  if (C.compare.value) C.closeCompare();
  // The other windows carry on: this one's documents close (the last window's go with the app).
  if ((await api.windowList().catch(() => [])).length) await Promise.allSettled(tabs.value.map((t) => t.info.value && api.close(t.info.value.docId)));
  await getCurrentWindow().destroy();
});

// ---------------------------------------------------------------- keyboard

const isTyping = (e: KeyboardEvent) => {
  const el = e.target as HTMLElement;
  return el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable;
};

window.addEventListener("keydown", (e) => {
  const ctrl = e.ctrlKey || e.metaKey;
  const k = e.key.toLowerCase();
  const typing = isTyping(e);
  const handled = () => {
    e.preventDefault();
    e.stopPropagation();
  };

  if (dialog.value || settingsOpen.value) return; // a dialog is open; it handles its own keys
  // Alt+` / Alt+~: the tab switcher (it takes the keys while it's open).
  if (switcherKey(e)) return handled();
  const tab = activeTab.value;
  const ready = tab?.status.value === "ready" ? tab : null;

  if (ctrl && !e.shiftKey && !e.altKey) {
    switch (k) {
      case "s": handled(); if (ready) void P.save(ready); return;
      case "z": if (!typing && ready) { handled(); P.undo(ready); } return;
      case "y": if (!typing && ready) { handled(); P.redo(ready); } return;
      case "o": handled(); return A.openDialog();
      case "w": handled(); return A.closeActive();
      case "t": handled(); return openHome();
      case "n": handled(); return void api.windowNew(null, false).catch(() => {});
      case "f": handled(); return A.openFind();
      case "k": handled(); paletteOpen.value = true; return;
      case "p": handled(); if (ready) void printTab(ready); return;
      case ",": handled(); return openSettings();
      case "g": handled(); return (document.getElementById("page-input") as HTMLInputElement | null)?.focus();
      case "=": case "+": handled(); return A.zoomIn();
      case "-": handled(); return A.zoomOut();
      case "0": handled(); return A.zoomTo(1);
      case "1": handled(); return A.fitPage();
      case "2": handled(); return A.fitWidth();
      case "tab": handled(); return A.nextTab();
      case "h": if (ready) { handled(); A.toggleMode("read"); } return;
      case "d": if (ready) { handled(); startProperties(ready); } return;
      case "c": if (!typing && activeTab.value?.hasSelection.value) { handled(); return A.copySelection(); } return;
      case "a":
        if (!typing && ready?.grid.value) { handled(); ready.pageSel.value = new Set(ready.edits.value?.order.map((_, i) => i)); return; }
        if (!typing && activeTab.value?.viewer) { handled(); return activeTab.value.viewer.selectAllOnPage(); }
        return;
    }
  }
  if (ctrl && e.shiftKey) {
    if (k === "tab") { handled(); return A.prevTab(); }
    if (k === "s") { handled(); if (ready) void P.save(ready, true); return; }
    if (k === "z" && !typing && ready) { handled(); return P.redo(ready); }
    if (k === "n") { handled(); return A.toggleNight(); }
    if (k === "d") { handled(); return A.toggleTheme(); }
    if (k === "g" && ready) { handled(); return toggleGrid(ready); }
    if (k === "u" && (ready || speaking.value)) { handled(); return toggleReadAloud(ready); }
    if (e.key === " " && speaking.value) { handled(); return pauseOrResume(); }
  }
  if (e.altKey && !ctrl && k === "f") { handled(); fileMenuOpen.value = true; return; }
  if (e.key === "F1") { handled(); openSettings("shortcuts"); return; }
  if (e.key === "F5") { handled(); A.toggleMode("book"); return; }
  if (e.key === "F4") { handled(); return A.toggleSidebar(); }
  if (e.key === "F11") { handled(); return A.toggleFullscreen(); }
  if (e.key === "F3") { handled(); return findOpen.value ? A.stepSearch(e.shiftKey ? -1 : 1) : A.openFind(); }
  if (e.key === "Escape" && !typing) {
    if (findOpen.value) return A.closeFind();
    if (bookScreen.value) return exitBook();
    if (layoutEdit.value) return endLayoutEdit();
    if (pageSizeEdit.value) return endPageSize();
    if (ready?.grid.value) return toggleGrid(ready, false);
    if (annotTool.value || selectedAnnot.value) {
      annotTool.value = null;
      selectedAnnot.value = null;
      return;
    }
    activeTab.value?.viewer?.clearSelection();
    if (ready) ready.pageSel.value = new Set();
    return;
  }
  if (typing || ctrl || e.altKey) return;
  if (ready && k === "r") { handled(); return P.rotatePages(ready, e.shiftKey ? -90 : 90); }
  if (ready && e.key === "Delete") {
    handled();
    if (selectedAnnot.value?.tab === ready) return void deleteSelected();
    return P.deletePages(ready);
  }
  // Book mode: arrows, Space and Page Up/Down turn the page. Elsewhere Left/Right step a page
  // (or spread), and a screenful in Read mode where one paragraph can span several pages.
  if (bookScreen.value && ready) {
    const fwd = ["ArrowRight", "ArrowDown", "PageDown", " "].includes(e.key) && !(e.key === " " && e.shiftKey);
    const back = ["ArrowLeft", "ArrowUp", "PageUp"].includes(e.key) || (e.key === " " && e.shiftKey);
    if (fwd || back) { handled(); return fwd ? A.nextPage() : A.prevPage(); }
  }
  if (k === "h") tool.value = "hand";
  else if (k === "v") tool.value = "select";
  else if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && ready) { handled(); (e.key === "ArrowRight" ? A.nextPage : A.prevPage)(); }
});

// Disable the browser's own zoom and context menu in the app window (fields keep theirs: cut, copy,
// paste). Pages and Read/Book text have the app's own (extract.ts).
window.addEventListener("wheel", (e) => e.ctrlKey && e.preventDefault(), { passive: false });
window.addEventListener("contextmenu", (e) => {
  if (!isTyping(e as unknown as KeyboardEvent)) e.preventDefault();
});
