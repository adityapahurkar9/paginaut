// The shelf: every e-book and comic in the folders chosen for it, or (with all-files access)
// anywhere on the phone. Books are read where they are; covers and titles come from the books.
import { computed, signal } from "@preact/signals";
import { native, type Picked } from "./native";
import { openPicked } from "./open";
import { inProgress, isFinished, records, refreshStorage, storage, type ReadRecord } from "./records";
import { busy, doc, errorText, load, save, say } from "./state";

export interface ShelfItem {
  /** Opens it with `native.reopen`: a file:// path, or a document in a chosen folder. */
  id: string;
  name: string;
  size: number;
  mtime: number;
  fp: string;
  kind: "ebook" | "comic";
}

export interface BookMeta {
  title: string;
  author: string;
  pages: number;
  /** A cover was made (paginaut://…/cover?f=<fp>). */
  cover: boolean;
}

const KEY = "paginaut.m.shelf";
const META = "paginaut.m.shelfmeta";

export const shelf = signal<ShelfItem[]>(load<ShelfItem[]>(KEY, []));
/** Titles, authors and covers, by fingerprint. */
export const metas = signal<Record<string, BookMeta>>(load<Record<string, BookMeta>>(META, {}));
export const scanning = signal(false);

/** The shelf has somewhere to look: a chosen folder, or all-files access. */
export const shelfOn = computed(() => {
  const st = storage.value;
  return !!st && (st.allFiles || st.folders.length > 0);
});

export const COVER_BASE = /Windows|Android/.test(navigator.userAgent) ? "http://paginaut.localhost/cover" : "paginaut://localhost/cover";

/** The record of a book on the shelf (opened before, here or sent by another app). */
export function recordFor(it: ShelfItem, list: ReadRecord[] = records.value): ReadRecord | undefined {
  return list.find((r) => r.uris.includes(it.id) || (!!it.fp && r.fp === it.fp));
}

export type Shelved = { it: ShelfItem; r?: ReadRecord; m?: BookMeta; title: string; author: string };

/** A book's title and author as the shelf shows them (from the book, or its file name). */
export function shelved(it: ShelfItem, list: ReadRecord[], m: Record<string, BookMeta>): Shelved {
  const meta = m[it.fp];
  return { it, r: recordFor(it, list), m: meta, title: meta?.title?.trim() || it.name.replace(/\.[^.]+$/, "").replace(/[_]+/g, " "), author: meta?.author?.trim() ?? "" };
}

/** Shelf books of a kind, sorted into being read (latest first), not started, and finished. */
export function sections(kind: ShelfItem["kind"], sort: "title" | "author" | "added") {
  const list = records.value;
  const m = metas.value;
  const all = shelf.value.filter((it) => it.kind === kind).map((it) => shelved(it, list, m));
  const by = (a: Shelved, b: Shelved) =>
    sort === "added" ? b.it.mtime - a.it.mtime : sort === "author" ? (a.author || "~").localeCompare(b.author || "~") || a.title.localeCompare(b.title) : a.title.localeCompare(b.title);
  const reading = all.filter((s) => s.r && inProgress(s.r)).sort((a, b) => b.r!.at - a.r!.at);
  const finished = all.filter((s) => s.r && isFinished(s.r)).sort(by);
  const rest = all.filter((s) => !reading.includes(s) && !finished.includes(s)).sort(by);
  return { all, reading, rest, finished };
}

/** What the last scan could reach, and when it ran (scanIfStale). */
let lastScan = { at: 0, access: "" };
const accessKey = () => {
  const st = storage.value;
  return st ? `${st.allFiles}|${st.folders.map((f) => f.uri).join("|")}` : "";
};

/** Looks again when the app comes back, if access changed (a folder, all-files access) or it's been ten minutes. */
export function scanIfStale() {
  if (accessKey() !== lastScan.access || Date.now() - lastScan.at > 600_000) void scanShelf();
}

/** Looks for books again (at start, when access changes, on pulling the shelf down). */
export async function scanShelf(loud = false) {
  if (scanning.value) return;
  scanning.value = true;
  try {
    const st = await refreshStorage();
    lastScan = { at: Date.now(), access: accessKey() };
    if (!st || (!st.allFiles && !st.folders.length)) {
      shelf.value = [];
      save(KEY, []);
      return;
    }
    const { items } = await native.scanBooks();
    const before = new Set(shelf.value.map((it) => it.id));
    const added = items.filter((it) => !before.has(it.id)).length;
    shelf.value = items;
    save(KEY, items);
    // Covers of books no longer anywhere are forgotten (the files in the cache go with Android's cleanups).
    const fps = new Set(items.map((it) => it.fp));
    const m = Object.fromEntries(Object.entries(metas.value).filter(([fp]) => fps.has(fp)));
    metas.value = m;
    save(META, m);
    if (loud) say(added ? `Found ${added} new ${added === 1 ? "book" : "books"}` : `${items.length} books, nothing new`);
  } catch (e) {
    if (loud) say(errorText(e));
  } finally {
    scanning.value = false;
  }
  void fillCovers();
}

let filling = false;

/** Reads each new book's title, author and cover, one at a time, while no document is open. */
async function fillCovers() {
  if (filling) return;
  filling = true;
  try {
    for (const it of shelf.value) {
      if (doc.value || busy.value) break;
      if (!it.fp || metas.value[it.fp]) continue;
      let p: Picked | null = null;
      let meta: BookMeta = { title: "", author: "", pages: 0, cover: false };
      try {
        p = await native.reopen(it.id);
        if (p.path) meta = await native.bookCover(p.path, it.fp);
      } catch {
        /* a book that won't open keeps its file name and a plain cover */
      } finally {
        if (p?.inPlace && p.path) void native.release(p.path);
      }
      metas.value = { ...metas.value, [it.fp]: meta };
      save(META, metas.value);
    }
  } finally {
    filling = false;
  }
}

export async function openShelf(it: ShelfItem) {
  busy.value = "Opening…";
  try {
    const p = await native.reopen(it.id);
    busy.value = null;
    await openPicked(p);
  } catch (e) {
    busy.value = null;
    say(errorText(e));
    void scanShelf();
  }
}

export async function addFolder() {
  try {
    const f = await native.addFolder();
    if (f.uri) {
      say(`Looking for books in ${f.name}…`);
      await scanShelf(true);
    }
  } catch (e) {
    say(errorText(e));
  }
}

export async function removeFolder(uri: string) {
  await native.removeFolder(uri);
  await scanShelf();
}
