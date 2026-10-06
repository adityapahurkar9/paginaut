// The shelf: every e-book and comic in the folders chosen for it (shelf.rs finds them), with their
// covers, where you are in each and which you've finished. Books are known by a fingerprint of their
// contents, so a book moved or renamed keeps its cover and its place.
import { computed, effect, signal } from "@preact/signals";
import { invoke } from "@tauri-apps/api/core";
import { desktopDir, documentDir, downloadDir } from "@tauri-apps/api/path";
import { open } from "@tauri-apps/plugin-dialog";
import type { ViewState } from "./render/Viewer";
import { load, openFiles, recent, rememberFile, save, showToast, tabs } from "./store";

export type ShelfKind = "ebook" | "comic";

export interface ShelfItem {
  path: string;
  name: string;
  size: number;
  mtime: number;
  fp: string;
  kind: ShelfKind;
}

/** What opening a book told us: title, author, pages, and whether a cover was made. */
export interface BookMeta {
  title: string;
  author: string;
  pages: number;
  cover: boolean;
}

/** Where you were in a book, kept by fingerprint (Recent keeps only the last 40 files). */
interface Place {
  view: ViewState;
  pages: number;
  at: number;
}

/** The folders looked in; null until changed: Documents, Downloads and the Desktop. */
export const shelfFolders = signal<string[] | null>(load("paginaut.shelf.folders", { list: null as string[] | null }).list);
export const shelf = signal<ShelfItem[]>(load("paginaut.shelf", { list: [] as ShelfItem[] }).list);
export const metas = signal<Record<string, BookMeta>>(load("paginaut.shelf.meta", { map: {} as Record<string, BookMeta> }).map);
const places = signal<Record<string, Place>>(load("paginaut.shelf.places", { map: {} as Record<string, Place> }).map);
export const scanning = signal(false);
let scannedAt = 0;
let scannedFor = "";

export async function defaultFolders() {
  const dirs = await Promise.all([documentDir(), downloadDir(), desktopDir()].map((p) => p.catch(() => "")));
  return dirs.filter(Boolean);
}

export async function foldersNow() {
  return shelfFolders.value ?? (await defaultFolders());
}

function setFolders(list: string[]) {
  shelfFolders.value = list;
  save("paginaut.shelf.folders", { list });
  void scanShelf();
}

export async function addFolder() {
  const dir = await open({ directory: true, title: "Add a folder to the shelf" });
  if (typeof dir !== "string") return;
  const now = await foldersNow();
  if (now.some((f) => f.toLowerCase() === dir.toLowerCase())) return;
  setFolders([...now, dir]);
}

export async function removeFolder(dir: string) {
  setFolders((await foldersNow()).filter((f) => f !== dir));
}

/** Looks through the folders again (`loud`: says so when it's done). */
export async function scanShelf(loud = false) {
  if (scanning.value) return;
  scanning.value = true;
  try {
    const folders = await foldersNow();
    const { items } = await invoke<{ items: ShelfItem[] }>("scan_books", { folders });
    shelf.value = items;
    save("paginaut.shelf", { list: items });
    scannedAt = Date.now();
    scannedFor = folders.join("|");
    if (loud) showToast(`${items.length} ${items.length === 1 ? "book" : "books"} on the shelf`);
  } catch (e) {
    if (loud) showToast(`Couldn't look through the folders: ${e}`);
  } finally {
    scanning.value = false;
  }
  void fillCovers();
}

/** On showing the shelf: looks again if the folders changed or it's been ten minutes. */
export async function scanIfStale() {
  const folders = (await foldersNow()).join("|");
  if (folders !== scannedFor || Date.now() - scannedAt > 600_000) await scanShelf();
}

