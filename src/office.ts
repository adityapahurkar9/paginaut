// Office files: which converters are here (Microsoft Office, LibreOffice, the built-in text preview), the
// preferred one, setting up Paginaut's own LibreOffice, converting a file again with another converter, and
// converting many files to PDFs.
import { signal } from "@preact/signals";
import { api, type InstallEvent, type OfficeEngine, type OfficeEngines } from "./api";
import { adoptSaved } from "./pages";
import { ask, baseName, settings, showToast, updateSettings, type Tab } from "./store";

/** Office files Paginaut opens (through a PDF made from them). */
export const OFFICE_EXT = [
  "docx", "doc", "docm", "dotx", "dot", "dotm", "rtf", "odt", "ott",
  "xlsx", "xls", "xlsm", "xlsb", "xltx", "xlt", "ods", "ots",
  "pptx", "ppt", "pptm", "ppsx", "pps", "potx", "pot", "odp", "otp",
];
/** What MuPDF can show itself (text only). */
const BUILTIN_EXT = ["docx", "xlsx", "pptx"];

const ext = (path: string) => path.split(".").pop()?.toLowerCase() ?? "";
export const isOffice = (path: string) => OFFICE_EXT.includes(ext(path));
/** Other files Convert to PDF takes (Paginaut makes those PDFs itself). */
export const WEB_EXT = ["html", "htm", "xhtml"];
const OTHER_EXT = [...WEB_EXT, "md", "markdown", "epub", "mobi", "fb2", "xps", "oxps", "txt"];
export const convertible = (path: string) => isOffice(path) || OTHER_EXT.includes(ext(path));
export type OfficeKind = "word" | "excel" | "powerpoint";
export function officeKind(path: string): OfficeKind | null {
  const e = ext(path);
  if (!OFFICE_EXT.includes(e)) return null;
  if (/^(doc|dot|rtf|odt|ott)/.test(e)) return "word";
  if (/^(xl|ods|ots)/.test(e)) return "excel";
  return "powerpoint";
}
const PROGRAM: Record<OfficeKind, string> = { word: "Microsoft Word", excel: "Microsoft Excel", powerpoint: "Microsoft PowerPoint" };

export const engines = signal<OfficeEngines | null>(null);
let loading: Promise<OfficeEngines | null> | null = null;

/** What can convert here (asked once; `fresh` looks again). */
export function loadEngines(fresh = false): Promise<OfficeEngines | null> {
  if (fresh || !loading) {
    loading = api.officeEngines(fresh).then(
      (e) => (engines.value = e),
      () => null,
    );
  }
  return loading;
}

/** Tells the converter which one to prefer (before any file opens). */
export function initOffice() {
  void api.officeSetPreference(settings.value.officeEngine);
}

export function setPreference(p: "auto" | OfficeEngine) {
  updateSettings({ officeEngine: p });
  void api.officeSetPreference(p);
  if (engines.value) engines.value = { ...engines.value, preference: p };
}

/** Converters that can do `path`, best first. */
export function available(path: string, e = engines.value): OfficeEngine[] {
  const kind = officeKind(path);
  if (!kind || !e) return [];
  const out: OfficeEngine[] = [];
  if (e.msoffice?.[kind]) out.push("msoffice");
  if (e.pack || e.installed) out.push("libreoffice");
  if (BUILTIN_EXT.includes(ext(path))) out.push("builtin");
  return out;
}

export function engineName(engine: OfficeEngine, path: string) {
  if (engine === "msoffice") return PROGRAM[officeKind(path) ?? "word"];
  if (engine === "libreoffice") return "LibreOffice";
  return "Text preview";
}

/** The converter a file will get with the current preference (for "Converting with…"). */
export function likelyEngine(path: string): OfficeEngine | null {
  const list = available(path);
  const pref = settings.value.officeEngine;
  if (pref !== "auto" && list.includes(pref)) return pref;
  return list[0] ?? null;
}

// ---------------------------------------------------------------- one file, another converter

/** A tab being converted again: the converter's name. */
export const reconverting = signal<Map<string, string>>(new Map());

/** Shows the tab's Office file converted by `engine` instead. */
export async function reconvert(tab: Tab, engine: OfficeEngine) {
  const info = tab.info.value;
  const from = info?.converted?.from ?? (isOffice(tab.path) ? tab.path : null);
  if (!info || !from) return;
  if (tab.dirty.value) {
    const go = await ask("Convert again?", "Your changes to this file haven't been saved and will be lost.", [
      { label: "Convert again", value: "go", primary: true },
      { label: "Cancel", value: "cancel" },
    ]);
    if (go !== "go") return;
  }
  const name = engineName(engine, from);
  reconverting.value = new Map(reconverting.value).set(tab.key, name);
  try {
    const next = await api.open(from, engine);
    await adoptSaved(tab, next);
    void api.close(info.docId);
    showToast(engine === "builtin" ? "Showing the text preview" : `Converted with ${name}`);
  } catch (e) {
    showToast(String(e), 7000);
  } finally {
    const m = new Map(reconverting.value);
    m.delete(tab.key);
    reconverting.value = m;
  }
}

// ---------------------------------------------------------------- Paginaut's LibreOffice

export const officeDialog = signal(false);
export const install = signal<{ stage: InstallEvent["stage"]; done: number; total: number; stopping?: boolean } | null>(null);

