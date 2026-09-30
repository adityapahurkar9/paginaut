// App state: tabs, settings, recent files. Signals drive the Preact UI;
// the Viewer itself is imperative and lives on its tab.
import type { PageSizeSettings } from "./pagesize";
import type { HeaderSettings, WatermarkSettings } from "./stamps";
import { batch, computed, effect, signal, type Signal } from "@preact/signals";
import { api, type Attachment, type DocInfo, type OutlineItem } from "./api";
import type { FitMode, LayoutMode, ViewState, Viewer } from "./render/Viewer";
import { renderer } from "./render/RenderManager";
import type { SearchState } from "./search";
import { History, identity, isIdentity, type Edits } from "./edits";

// ---------------------------------------------------------------- persistence

function load<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v ? { ...fallback, ...JSON.parse(v) } : fallback;
  } catch {
    return fallback;
  }
}

function save(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* storage full or unavailable: settings just won't persist */
  }
}

export interface Settings {
  theme: "system" | "light" | "dark";
  night: boolean;
  sidebarOpen: boolean;
  sidebarWidth: number;
  sidebarTab: "pages" | "outline" | "comments" | "files";
  /** Smallest width of a page in the page grid, px. */
  gridSize: number;
  /** The Page size tool's last choices. */
  pageSize?: Partial<PageSizeSettings>;
  /** Page numbers / headers / footers and watermark dialogs' last choices. */
  headers?: Partial<HeaderSettings>;
  watermark?: Partial<WatermarkSettings>;
  /** Save rotations automatically (PDFs only, when nothing else changed). */
  autoSaveRotations: boolean;
  /** Which toolbar is showing (File is a menu, not a tab). */
  ribbon: "home" | "comment" | "sign" | "view" | "tools" | "help";
  /** Annotation colours per kind of tool, stroke width, text size. */
  annot: { colors: Record<"hl" | "pen" | "shape" | "text" | "note", string>; width: number; fontSize: number };
  /** Tint fillable form fields so they're easy to spot. */
  highlightFields: boolean;
  /** Double page: cover page on its own. */
  coverSeparate: boolean;
  /** Typography for Read and Book modes. */
  reflow: ReflowSettings;
  /** Digital signing choices, remembered (cert: thumbprint of the last one used). */
  dsc: { reason: string; location: string; showDate: boolean; lock: boolean; newFile: boolean; place: "box" | "invisible"; cert: string };
  /** Text recognition: the languages and model quality used last. */
  ocr: { langs: string[]; quality: "fast" | "best" };
  /** Which converter opens Office files ("auto": Microsoft Office, else LibreOffice, else the text preview). */
  officeEngine: "auto" | "msoffice" | "libreoffice" | "builtin";
  /** The side bar at the right edge: its panes' buttons in order, those taken off it, and whether it shows. */
  rail?: { order: string[]; hidden: string[]; visible: boolean };
  /** Compare's options, as last chosen. */
  compare?: Partial<import("./api").CompareOptions>;
}

export interface ReflowSettings {
  font: "sitka" | "georgia" | "segoe";
  /** Text size in px. */
  size: number;
  spacing: number;
  margins: "narrow" | "normal" | "wide";
  justify: boolean;
  paper: "auto" | "white" | "sepia" | "dark";
}

export const REFLOW_DEFAULTS: ReflowSettings = { font: "sitka", size: 19, spacing: 1.55, margins: "normal", justify: true, paper: "auto" };

export const settings = signal<Settings>(
  {
    theme: "system", night: false, sidebarOpen: true, sidebarWidth: 232, sidebarTab: "pages", gridSize: 190, autoSaveRotations: false,
    ribbon: "home", coverSeparate: false, highlightFields: true,
    annot: { colors: { hl: "#ffd400", pen: "#e8453c", shape: "#e8453c", text: "#1c1c1c", note: "#ffd400" }, width: 1.5, fontSize: 12 }, reflow: REFLOW_DEFAULTS,
    dsc: { reason: "I approve this document", location: "", showDate: true, lock: false, newFile: true, place: "box", cert: "" },
    ocr: { langs: ["eng"], quality: "fast" },
    officeEngine: "auto",
    ...load("paginaut.settings", {} as Partial<Settings>),
  },
);

export function updateSettings(patch: Partial<Settings>) {
  settings.value = { ...settings.value, ...patch };
  save("paginaut.settings", settings.value);
}

const systemDark = matchMedia("(prefers-color-scheme: dark)");
export const isDark = signal(false);
function applyTheme() {
  const t = settings.value.theme;
  const dark = t === "dark" || (t === "system" && systemDark.matches);
  isDark.value = dark;
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}
systemDark.addEventListener("change", applyTheme);
settings.subscribe(applyTheme);

export interface RecentFile {
  path: string;
  name: string;
  opened: number;
  view?: ViewState;
  pages?: number;
}

export const recent = signal<RecentFile[]>(load("paginaut.recent", { list: [] as RecentFile[] }).list);

