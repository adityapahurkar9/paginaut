// Markdown files: editing the source in the Markdown pane (right side), the pages following as you type, and
// saving back to the .md file. (MuPDF lays Markdown out; the pages are made again from the text.)
import { signal } from "@preact/signals";
import { api } from "./api";
import { adoptSaved, setSaveOverride } from "./pages";
import { openPane, pane, togglePane } from "./panes";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { baseName, openFiles, showToast, tabs, type Tab } from "./store";

export const isMarkdown = (path: string) => /\.(md|markdown)$/i.test(path);

/** The source being edited, per tab: the text now and as last saved, whether the file starts with a byte order
 *  mark (kept on saving), and for a file that isn't UTF-8 its encoding: such a file is read-only here. */
interface MdSource {
  text: string;
  saved: string;
  timer: number;
  showing: number;
  bom: boolean;
  encoding: string | null;
}
const sources = new Map<string, MdSource>();

/** A source as it travels with its tab to another window (windows.ts). */
export type MdCarried = Pick<MdSource, "text" | "saved" | "bom" | "encoding">;

/** The tab's source for a move to another window, if it has one. */
export function packMdSource(tab: Tab): MdCarried | null {
  const s = sources.get(tab.key);
  return s ? { text: s.text, saved: s.saved, bom: s.bom, encoding: s.encoding } : null;
}

/** Takes up the source of a tab that came from another window: what was typed there and not yet saved comes
 *  along (read from disk again it would be lost, with the tab still marked as changed). */
export function adoptMdSource(tab: Tab, c: MdCarried) {
  sources.set(tab.key, { ...c, timer: 0, showing: 0 });
  mdLoaded.value++;
  if (c.text !== c.saved) {
    tab.docChanged.value = true;
    // The pages may be from before the last letters typed.
    const s = sources.get(tab.key)!;
    s.timer = window.setTimeout(() => void refresh(tab), 450);
  }
}

export function mdSource(tab: Tab) {
  return sources.get(tab.key) ?? null;
}

/** Reads the tab's Markdown source (once); false if it couldn't be read. */
export async function loadMdSource(tab: Tab): Promise<boolean> {
  if (sources.has(tab.key)) return true;
  try {
    const f = await api.markdownRead(tab.path);
    if (!sources.has(tab.key)) sources.set(tab.key, { text: f.text, saved: f.text, timer: 0, showing: 0, bom: f.bom, encoding: f.encoding });
    mdLoaded.value++;
    return true;
  } catch (e) {
    showToast(String(e), 5000);
    return false;
  }
}
/** Bumped when a source is read, for the pane to show it. */
export const mdLoaded = signal(0);

export async function openMdPane(tab: Tab) {
  if (isMarkdown(tab.path) && (await loadMdSource(tab))) openPane("markdown");
}

export function toggleMdPane(tab: Tab) {
  if (pane.value === "markdown") togglePane("markdown");
  else void openMdPane(tab);
}

/** The text changed: the pages follow after a short pause. */
export function mdInput(tab: Tab, text: string) {
  const s = sources.get(tab.key);
  if (!s || s.encoding) return;
  s.text = text;
  tab.docChanged.value = text !== s.saved;
  clearTimeout(s.timer);
  s.timer = window.setTimeout(() => void refresh(tab), 450);
}

async function refresh(tab: Tab) {
  const s = sources.get(tab.key);
  const old = tab.info.value;
  if (!s || !old) return;
  const n = ++s.showing;
  try {
    const next = await api.markdownPreview(tab.path, s.text);
    // Typing on while this was made: a newer one is coming.
    if (n !== s.showing) return void api.close(next.docId);
    const page = tab.viewer?.viewState().page;
    await adoptSaved(tab, next, page);
    tab.docChanged.value = s.text !== s.saved;
    void api.close(old.docId);
  } catch (e) {
    showToast(`Couldn't show the Markdown: ${e}`, 4000);
  }
}

/** Saves the source to the .md file. */
export async function saveMarkdown(tab: Tab): Promise<boolean> {
  const s = sources.get(tab.key);
  if (!s || s.encoding) return true;
  try {
    await api.markdownSave(tab.path, s.text, s.bom);
    s.saved = s.text;
    tab.docChanged.value = false;
    showToast(`Saved ${tab.name}`);
    return true;
  } catch (e) {
    showToast(String(e), 5000);
    return false;
  }
}

/** A file that isn't UTF-8 is read-only here: this writes its text as a new UTF-8 file and opens that to edit. */
export async function saveUtf8Copy(tab: Tab) {
  const s = sources.get(tab.key);
  if (!s) return;
  const stem = baseName(tab.path).replace(/\.[^.]+$/, "");
  const target = await saveDialog({
    title: "Save a UTF-8 copy",
    defaultPath: tab.path.replace(/[^\\/]+$/, `${stem} (UTF-8).md`),
    filters: [{ name: "Markdown", extensions: ["md"] }],
  });
  if (!target) return;
  try {
    await api.markdownSave(target, s.text, false);
    void openFiles([target]);
  } catch (e) {
    showToast(String(e), 5000);
  }
}

// A closed tab's source goes with it.
tabs.subscribe((list) => {
  for (const [key, s] of sources) {
    if (!list.some((t) => t.key === key)) {
      clearTimeout(s.timer);
      sources.delete(key);
    }
  }
});

// Save (Ctrl+S, closing the tab) writes the .md file when its source was edited; Save as still makes a PDF.
setSaveOverride(async (tab, saveAs) => {
  const s = sources.get(tab.key);
  if (saveAs || !s || s.encoding || s.text === s.saved) return null;
  return saveMarkdown(tab);
});
