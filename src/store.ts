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

/** Windows' file names ignore case; Linux's and Android's don't (Report.pdf and report.pdf are two files there). */
const CASELESS_PATHS = /Windows/.test(navigator.userAgent);
/** A path as it is compared, and as it keys what's remembered per file. */
export const pathKey = (p: string) => (CASELESS_PATHS ? p.toLowerCase() : p);
/** The two paths name the same file. */
export const samePath = (a: string, b: string) => pathKey(a) === pathKey(b);

export function load<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v ? { ...fallback, ...JSON.parse(v) } : fallback;
  } catch {
    return fallback;
  }
}

export function save(key: string, value: unknown) {
  storeItem(key, JSON.stringify(value));
}

// Everything kept under "paginaut." is mirrored in storage.json in the app's data folder (prefs.rs), which a
// reinstall can't take: the WebView once started its storage over, and settings, recent files and the shelf went
// with it. Writes go through storeItem, which stamps the storage and writes the file soon after. index.html puts
// the file back when it's newer than the stamp.
const STAMP = "paginaut.saved";

/** Stores a raw text (null removes it) and has it mirrored. */
export function storeItem(key: string, text: string | null) {
  try {
    if (text === null) localStorage.removeItem(key);
    else localStorage.setItem(key, text);
    localStorage.setItem(STAMP, String(Date.now()));
  } catch {
    /* storage full or unavailable: not persisted */
    return;
  }
  // Drags and scrolls store many times a second: the file takes the last.
  window.clearTimeout(mirrorTimer);
  mirrorTimer = window.setTimeout(writeMirror, 500);
}

let mirrorTimer = 0;
function writeMirror() {
  window.clearTimeout(mirrorTimer);
  mirrorTimer = 0;
  try {
    const items: Record<string, string> = {};
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k?.startsWith("paginaut.") && k !== STAMP) items[k] = localStorage.getItem(k) ?? "";
    }
    api.storageWrite(JSON.stringify({ saved: Date.now(), items })).catch(() => {
      /* the storage still has it */
    });
  } catch {
    /* no storage to mirror */
  }
}
// A change in the last moment before the window closes still reaches the file.
window.addEventListener("pagehide", () => mirrorTimer && writeMirror());