/** Why the last setup failed (shown in the Office files dialog). */
export const installError = signal("");

export async function installLibreOffice() {
  if (install.value) return;
  installError.value = "";
  install.value = { stage: "finding", done: 0, total: 0 };
  try {
    engines.value = await api.officeInstall((e) => (install.value = { ...install.value, stage: e.stage, done: e.done, total: e.total }));
    showToast("LibreOffice is ready: Office files now open with their layout.", 6000);
  } catch (e) {
    if (String(e) !== "stopped") {
      installError.value = String(e);
      showToast(`Couldn't set up LibreOffice: ${e}`, 9000);
    }
    void loadEngines(true);
  } finally {
    install.value = null;
  }
}

export function stopInstall() {
  if (install.value) install.value = { ...install.value, stopping: true };
  void api.officeInstallStop();
}

export async function removeLibreOffice() {
  const go = await ask("Remove Paginaut's LibreOffice?", "It frees about 600 MB. You can download it again any time.", [
    { label: "Remove", value: "go", primary: true },
    { label: "Cancel", value: "cancel" },
  ]);
  if (go !== "go") return;
  try {
    engines.value = await api.officeRemove();
    if (settings.value.officeEngine === "libreoffice" && !engines.value.installed) setPreference("auto");
    showToast("Removed Paginaut's LibreOffice");
  } catch (e) {
    showToast(String(e), 6000);
  }
}

// ---------------------------------------------------------------- many files to PDFs

export interface ConvertItem {
  path: string;
  /** Converter for this file ("auto": the preferred one). */
  engine: "auto" | OfficeEngine;
  state: "waiting" | "converting" | "done" | "error";
  out?: string;
  note?: string;
}

/** Files waiting to become PDFs (the Convert pane's "Files to PDF"). */
export const convertDialog = signal<{ items: ConvertItem[]; folder: string | null; running: boolean } | null>(null);

/** The Convert pane (right side) is open, showing this conversion of the document. Shares the side with the Sign pane. */
export type ExportTarget = "docx" | "xlsx" | "pptx" | "md" | "images" | "txt";
export const convertPane = signal<{ target: ExportTarget } | null>(null);
/** The Markdown pane (editing a .md file's source), for this tab. */
export const mdPane = signal<Tab | null>(null);

export function openConvertPane(target?: ExportTarget) {
  convertPane.value = { target: target ?? convertPane.value?.target ?? "docx" };
  mdPane.value = null;
  if (settings.value.signPane) updateSettings({ signPane: false });
}
// Opening the Sign pane closes the others: one pane on the right at a time.
settings.subscribe((s) => {
  if (s.signPane) {
    if (convertPane.peek()) convertPane.value = null;
    if (mdPane.peek()) mdPane.value = null;
  }
});

/** Adds files to "Files to PDF" (those already there stay) and shows the pane. */
export function startConvert(paths: string[] = []) {
  void loadEngines();
  const cur = convertDialog.value;
  const have = new Set(cur?.items.map((i) => i.path.toLowerCase()));
  const added: ConvertItem[] = paths.filter((p) => convertible(p) && !have.has(p.toLowerCase())).map((path) => ({ path, engine: "auto", state: "waiting" }));
  convertDialog.value = { items: [...(cur?.items ?? []), ...added], folder: cur?.folder ?? null, running: cur?.running ?? false };
  openConvertPane();
}

const stem = (p: string) => baseName(p).replace(/\.[^.]+$/, "");
const dirOf = (p: string) => p.slice(0, Math.max(p.lastIndexOf("\\"), p.lastIndexOf("/")));

/** Converts every waiting file; each PDF goes next to its file (or into the chosen folder), never over another. */
export async function runConvert() {
  const d = convertDialog.value;
  if (!d || d.running) return;
  const update = (i: number, patch: Partial<ConvertItem>) => {
    const cur = convertDialog.value;
    if (!cur) return;
    const items = cur.items.slice();
    items[i] = { ...items[i], ...patch };
    convertDialog.value = { ...cur, items };
  };
  convertDialog.value = { ...d, running: true };
  const taken = new Set<string>();
  for (let i = 0; i < d.items.length; i++) {
    const it = convertDialog.value?.items[i];
    if (!it || it.state === "done") continue;
    // A free name: "report.pdf", else "report (2).pdf"…
    const dir = d.folder ?? dirOf(it.path);
    let out = "";
    for (let n = 1; n < 100; n++) {
      const candidate = `${dir}\\${stem(it.path)}${n > 1 ? ` (${n})` : ""}.pdf`;
      if (taken.has(candidate.toLowerCase())) continue;
      if ((await api.existingFiles([candidate])).length === 0) {
        out = candidate;
        break;
      }
    }
    if (!out) {
      update(i, { state: "error", note: "No free file name" });
      continue;
    }
    taken.add(out.toLowerCase());
    update(i, { state: "converting" });
    try {
      const by = await api.officeConvert(it.path, out, it.engine === "auto" ? undefined : it.engine);
      update(i, { state: "done", out, note: by });
    } catch (e) {
      update(i, { state: "error", note: String(e) });
    }
  }
  if (convertDialog.value) convertDialog.value = { ...convertDialog.value, running: false };
}
