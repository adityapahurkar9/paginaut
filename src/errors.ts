// The page's side of the error log (report.rs, errlog.rs): script errors and promises nobody caught go to the log,
// and the report a person can hand over is put together here, for the desktop (ReportPane) and Android (Sheets).
// Nothing here sends anything anywhere: the log is a file on this computer or phone.
import { invoke } from "@tauri-apps/api/core";

/** Lines one window sends at most: a loop of errors mustn't fill the log (it rolls over anyway). */
const MOST = 60;
let sent = 0;

/** Writes a line to the error log (cleaned of paths and names on the Rust side). */
export function logError(kind: string, message: string) {
  if (sent >= MOST || !message.trim()) return;
  sent++;
  void invoke("log_error", { kind, message }).catch(() => {});
}

/** A thrown value as text: name, message and the first lines of its stack. */
export function describe(x: unknown): string {
  if (x instanceof Error) {
    // Chromium's stack starts with "Name: message" again; WebKit's doesn't.
    const lines = (x.stack ?? "").split("\n").filter((l) => l.trim());
    if (lines[0]?.startsWith(`${x.name}:`) || lines[0] === x.name) lines.shift();
    const stack = lines.slice(0, 12).join("\n");
    return `${x.name}: ${x.message}${stack ? `\n${stack}` : ""}`;
  }
  if (typeof x === "string") return x;
  try {
    return JSON.stringify(x) ?? String(x);
  } catch {
    return String(x);
  }
}

/** Something the user stopped, not something that went wrong. */
const STOPPED = /cancel|stopped by|aborted/i;

/** Catches the page's script errors and unhandled rejections into the log. */
export function catchPageErrors() {
  window.addEventListener("error", (e) => {
    const where = e.filename ? ` (${e.filename}:${e.lineno}:${e.colno})` : "";
    logError("script", e.error ? describe(e.error) : `${e.message}${where}`);
  });
  window.addEventListener("unhandledrejection", (e) => {
    const text = describe(e.reason);
    if (!STOPPED.test(text)) logError("promise", text);
  });
}

/** A command that failed in a way the user sees (opening, saving, converting…), for the log. */
export function logFailure(cmd: string, e: unknown) {
  const text = describe(e);
  if (!STOPPED.test(text)) logError("failed", `${cmd}: ${text}`);
}

// ---------------------------------------------------------------- the report

export type Rows = [string, string][];

/** What the page itself can tell: the graphics card (as the web view names it) and the screen. */
export function pageDetails(): Rows {
  const rows: Rows = [];
  try {
    const gl = document.createElement("canvas").getContext("webgl");
    const info = gl?.getExtension("WEBGL_debug_renderer_info");
    const gpu = gl && (info ? gl.getParameter(info.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER));
    if (gpu) rows.push(["Graphics", String(gpu)]);
  } catch {
    // No WebGL: nothing to say about the graphics.
  }
  rows.push(["Screen", `${screen.width} × ${screen.height}, scale ${Math.round(devicePixelRatio * 100)}%`]);
  rows.push(["Language", navigator.language]);
  return rows;
}

/** What goes in a report: each part is the user's choice (null: left out). */
export interface ReportParts {
  details: Rows | null;
  log: string | null;
}

export type Platform = "Windows" | "Linux" | "Android";

/** What the system details hold, said before anything is shared. */
export function detailsHold(p: Platform): string {
  const common = "Paginaut's version; processor type and threads; memory; the web view's version; graphics card; screen size and scale; language";
  if (p === "Android") return `The phone's maker and model, Android version and ABIs; ${common}.`;
  if (p === "Linux") return `The Linux distribution and kernel version; the desktop session; how Paginaut was installed (.deb or AppImage); ${common}.`;
  return `The Windows edition and version; ${common}.`;
}

/** What the other parts hold, said before anything is shared (desktop and Android say the same). */
export const REPORT_HOLDS = {
  log: "When something went wrong and what the app said about it (crashes, files that failed to open, save or convert, errors in the app itself). Paths become <file>.pdf and the home folder ~.",
  never: "Not in it: your documents or their text, file and folder names, passwords, signatures or certificates, form entries, your user name or accounts. The text below is exactly what would be shared: read it first.",
};

const rowsText = (rows: Rows) => {
  const w = Math.max(...rows.map(([k]) => k.length));
  return rows.map(([k, v]) => `${k.padEnd(w)}  ${v}`).join("\n");
};

/** The report as plain text, with only the parts chosen. */
export function reportText(p: ReportParts): string {
  const out = ["Paginaut problem report", ""];
  if (p.details) out.push("System details", rowsText(p.details), "");
  if (p.log !== null) out.push("Error log (file paths and document names removed)", p.log.trim() || "(empty: nothing has been logged)", "");
  if (!p.details && p.log === null) out.push("(Nothing chosen to include.)", "");
  return out.join("\n");
}

/** The issue form in the repository (.github/ISSUE_TEMPLATE/problem.yml): its field ids are filled from the address. */
const ISSUES = "https://github.com/adityapahurkar9/paginaut/issues/new";
const FORM = "problem.yml";
/** Kept under what browsers and GitHub take in an address. */
const MOST_URL = 7500;

/** A new issue on GitHub's form, filled with the chosen parts and as much of the log's end as fits; `cut` when some of
 *  the log was left out. */
export function issueUrl(p: ReportParts, platform: Platform): { url: string; cut: boolean } {
  const make = (tail: string | null, cut: boolean) => {
    const q: Record<string, string> = { template: FORM, title: "Problem: ", platform };
    if (p.details) q.system = rowsText(p.details);
    if (tail !== null) q.log = cut ? `(The last lines only: the whole log is in the saved report, which can be attached below.)\n${tail}` : tail || "(empty)";
    return `${ISSUES}?${new URLSearchParams(q).toString()}`;
  };
  if (p.log === null) return { url: make(null, false), cut: false };
  const lines = p.log.trim().split("\n");
  let url = make(lines.join("\n"), false);
  if (url.length <= MOST_URL) return { url, cut: false };
  // The newest lines are the ones that matter: as many of them as fit.
  let lo = 0;
  let hi = lines.length;
  while (lo < hi) {
    const mid = Math.floor((lo + hi) / 2);
    if (make(lines.slice(mid).join("\n"), true).length <= MOST_URL) hi = mid;
    else lo = mid + 1;
  }
  url = make(lines.slice(lo).join("\n"), true);
  return { url, cut: true };
}
