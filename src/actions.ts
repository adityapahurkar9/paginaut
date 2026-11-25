// User-facing actions, shared by the toolbar, keyboard shortcuts and the
// command palette.
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import * as S from "./search";
import { OFFICE_EXT } from "./office";
import { searchFlow } from "./reflow/find";
import { setViewMode, viewModeOf } from "./modes";
import { updateReflow, zoomText } from "./ui/Reflow";
import {
  activeKey, activeTab, closeHome, closeTab, cycleTab, isHome, findOpen, openFiles, paletteOpen, settings, showToast, updateSettings, type Tab,
} from "./store";
import { closeCompare, COMPARE } from "./compare";

export const OPENABLE = [
  "pdf", "xps", "oxps", "epub", "mobi", "fb2", "fbz", "cbz", "cbr", "cbt", "svg", "txt", "md", "markdown", "html", "htm", "xhtml",
  "png", "jpg", "jpeg", "jfif", "tif", "tiff", "bmp", "gif", "jp2", "jpx", "psd", "pnm", "pbm", "pgm", "ppm", "pam", "jxr",
  ...OFFICE_EXT,
];

export async function openDialog() {
  const res = await open({
    multiple: true,
    filters: [
      { name: "Documents and images", extensions: OPENABLE },
      { name: "PDF", extensions: ["pdf"] },
      { name: "Office documents", extensions: OFFICE_EXT },
      { name: "All files", extensions: ["*"] },
    ],
  });
  const files = Array.isArray(res) ? res : res ? [res] : [];
  if (files.length) await openFiles(files);
}

const viewer = () => activeTab.value?.viewer ?? null;

/** Read and Book modes show reflowed text: "zoom" there means text size. */
const reflowing = () => {
  const m = activeTab.value?.mode.value;
  return m === "read" || m === "book";
};
export const zoomIn = () => (reflowing() ? zoomText(1) : viewer()?.zoomBy(1.2));
export const zoomOut = () => (reflowing() ? zoomText(-1) : viewer()?.zoomBy(1 / 1.2));
export const zoomTo = (z: number) => (reflowing() ? zoomText(0) : viewer()?.setZoom(z));
export const textSize = (px: number) => updateReflow({ size: px });
export const fitWidth = () => viewer()?.setFit("width", true);
export const fitPage = () => viewer()?.setFit("page", true);
/** The width, up to a comfortable reading width: how documents open. */
export const fitRead = () => viewer()?.setFit("read", true);
export const goToPage = (p: number) => {
  const t = activeTab.value;
  if (t?.reflowView) t.reflowView.goToPage(p);
  else viewer()?.goToPage(p);
};
/** Goes to a page by its index in the file (outline, links). */
export function goToSource(src: number, yPt?: number) {
  const t = activeTab.value;
  if (!t) return;
  if (t.reflowView) {
    const v = t.edits.value?.order.indexOf(src) ?? src;
    if (v >= 0) t.reflowView.goToPage(v);
  } else viewer()?.goToSource(src, yPt);
}
/** Next/previous page, or spread in two-page layouts and Book mode. */
export const nextPage = () => {
  const t = activeTab.value;
  if (t?.reflowView) t.reflowView.next();
  else viewer()?.stepRow(1);
};
export const prevPage = () => {
  const t = activeTab.value;
  if (t?.reflowView) t.reflowView.prev();
  else viewer()?.stepRow(-1);
};

/** Read and Book toggle: a second press goes back to the page view. */
export function toggleMode(mode: "read" | "book") {
  const t = activeTab.value;
  if (!t || t.status.value !== "ready") return;
  const back = t.layout.value === "single" ? "single" : "double";
  void setViewMode(t, viewModeOf(t) === mode ? back : mode);
}

export const toggleSidebar = () => updateSettings({ sidebarOpen: !settings.value.sidebarOpen });
export const toggleNight = () => updateSettings({ night: !settings.value.night });
export function toggleTheme() {
  const dark = document.documentElement.dataset.theme === "dark";
  updateSettings({ theme: dark ? "light" : "dark" });
}

export function closeActive() {
  const t = activeTab.value;
  if (t) closeTab(t);
  else if (activeKey.value === COMPARE) closeCompare();
  else if (isHome(activeKey.value)) closeHome(activeKey.value!);
}
export const nextTab = () => cycleTab(1);
export const prevTab = () => cycleTab(-1);

export async function toggleFullscreen() {
  const w = getCurrentWindow();
  await w.setFullscreen(!(await w.isFullscreen()));
}

export async function copySelection() {
  const text = await viewer()?.copySelection();
  if (!text) return;
  await navigator.clipboard.writeText(text);
  showToast("Copied");
}

export function openFind() {
  findOpen.value = true;
}
export function openPalette() {
  paletteOpen.value = true;
}

// ---------------------------------------------------------------- search

// Read and Book modes search the reflowed text; the page view searches the pages.

let flowTimer = 0;
export function runSearch(tab: Tab, query: string) {
  const view = tab.reflowView;
  if (!view) return S.runSearch(tab, query, S.reveal, (e) => showToast(String(e)));
  clearTimeout(flowTimer);
  flowTimer = window.setTimeout(() => {
    const { hits, error, limited } = searchFlow(view.flow, query, S.searchOpts.value);
    // Start at the first match after where you're reading.
    const here = view.currentBlock();
    const first = hits.findIndex((h) => h.b >= here);
    const active = hits.length ? (first >= 0 ? first : 0) : -1;
    tab.flowSearch.value = query.trim() ? { query: query.trim(), hits, active, error, limited } : null;
    view.setHits(hits, hits[active] ?? null);
    if (active >= 0) view.showHit(hits[active]);
  }, 140);
}

export function stepSearch(dir: 1 | -1) {
  const tab = activeTab.value;
  if (!tab) return;
  const fs = tab.flowSearch.value, view = tab.reflowView;
  if (!view || !fs) return S.stepSearch(tab, dir);
  if (!fs.hits.length) return;
  const active = (fs.active + dir + fs.hits.length) % fs.hits.length;
  tab.flowSearch.value = { ...fs, active };
  view.setHits(fs.hits, fs.hits[active]);
  view.showHit(fs.hits[active]);
}

export function closeFind() {
  findOpen.value = false;
  const tab = activeTab.value;
  if (tab) {
    S.clearSearch(tab);
    tab.flowSearch.value = null;
    tab.reflowView?.setHits([], null);
  }
  (tab?.reflowView ?? tab?.viewer)?.focus();
}