// A first start (from 0.9.6 or before), or a storage newer than the file (a reload since): write the file.
try {
  localStorage.removeItem("paginaut.settings.saved"); // 0.9.6's stamp, for settings alone
  const fileSaved = (window as { __paginautStorage?: { saved: number } }).__paginautStorage?.saved ?? 0;
  const stamp = Number(localStorage.getItem(STAMP)) || 0;
  if (!stamp) localStorage.setItem(STAMP, String(Date.now()));
  if (!stamp || stamp > fileSaved) writeMirror();
} catch {
  /* no storage */
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
  dsc: {
    reason: string; location: string; showDate: boolean; lock: boolean; newFile: boolean; place: "box" | "invisible"; cert: string;
    /** Add a trusted timestamp (on unless turned off), from this service (RFC 3161). */
    timestamp?: boolean; tsa?: string;
    /** Linux: a DSC token's PKCS#11 driver Paginaut doesn't find by itself. */
    pkcs11?: string;
  };
  /** Text recognition: the languages and model quality used last. */
  ocr: { langs: string[]; quality: "fast" | "best" };
  /** Which converter opens Office files ("auto": Microsoft Office, else LibreOffice, else the text preview). */
  officeEngine: "auto" | "msoffice" | "libreoffice" | "builtin";
  /** The side bar at the right edge: its panes' buttons in order, those taken off it, and whether it shows. */
  rail?: { order: string[]; hidden: string[]; visible: boolean };
  /** The theme (a preset's id or one of the user's own). `theme` is its light or dark mode. */
  look?: string;
  customThemes?: import("./themes").Theme[];
  skin?: import("./skin").Skin;
  /** Update checks: automatically once a day (on unless turned off), and when the last one was. */
  updates?: { auto: boolean; last?: number };
  /** Read aloud: the Windows voice (its id; empty for Windows' default) and speed (1 = normal). */
  speech?: { voice: string; rate: number };
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

const SETTINGS_KEY = "paginaut.settings";

export const settings = signal<Settings>(
  {
    theme: "system", night: false, sidebarOpen: true, sidebarWidth: 232, sidebarTab: "pages", gridSize: 190, autoSaveRotations: false,
    ribbon: "home", coverSeparate: false, highlightFields: true,
    annot: { colors: { hl: "#ffd400", pen: "#e8453c", shape: "#e8453c", text: "#1c1c1c", note: "#ffd400" }, width: 1.5, fontSize: 12 }, reflow: REFLOW_DEFAULTS,
    dsc: { reason: "I approve this document", location: "", showDate: true, lock: false, newFile: true, place: "box", cert: "" },
    ocr: { langs: ["eng"], quality: "fast" },
    officeEngine: "auto",
    ...load(SETTINGS_KEY, {} as Partial<Settings>),
  },
);

export function updateSettings(patch: Partial<Settings>) {
  settings.value = { ...settings.value, ...patch };
  save(SETTINGS_KEY, settings.value);
}

/** The interface is dark now (set by themes.ts, which applies the theme). */
export const isDark = signal(false);

export interface RecentFile {
  path: string;
  name: string;
  opened: number;
  view?: ViewState;
  pages?: number;
}

export const recent = signal<RecentFile[]>(load("paginaut.recent", { list: [] as RecentFile[] }).list);

// Every window keeps these in the same storage: what one window writes, the others take up (the theme follows at
// once, recent files stay one list). How a window is laid out (side bar, ribbon tab) stays its own.
const OWN_LAYOUT = ["sidebarOpen", "sidebarWidth", "sidebarTab", "ribbon"] as const;
window.addEventListener("storage", (e) => {
  if (!e.newValue) return;
  try {
    if (e.key === SETTINGS_KEY) {
      const theirs = JSON.parse(e.newValue) as Settings;
      const mine = settings.value;
      const keep = Object.fromEntries(OWN_LAYOUT.map((k) => [k, mine[k]]));
      settings.value = { ...mine, ...theirs, ...keep };
    } else if (e.key === "paginaut.recent") recent.value = (JSON.parse(e.newValue) as { list: RecentFile[] }).list ?? recent.value;
  } catch {
    /* half-written: the next change brings it */
  }
});

function saveRecent() {
  save("paginaut.recent", { list: recent.value });
}

export function rememberFile(path: string, patch: Partial<RecentFile>) {
  const list = recent.value.filter((r) => !samePath(r.path, path));
  const prev = recent.value.find((r) => samePath(r.path, path));
  list.unshift({ path, name: baseName(path), opened: Date.now(), ...prev, ...patch });
  recent.value = list.slice(0, 40);
  saveRecent();
}

export function forgetFile(path: string) {
  // Compared as rememberFile compares, or a file kept under another spelling of its path stayed.
  recent.value = recent.value.filter((r) => !samePath(r.path, path));
  saveRecent();
}

export const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p;

/** Running on Linux (the desktop app's other system): words and choices that name Windows change with it. */
export const onLinux = /Linux/.test(navigator.userAgent) && !/Android/.test(navigator.userAgent);
/** "Match Windows" / "Match system": following the system's light or dark setting. */
export const MATCH_SYSTEM = onLinux ? "Match system" : "Match Windows";

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
  /** Taken out of the window with its document closed: one still opening is closed when it arrives. */
  closed = false;
  /** Where to open the pages instead of the reading position kept for the file (a tab moved from another window). */
  carriedView: ViewState | null = null;
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
/**
 * Home pages as tabs of their own, like a browser's new tabs: the app starts with one, + adds
 * another, a file opened from one takes its place, and closing the last tab leaves a fresh one.
 * Shown after the documents in the tab strip.
 */
export const homes = signal<string[]>([]);
let homeSeq = 0;
export const isHome = (key: string | null) => !!key && key.startsWith("home:");
export function openHome() {
  const key = `home:${++homeSeq}`;
  batch(() => {
    homes.value = [...homes.value, key];
    activeKey.value = key;
  });
}
export function closeHome(key: string) {
  closeKey(key, () => (homes.value = homes.value.filter((k) => k !== key)));
}
/** The Home tab a file is being opened from gives way to it. */
export function leaveHome() {
  const key = activeKey.value;
  if (isHome(key)) homes.value = homes.value.filter((k) => k !== key);
}

/** Set by compare.ts (no import cycle): the comparison's key while it has a tab. */
let compareKey: () => string | null = () => null;
export function setCompareKey(fn: () => string | null) {
  compareKey = fn;
}
/** Every tab, in the strip's order. */
function stripKeys() {
  const c = compareKey();
  return [...tabs.value.map((t) => t.key), ...(c ? [c] : []), ...homes.value];
}
/**
 * Removes a tab (`drop` takes it out of its list): the one next to it becomes current if it was,
 * and with no tab left, a new Home tab opens.
 */
export function closeKey(key: string, drop: () => void) {
  const before = stripKeys();
  const i = before.indexOf(key);
  batch(() => {
    drop();
    const after = stripKeys();
    if (!after.length) {
      const k = `home:${++homeSeq}`;
      homes.value = [k];
      activeKey.value = k;
    } else if (activeKey.value === key || !after.includes(activeKey.value ?? "")) activeKey.value = after[Math.min(Math.max(0, i), after.length - 1)];
  });
}
openHome();
export const activeTab = computed(() => tabs.value.find((t) => t.key === activeKey.value) ?? null);

// ---------------------------------------------------------------- transient UI

export const paletteOpen = signal(false);
/** The Settings dialog, at one of its tabs. */
export type SettingsTab = "general" | "defaults" | "themes" | "sidebar" | "signing" | "ocr" | "speech" | "office" | "compare" | "shortcuts" | "about";
export const settingsOpen = signal<SettingsTab | null>(null);
export function openSettings(tab: SettingsTab = "general") {
  settingsOpen.value = tab;
}
export const findOpen = signal(false);
export const toast = signal<{ text: string; id: number } | null>(null);

export interface AskButton {
  label: string;
  value: string;
  primary?: boolean;
}
export const dialog = signal<{ title: string; text: string; buttons: AskButton[]; secret?: string; resolve: (v: string) => void } | null>(null);

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
/** Asks for something not to be shown as it's typed (a token's PIN): the text, or null when cancelled. */
export function askSecret(title: string, text: string, label: string, ok: string): Promise<string | null> {
  return new Promise((resolve) => {
    dialog.value = {
      title,
      text,
      secret: label,
      buttons: [
        { label: "Cancel", value: "cancel" },
        { label: ok, value: "ok", primary: true },
      ],
      resolve: (v) => {
        dialog.value = null;
        resolve(v === "cancel" ? null : v.replace(/^ok:/, ""));
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

/** `launch`: the files Paginaut was started with, in its first window (no other window to have them open). */
export async function openFiles(paths: string[], launch = false) {
  let last: Tab | null = null;
  for (const path of paths) {
    const existing = tabs.value.find((t) => samePath(t.path, path));
    if (existing) {
      last = existing;
      continue;
    }
    // Open in another window already: that window comes forward at it (two copies would overwrite each other's
    // saves).
    const owner = launch ? null : await api.windowWith(path).catch(() => null);
    if (owner) {
      void api.tabFocus(owner, path).catch(() => {});
      continue;
    }
    const tab = new Tab(path);
    tabs.value = [...tabs.value, tab];
    last = tab;
    void loadTab(tab);
  }
  if (!last) return;
  // A file opened from a Home tab takes its place.
  batch(() => {
    leaveHome();
    activeKey.value = last!.key;
  });
}

async function loadTab(tab: Tab) {
  try {
    const info = await api.open(tab.path);
    // Closed while it was opening (an Office file converting, a big scan): the engine lets go of it now, or the
    // document stayed open (and on Windows the file locked) until Paginaut quit.
    if (tab.closed) {
      api.close(info.docId).catch(() => {});
      return;
    }
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

/** False when the password is wrong; throws when the file couldn't be read with it. */
export async function unlockTab(tab: Tab, password: string): Promise<boolean> {
  const info = tab.info.value;
  if (!info) return false;
  const res = await api.unlock(info.docId, password);
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
  // Said once on opening too: the bar offering to save it isn't shown in Read and Book mode.
  if (info.repaired) showToast("This e-book is a PDF converted page by page: shown with its pages as made.", 6000);
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

/**
 * Takes a tab out of the window. Its document closes, unless `keepDoc` (the tab moves to another window, which
 * carries on with the same open document).
 */
export function closeTabNow(tab: Tab, keepDoc = false) {
  const list = tabs.value;
  const i = list.indexOf(tab);
  if (i < 0) return;
  if (tab.viewer) {
    rememberFile(tab.path, { view: tab.viewer.viewState() });
    tab.viewer.destroy();
    tab.viewer = null;
  }
  const info = tab.info.value;
  if (!keepDoc) tab.closed = true;
  if (info) {
    if (!keepDoc) api.close(info.docId).catch(() => {});
    // Its pages and thumbnails can never be shown here again (document ids aren't reused; another window has
    // its own picture cache).
    renderer.forget(`${info.docId}.`);
  }
  closeKey(tab.key, () => (tabs.value = list.filter((t) => t !== tab)));
}

/** Puts a tab (one moved from another window) into the strip at `index` (the end without one), current. */
export function adoptTab(tab: Tab, index = tabs.value.length) {
  batch(() => {
    const list = tabs.value.slice();
    list.splice(Math.max(0, Math.min(index, list.length)), 0, tab);
    tabs.value = list;
    leaveHome();
    activeKey.value = tab.key;
  });
}

/** Moves a document tab to `index` in the strip (dragged along it). True when its place changed. */
export function moveTab(tab: Tab, index: number): boolean {
  const list = tabs.value.filter((t) => t !== tab);
  if (list.length === tabs.value.length) return false;
  list.splice(Math.max(0, Math.min(index, list.length)), 0, tab);
  if (!list.some((t, i) => t !== tabs.value[i])) return false;
  tabs.value = list;
  return true;
}

// WebView2 keeps what drawn pages used (mostly in its GPU process, often several
// hundred MB) until it collects garbage. Its "low memory" level makes it give that
// back within ~10 s, so it's on whenever nothing is being drawn: while the window is
// minimized and while no document is open.
/** The window is minimized (work nobody can see can wait). */
export const minimized = signal(false);
export const setMinimized = (m: boolean) => (minimized.value = m);
let lowMemory: boolean | undefined;
effect(() => {
  const low = minimized.value || tabs.value.length === 0;
  if (low === lowMemory) return;
  lowMemory = low;
  api.memoryLevel(low).catch(() => {});
});

/** Tabs by when they were last current, most recent first (the Alt+` switcher's order). */
export const recentKeys = signal<string[]>([]);
effect(() => {
  const key = activeKey.value;
  const all = new Set(stripKeys());
  if (key) recentKeys.value = [key, ...recentKeys.peek().filter((k) => k !== key && all.has(k))];
});
/** Every tab in the strip, most recently used first (ones never visited after the rest, in strip order). */
export function tabsByUse(): string[] {
  const all = stripKeys();
  const used = recentKeys.value.filter((k) => all.includes(k));
  return [...used, ...all.filter((k) => !used.includes(k))];
}

export function cycleTab(dir: 1 | -1) {
  const list = stripKeys();
  if (list.length < 2) return;
  const i = list.indexOf(activeKey.value ?? "");
  activeKey.value = list[(i + dir + list.length) % list.length];
}

/** The reading position last saved, by file: an unchanged one isn't saved again. */
const savedViews = new Map<string, string>();

/** Saves reading positions for every open tab that moved (called periodically and on exit). */
export function persistPositions() {
  for (const t of tabs.value) {
    if (!t.viewer) continue;
    const view = t.viewer.viewState();
    const json = JSON.stringify(view);
    if (savedViews.get(t.path) === json) continue;
    savedViews.set(t.path, json);
    rememberFile(t.path, { view });
  }
}
window.addEventListener("beforeunload", persistPositions);
setInterval(persistPositions, 15000);