function saveRecent() {
  save("paginaut.recent", { list: recent.value });
}

export function rememberFile(path: string, patch: Partial<RecentFile>) {
  const list = recent.value.filter((r) => r.path.toLowerCase() !== path.toLowerCase());
  const prev = recent.value.find((r) => r.path.toLowerCase() === path.toLowerCase());
  list.unshift({ path, name: baseName(path), opened: Date.now(), ...prev, ...patch });
  recent.value = list.slice(0, 40);
  saveRecent();
}

export function forgetFile(path: string) {
  recent.value = recent.value.filter((r) => r.path !== path);
  saveRecent();
}

export const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p;

// ---------------------------------------------------------------- tabs

export type TabStatus = "loading" | "password" | "ready" | "error";

export class Tab {
  readonly key = crypto.randomUUID();
  private _path: Signal<string>;
  status = signal<TabStatus>("loading");
  error = signal("");
  info = signal<DocInfo | null>(null);
  page = signal(0);
  pageCount = signal(0);
  zoom = signal(1);
  fit = signal<FitMode>("width");
  outline = signal<OutlineItem[] | null>(null);
  hasSelection = signal(false);
  search = signal<SearchState | null>(null);
  sizes = signal<[number, number][] | null>(null);
  /** The file's own page labels (by original page index), when it has any. */
  labels = signal<string[] | null>(null);
  /** Files embedded in the PDF (null until read). */
  attachments = signal<Attachment[] | null>(null);
  viewer: Viewer | null = null;
  passwordTries = 0;
  /** Unsaved page edits (order, rotation); null until the document is ready. */
  edits = signal<Edits | null>(null);
  history = new History();
  /** The page grid (big thumbnails for organizing) is showing instead of the pages. */
  grid = signal(false);
  /** Bumped whenever history changes, so undo/redo buttons refresh. */
  historyVersion = signal(0);
  /** Pages selected in the thumbnails (display indices). */
  pageSel = signal<Set<number>>(new Set());
  layout = signal<LayoutMode>("single");
  /** Pages as they are, or the text reflowed: Read (one column) or Book (two pages, full screen). */
  mode = signal<"pages" | "read" | "book">("pages");
  /** The open Read/Book view, if any. */
  reflowView: import("./reflow/ReadView").ReflowView | null = null;
  /** Where Read/Book mode was last (a block of the flow), to come back to the same place. */
  reflowBlock = -1;
  /** Find in Read/Book modes. */
  flowSearch = signal<{ query: string; hits: import("./reflow/find").FlowHit[]; active: number; error?: string; limited: boolean } | null>(null);
  /** Book mode page numbers. */
  bookInfo = signal<{ page: number; total: number; approx: boolean } | null>(null);
  saving = signal(false);
  /** Form fields or annotations changed in memory (not yet saved). */
  docChanged = signal(false);
  /** Form fields in the PDF (null until checked). */
  forms = signal<{ fields: number; xfa: boolean } | null>(null);
  /** Bumped when page content changes in memory (thumbnails redraw). */
  contentRev = signal(0);
  /** Bumped when the pages' text may have changed in memory (recognised text, undo): Read mode re-reads it. */
  textRev = signal(0);
  dirty = computed(() => {
    const e = this.edits.value;
    return (!!e && !isIdentity(e)) || this.docChanged.value;
  });

  constructor(path: string) {
    this._path = signal(path);
  }

  /** Changes after "Save as". */
  get path() {
    return this._path.value;
  }
  set path(p: string) {
    this._path.value = p;
  }

  get name() {
    return baseName(this.path);
  }
}

export const tabs = signal<Tab[]>([]);
export const activeKey = signal<string | null>(null);
/** The Home page as a tab of its own (the + button). */
export const HOME = "home";
export const homeOpen = signal(false);
export function openHome() {
  batch(() => {
    homeOpen.value = true;
    activeKey.value = HOME;
  });
}
export function closeHome() {
  batch(() => {
    homeOpen.value = false;
    if (activeKey.value === HOME) activeKey.value = tabs.value[tabs.value.length - 1]?.key ?? null;
  });
}
export const activeTab = computed(() => tabs.value.find((t) => t.key === activeKey.value) ?? null);

// ---------------------------------------------------------------- transient UI

export const paletteOpen = signal(false);
export const findOpen = signal(false);
export const toast = signal<{ text: string; id: number } | null>(null);

export interface AskButton {
  label: string;
  value: string;
  primary?: boolean;
}
export const dialog = signal<{ title: string; text: string; buttons: AskButton[]; resolve: (v: string) => void } | null>(null);

/** Shows a modal question; resolves with the chosen button's value ("cancel" on Esc). */
export function ask(title: string, text: string, buttons: AskButton[]): Promise<string> {
  return new Promise((resolve) => {
    dialog.value = {
      title,
      text,
      buttons,
      resolve: (v) => {
        dialog.value = null;
        resolve(v);
      },
    };
  });
}
let toastTimer = 0;
export function showToast(text: string, ms = 2600) {
  clearTimeout(toastTimer);
  toast.value = { text, id: Date.now() };
  toastTimer = window.setTimeout(() => (toast.value = null), ms);
}