let filling = false;
/** Titles, authors and covers for books not opened yet, one at a time. */
async function fillCovers() {
  if (filling) return;
  filling = true;
  try {
    for (const it of shelf.value) {
      if (!it.fp || metas.value[it.fp]) continue;
      // Not while documents are loading: they come first.
      while (tabs.value.some((t) => t.status.value === "loading")) await new Promise((r) => setTimeout(r, 500));
      const m = await invoke<BookMeta>("book_cover", { path: it.path, fp: it.fp }).catch(() => ({ title: "", author: "", pages: 0, cover: false }));
      metas.value = { ...metas.value, [it.fp]: m };
      save("paginaut.shelf.meta", { map: metas.value });
    }
  } finally {
    filling = false;
  }
}

const byPath = computed(() => new Map(shelf.value.map((it) => [it.path.toLowerCase(), it])));

// Recent holds where you are in an open book; kept here too, by fingerprint, for as long as the
// book is on the shelf.
effect(() => {
  const list = recent.value;
  const map = byPath.value;
  let next: Record<string, Place> | null = null;
  for (const r of list) {
    const it = map.get(r.path.toLowerCase());
    if (!it?.fp || !r.view || !r.pages) continue;
    const had = (next ?? places.peek())[it.fp];
    if (had && had.at >= r.opened && had.view.page === r.view.page) continue;
    next ??= { ...places.peek() };
    next[it.fp] = { view: r.view, pages: r.pages, at: r.opened };
  }
  if (next) {
    places.value = next;
    save("paginaut.shelf.places", { map: next });
  }
});

export interface Shelved {
  it: ShelfItem;
  title: string;
  author: string;
  /** 0–1, or null when never opened. */
  progress: number | null;
  at: number;
}

function placeOf(it: ShelfItem): Place | null {
  const r = recent.value.find((r) => r.path.toLowerCase() === it.path.toLowerCase());
  const p = it.fp ? places.value[it.fp] : undefined;
  if (r?.view && r.pages && (!p || r.opened >= p.at)) return { view: r.view, pages: r.pages, at: r.opened };
  return p ?? null;
}

export function shelved(it: ShelfItem): Shelved {
  const m = it.fp ? metas.value[it.fp] : undefined;
  const p = placeOf(it);
  return {
    it,
    title: m?.title || it.name.replace(/\.[^.]+$/, ""),
    author: m?.author ?? "",
    progress: p ? Math.min(1, (p.view.page + p.view.frac + 1) / p.pages) : null,
    at: p?.at ?? 0,
  };
}

export type ShelfSort = "title" | "author" | "added";

/** A kind's books: being read (opened in the last 60 days, not finished), the rest, and finished. */
export function sections(kind: ShelfKind, sort: { key: ShelfSort; desc: boolean }, find: string) {
  const q = find.trim().toLowerCase();
  const all = shelf.value
    .filter((it) => it.kind === kind)
    .map(shelved)
    .filter((b) => !q || `${b.title} ${b.author} ${b.it.name}`.toLowerCase().includes(q));
  const sign = sort.desc ? -1 : 1;
  const by = (a: Shelved, b: Shelved) =>
    sign *
    (sort.key === "added"
      ? a.it.mtime - b.it.mtime
      : sort.key === "author"
        ? (a.author || "~").localeCompare(b.author || "~") || a.title.localeCompare(b.title)
        : a.title.localeCompare(b.title));
  const finished = (b: Shelved) => b.progress !== null && b.progress >= 0.995;
  const reading = all.filter((b) => b.progress !== null && !finished(b) && Date.now() - b.at < 60 * 86_400_000).sort((a, b) => b.at - a.at);
  return {
    reading,
    rest: all.filter((b) => !reading.includes(b) && !finished(b)).sort(by),
    finished: all.filter(finished).sort(by),
  };
}

export const shelfCount = (kind: ShelfKind) => shelf.value.filter((it) => it.kind === kind).length;

/** Opens a book where you left it, even if it has left Recent or moved since. */
export function openBook(it: ShelfItem) {
  const inRecent = recent.value.some((r) => r.path.toLowerCase() === it.path.toLowerCase() && r.view);
  const p = it.fp ? places.value[it.fp] : undefined;
  if (!inRecent && p) rememberFile(it.path, { view: p.view, pages: p.pages });
  void openFiles([it.path]);
}
