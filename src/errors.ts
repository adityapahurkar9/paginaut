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

/** The report as plain text: the details, then the log. */
export function reportText(details: Rows, log: string): string {
  const w = Math.max(...details.map(([k]) => k.length));
  const lines = details.map(([k, v]) => `${k.padEnd(w)}  ${v}`);
  return [
    "Paginaut problem report",
    "",
    "System details",
    ...lines,
    "",
    "Error log (file paths and document names removed)",
    log.trim() || "(empty: nothing has been logged)",
    "",
  ].join("\n");
}

const ISSUES = "https://github.com/adityapahurkar9/paginaut/issues/new";
/** Kept under what browsers and GitHub take in an address. */
const MOST_URL = 7500;

/** A new GitHub issue with the details and as much of the log's end as fits; `cut` when some of the log was left out. */
export function issueUrl(details: Rows, log: string): { url: string; cut: boolean } {
  const w = Math.max(...details.map(([k]) => k.length));
  const sys = details.map(([k, v]) => `${k.padEnd(w)}  ${v}`).join("\n");
  const make = (tail: string, cut: boolean) => {
    const body = [
      "**What happened**",
      "",
      "<!-- What you did, what you expected, and what happened instead. -->",
      "",
      "**System details**",
      "```",
      sys,
      "```",
      "",
      `**Error log${cut ? " (its last lines: the whole log is in the saved report, which you can attach here)" : ""}**`,
      "```",
      tail || "(empty)",
      "```",
    ].join("\n");
    return `${ISSUES}?${new URLSearchParams({ title: "Problem: ", body }).toString()}`;
  };
  const lines = log.trim().split("\n");
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