// ---------------------------------------------------------------- document lifecycle

export async function openFiles(paths: string[]) {
  let last: Tab | null = null;
  for (const path of paths) {
    const existing = tabs.value.find((t) => t.path.toLowerCase() === path.toLowerCase());
    if (existing) {
      last = existing;
      continue;
    }
    const tab = new Tab(path);
    tabs.value = [...tabs.value, tab];
    last = tab;
    void loadTab(tab);
  }
  if (!last) return;
  // A file opened from the Home tab takes its place.
  batch(() => {
    if (activeKey.value === HOME) homeOpen.value = false;
    activeKey.value = last!.key;
  });
}

async function loadTab(tab: Tab) {
  try {
    const info = await api.open(tab.path);
    tab.info.value = info;
    if (info.needsPassword) {
      tab.status.value = "password";
      return;
    }
    ready(tab, info);
  } catch (e) {
    batch(() => {
      tab.error.value = String(e);
      tab.status.value = "error";
    });
  }
}

export async function unlockTab(tab: Tab, password: string): Promise<boolean> {
  const info = tab.info.value;
  if (!info) return false;
  const res = await api.unlock(info.docId, password).catch(() => null);
  tab.passwordTries++;
  if (!res) return false;
  ready(tab, res);
  return true;
}

function ready(tab: Tab, info: DocInfo) {
  batch(() => {
    tab.info.value = info;
    tab.pageCount.value = info.pageCount;
    tab.edits.value = identity(info.pageCount);
    tab.status.value = "ready";
  });
  rememberFile(tab.path, { pages: info.pageCount });
  api.pageSizes(info.docId).then((s) => (tab.sizes.value = s)).catch(() => {});
  api.pageLabels(info.docId).then((l) => (tab.labels.value = l)).catch(() => {});
  api.outline(info.docId).then((o) => (tab.outline.value = o)).catch(() => (tab.outline.value = []));
}

/** Closes a tab, asking about unsaved page edits first. Resolves false if cancelled. */
export async function closeTab(tab: Tab, opts: { save?: (tab: Tab) => Promise<boolean> } = {}): Promise<boolean> {
  if (tab.dirty.value) {
    activeKey.value = tab.key;
    const choice = await ask(`Save changes to ${tab.name}?`, "Your changes (filled-in fields, rotations, deletions, page order) haven't been saved.", [
      { label: "Save", value: "save", primary: true },
      { label: "Don't save", value: "discard" },
      { label: "Cancel", value: "cancel" },
    ]);
    if (choice === "cancel") return false;
    if (choice === "save" && !(await (opts.save ?? saveHook)(tab))) return false;
  }
  closeTabNow(tab);
  return true;
}

/** Set by pages.ts so closeTab can save without an import cycle. */
export let saveHook: (tab: Tab) => Promise<boolean> = async () => true;
export function setSaveHook(fn: (tab: Tab) => Promise<boolean>) {
  saveHook = fn;
}

function closeTabNow(tab: Tab) {
  const list = tabs.value;
  const i = list.indexOf(tab);
  if (i < 0) return;
  if (tab.viewer) {
    rememberFile(tab.path, { view: tab.viewer.viewState() });
    tab.viewer.destroy();
    tab.viewer = null;
  }
  const info = tab.info.value;
  if (info) {
    api.close(info.docId).catch(() => {});
    // Its pages and thumbnails can never be shown again (document ids aren't reused).
    renderer.forget(`${info.docId}.`);
  }
  const next = list.filter((t) => t !== tab);
  batch(() => {
    tabs.value = next;
    if (activeKey.value === tab.key) activeKey.value = next[Math.min(i, next.length - 1)]?.key ?? null;
  });
}

// WebView2 keeps what drawn pages used (mostly in its GPU process, often several
// hundred MB) until it collects garbage. Its "low memory" level makes it give that
// back within ~10 s, so it's on whenever nothing is being drawn: while the window is
// minimized and while no document is open.
const minimized = signal(false);
export const setMinimized = (m: boolean) => (minimized.value = m);
let lowMemory: boolean | undefined;
effect(() => {
  const low = minimized.value || tabs.value.length === 0;
  if (low === lowMemory) return;
  lowMemory = low;
  api.memoryLevel(low).catch(() => {});
});

export function cycleTab(dir: 1 | -1) {
  const list = tabs.value;
  if (list.length < 2) return;
  const i = list.findIndex((t) => t.key === activeKey.value);
  activeKey.value = list[(i + dir + list.length) % list.length].key;
}

/** Saves reading positions for every open tab (called periodically and on exit). */
export function persistPositions() {
  for (const t of tabs.value) if (t.viewer) rememberFile(t.path, { view: t.viewer.viewState() });
}
window.addEventListener("beforeunload", persistPositions);
setInterval(persistPositions, 15000);
