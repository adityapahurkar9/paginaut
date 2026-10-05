// Opening and closing documents, and remembering where you were in them.
import { signal } from "@preact/signals";
import { api, type DocInfo } from "../api";
import { releaseAnalysis } from "./flow";
import { native, type Picked } from "./native";
import { stopReading } from "./speech";
import { back, pushLayer } from "./nav";
import { findRecord, openedRecord, recordById, tidyCache, unlist, updateRecord, type ReadRecord } from "./records";
import { busy, chrome, doc, errorText, isTablet, layout, mode, page, say, searching, sheet } from "./state";

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

const EBOOKS = ["epub", "mobi", "fb2", "fbz"];
const COMICS = ["cbz", "cbr", "cbt"];

const kind = (info: DocInfo, name: string) => (info.isPdf ? "PDF" : (name.split(".").pop() ?? info.format).toUpperCase());

/** Opens a file Android handed over (picked, sent, from Recent or the shelf). False if it couldn't be opened
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
  // Known by its contents, wherever it came from this time (or as an edited copy of one read before).
  const at = findRecord({ fp: p.fp ?? "", uri, name, pages: info.pageCount, title: info.title });
  page.value = at ? Math.min(at.page, info.pageCount - 1) : 0;
  // Opened before: as it was read. New: e-books as a book (their text as pages you turn), comics as
  // pages turned one by one (two side by side on a tablet held sideways), everything else one page
  // at a time.
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  const ebook = info.reflowable && EBOOKS.includes(ext);
  const comic = COMICS.includes(ext);
  mode.value = at?.mode ?? (ebook ? "book" : "pages");
  layout.value = at?.layout ?? (comic && isTablet ? "double" : "paged");
  chrome.value = true;
  startPosition = at ? { page: page.value, frac: at.frac } : { page: 0, frac: 0 };
  // Remembered even when it can't be opened from Recent (a big file lent once): where you were
  // comes back when it's sent again.
  const rec = openedRecord(at, {
    uri,
    fp: p.fp ?? "",
    copy: p.kept ? p.path : "",
    lasting: !!p.lasting,
    name,
    title: info.title,
    format: kind(info, name),
    pages: info.pageCount,
    page: page.value,
    frac: startPosition.frac,
    mode: mode.value,
    layout: layout.value,
    at: Date.now(),
  });
  doc.value = { info, name, uri, path: p.path, rec, inPlace: p.inPlace };
  // A new copy may take the cache over its size.
  if (p.kept) void tidyCache(p.path);
  // One history entry for the reader, replaced (not stacked) when another file opens from it.
  if (!wasOpen) pushLayer(closeCurrent);
  return true;
}

/** The place a document opened at (the viewer reads it once it's laid out). */
export let startPosition = { page: 0, frac: 0 };

/** Saves where you are in the open document (on page turns, leaving, and when the app goes to the background). */
export function savePosition() {
  const d = doc.value;
  if (!d) return;
  const pos = positionOf();
  updateRecord(d.rec, { page: pos.page, frac: pos.frac, mode: mode.value, layout: layout.value, at: Date.now() });
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
      if (d.inPlace) void native.release(d.path);
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

export async function openRecent(r: ReadRecord) {
  busy.value = "Opening…";
  try {
    const p = await native.reopen(r.uris[0]);
    busy.value = null;
    await openPicked(p);
  } catch (e) {
    busy.value = null;
    // The source is gone or no longer readable: the cached copy may still be there.
    if (r.copy && (await openPicked({ path: r.copy, name: r.name, uri: r.uris[0], kept: true, fp: r.fp }, true))) return;
    const msg = errorText(e);
    say(msg);
    // Gone, or no longer Paginaut's to open (PaginautPlugin.copyThenResolve): off Recent. Where you
    // were is kept, for when the file comes back.
    if (/isn't there any more|can no longer open/.test(msg) && recordById(r.id)) unlist(r.id);
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
