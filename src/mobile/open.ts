// Opening and closing documents, and remembering where you were in them.
import { signal } from "@preact/signals";
import { api, type DocInfo } from "../api";
import { releaseAnalysis } from "./flow";
import { native, type Picked } from "./native";
import { stopReading } from "./speech";
import { back, pushLayer } from "./nav";
import { busy, chrome, doc, errorText, isTablet, mode, page, recent, remember, say, searching, sheet, type Recent } from "./state";

/** A password asked for: the file's name, and how to answer (null: cancelled). */
export const passwordAsk = signal<{ name: string; wrong: boolean; answer: (pw: string | null) => void } | null>(null);

function askPassword(name: string, wrong: boolean) {
  return new Promise<string | null>((resolve) => {
    passwordAsk.value = {
      name,
      wrong,
      answer: (pw) => {
        passwordAsk.value = null;
        resolve(pw);
      },
    };
  });
}

/** Where to save the reading position (set by the reader: the viewer's own state, or the reflowed text's page). */
export let positionOf: () => { page: number; frac: number } = () => ({ page: page.value, frac: 0 });
export function setPositionSource(f: () => { page: number; frac: number }) {
  positionOf = f;
}

const kind = (info: DocInfo, name: string) => (info.isPdf ? "PDF" : (name.split(".").pop() ?? info.format).toUpperCase());

/** Opens a file Android handed over (picked, sent, or from Recent). False if it couldn't be opened
 *  (`quiet`: without saying so). */
export async function openPicked(p: Picked, quiet = false): Promise<boolean> {
  if (!p.path) return false;
  const name = p.name ?? p.path.split("/").pop() ?? "Document";
  const uri = p.uri ?? p.path;
  busy.value = "Opening…";
  let info: DocInfo;
  try {
    info = await api.open(p.path);
  } catch (e) {
    busy.value = null;
    if (!quiet) say(`Couldn't open ${name}: ${errorText(e)}`);
    return false;
  }
  busy.value = null;
  if (info.needsPassword) {
    let wrong = false;
    for (;;) {
      const pw = await askPassword(name, wrong);
      if (pw === null) {
        void api.close(info.docId);
        return true;
      }
      const unlocked = await api.unlock(info.docId, pw).catch(() => null);
      if (unlocked) {
        info = unlocked;
        break;
      }
      wrong = true;
    }
  }
  const wasOpen = !!doc.value;
  closeCurrent();
  const prev = recent.value.find((r) => r.uri === uri);
  const at = prev && prev.pages === info.pageCount ? prev : null;
  page.value = at ? Math.min(at.page, info.pageCount - 1) : 0;
  // E-books and Markdown are laid out on book-sized pages, too small to read on a phone: they open
  // in Read mode. Everything else (PDFs, comics, XPS) opens as pages.
  const saved = at?.mode === "book" && !isTablet ? "read" : at?.mode;
  mode.value = saved ?? (info.reflowable ? "read" : "pages");
  chrome.value = true;
  doc.value = { info, name, uri, path: p.path, direct: p.direct };
  startPosition = at ? { page: page.value, frac: at.frac } : { page: 0, frac: 0 };
  // Big files read in place aren't kept, so Recent couldn't open them again.
  if (!p.direct) remember({ uri, name, path: p.path, format: kind(info, name), pages: info.pageCount, page: page.value, frac: startPosition.frac, mode: mode.value, at: Date.now() });
  // One history entry for the reader, replaced (not stacked) when another file opens from it.
  if (!wasOpen) pushLayer(closeCurrent);
  return true;
}

/** The place a document opened at (the viewer reads it once it's laid out). */
export let startPosition = { page: 0, frac: 0 };

/** Saves where you are in the open document (on page turns, leaving, and when the app goes to the background). */
export function savePosition() {
  const d = doc.value;
  if (!d || d.direct) return;
  const pos = positionOf();
  const prev = recent.value.find((r) => r.uri === d.uri);
  remember({
    uri: d.uri,
    name: d.name,
    path: d.path,
    format: prev?.format ?? kind(d.info, d.name),
    pages: d.info.pageCount,
    page: pos.page,
    frac: pos.frac,
    mode: mode.value,
    at: Date.now(),
  });
}

/** Closes the open document (the reader's history entry is taken off by whoever asked). */
export function closeCurrent() {
  const d = doc.value;
  if (!d) return;
  savePosition();
  stopReading();
  releaseAnalysis(d.info.docId);
  doc.value = null;
  sheet.value = null;
  searching.value = false;
  void api
    .close(d.info.docId)
    .catch(() => {})
    .then(() => {
      if (d.direct) void native.release(d.path);
    });
}

/** The reader's back arrow. */
export function leaveReader() {
  back();
}

export async function pickAndOpen() {
  try {
    const p = await native.pickFile();
    if (p.path) await openPicked(p);
  } catch (e) {
    say(errorText(e));
  }
}

export async function openRecent(r: Recent) {
  busy.value = "Opening…";
  try {
    const p = await native.reopen(r.uri);
    busy.value = null;
    await openPicked(p);
  } catch (e) {
    busy.value = null;
    // The source is gone or no longer readable: the cached copy may still be there.
    if (!(await openPicked({ path: r.path, name: r.name, uri: r.uri }, true))) say(errorText(e));
  }
}

/** Files sent by other apps: at start, and whenever the app comes back to the front. */
export async function checkIntent() {
  try {
    const p = await native.takeIntent();
    if (p.path) await openPicked(p);
  } catch (e) {
    say(errorText(e));
  }
}
