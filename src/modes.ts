// The four ways to view a document:
//   Single / Double  the pages as they are (Double: two side by side)
//   Read             the text reflowed into one comfortable column
//   Book             the text reflowed into a two-page book, full screen, with page turns
import { computed, effect } from "@preact/signals";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { activeKey, activeTab, findOpen, settings, tabs, updateSettings, type Tab } from "./store";

export type ViewMode = "single" | "double" | "read" | "book";

export function viewModeOf(tab: Tab): ViewMode {
  const m = tab.mode.value;
  if (m !== "pages") return m;
  return tab.layout.value === "single" ? "single" : "double";
}

/** True while the active tab is in Book mode: bars and side panel hidden, full screen. */
export const bookScreen = computed(() => activeTab.value?.mode.value === "book");

/** Mode to go back to when leaving Book mode, and whether the window was full screen before. */
let beforeBook: { tab: Tab; mode: ViewMode; fullscreen: boolean } | null = null;

export async function setViewMode(tab: Tab, mode: ViewMode) {
  const cur = viewModeOf(tab);
  if (cur === mode) return;
  const win = getCurrentWindow();
  if (mode === "book") {
    beforeBook = { tab, mode: cur, fullscreen: await win.isFullscreen() };
    findOpen.value = false;
  }
  if (mode === "single" || mode === "double") {
    tab.layout.value = mode === "single" ? "single" : settings.value.coverSeparate ? "spread-cover" : "spread";
    tab.mode.value = "pages";
  } else tab.mode.value = mode;
  if (mode === "book") {
    if (!beforeBook?.fullscreen) await win.setFullscreen(true);
  } else if (cur === "book") {
    const b = beforeBook;
    beforeBook = null;
    if (b && !b.fullscreen) await win.setFullscreen(false);
  }
}

/** Leaves Book mode, back to the mode it was entered from. */
export function exitBook() {
  const tab = activeTab.value;
  if (!tab || tab.mode.value !== "book") return;
  const back = beforeBook?.tab === tab ? beforeBook.mode : "read";
  void setViewMode(tab, back === "book" ? "read" : back);
}

export function setCoverSeparate(on: boolean) {
  updateSettings({ coverSeparate: on });
  for (const t of tabs.value) if (t.mode.value === "pages" && t.layout.value !== "single") t.layout.value = on ? "spread-cover" : "spread";
}

// Switching away from a tab in Book mode takes it out of Book mode.
effect(() => {
  const key = activeKey.value;
  const b = beforeBook;
  if (b && b.tab.key !== key && b.tab.mode.value === "book") void setViewMode(b.tab, b.mode === "book" ? "read" : b.mode);
});
