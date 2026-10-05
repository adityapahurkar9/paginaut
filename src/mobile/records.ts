// What Paginaut remembers about each file you've read (where you were, your bookmarks), apart from
// the file itself; and which copies of files lent by other apps it keeps.
//
// A record outlives the file: the copy of a file WhatsApp lent once can go, and when the same file
// comes again (from WhatsApp with a new link, from Files, renamed) it is known by its contents
// and opens where you left it. Recent lists the records whose file can still be opened.
import { computed, signal } from "@preact/signals";
import { native, type Storage } from "./native";
import { load, save, type Layout, type Mode } from "./state";

export interface Bookmark {
  page: number;
  at: number;
}

export interface ReadRecord {
  id: string;
  /** The contents' fingerprint (PaginautPlugin.fingerprint); "" until the file is opened again (records from before). */
  fp: string;
  /** Where it came from (content://… or a path), the latest first. */
  uris: string[];
  /** Its copy in the cache, or "". */
  copy: string;
  /** Its source can be opened again whenever (picked here, in a shelf folder, a path with all-files access). */
  lasting: boolean;
  name: string;
  /** The document's own title (to know an edited file again). */
  title: string;
  format: string;
  pages: number;
  /** Where you were: page, fraction of it scrolled past, and the way you read it. */
  page: number;
  frac: number;
  mode: Mode;
  /** Its pages in Pages mode: scrolled, one by one, or two side by side. */
  layout?: Layout;
  bookmarks: Bookmark[];
  /** Last opened. */
  at: number;
  /** In the Recent list (Remove takes it off; where you were is kept). */
  listed: boolean;
}

const KEY = "paginaut.m.records";
const DAY = 86_400_000;
/** Records not opened for a year are forgotten. */
const KEEP_FOR = 365 * DAY;
const MAX_RECENT = 60;

function initial(): ReadRecord[] {
  const now = Date.now();
  const kept = load<ReadRecord[] | null>(KEY, null);
  if (kept) return kept.filter((r) => now - r.at < KEEP_FOR);
  // From before records: Recent's entries and the bookmarks kept by link.
  type Old = { uri: string; name: string; path: string; format: string; pages: number; page: number; frac: number; mode: Mode; at: number };
  const old = load<Old[]>("paginaut.m.recent", []);
  const marks = load<Record<string, Bookmark[]>>("paginaut.m.bookmarks", {});
  const list = old.map<ReadRecord>((o) => ({
    id: newId(),
    fp: "",
    uris: [o.uri],
    copy: o.path,
    lasting: false,
    name: o.name,
    title: "",
    format: o.format,
    pages: o.pages,
    page: o.page,
    frac: o.frac,
    mode: o.mode,
    bookmarks: marks[o.uri] ?? [],
    at: o.at,
    listed: true,
  }));
  save(KEY, list);
  try {
    localStorage.removeItem("paginaut.m.recent");
    localStorage.removeItem("paginaut.m.bookmarks");
  } catch {
    /* kept */
  }
  return list;
}

function newId() {
  return Date.now().toString(36) + Math.random().toString(36).slice(2, 8);
}

export const records = signal<ReadRecord[]>(initial());

function persist(list: ReadRecord[]) {
  records.value = list;
  save(KEY, list);
}

export const recordById = (id: string) => records.value.find((r) => r.id === id);

/**
 * The record for a file just opened, decided on what matches: the same contents; or (an edited
 * file: signed, annotated) the same link or the same name, with the same number of pages and the
 * same title. A name alone never is: "scan.pdf" is a thousand different files.
 */
export function findRecord(f: { fp: string; uri: string; name: string; pages: number; title: string }): ReadRecord | undefined {
  const list = records.value;
  return (
    (f.fp ? list.find((r) => r.fp === f.fp) : undefined) ??
    list.find((r) => r.pages === f.pages && r.uris.includes(f.uri)) ??
    list.find((r) => r.pages === f.pages && r.name === f.name && r.title === f.title)
  );
}

/** Records the file just opened (updating its record, or a new one); returns the record's id. */
export function openedRecord(prev: ReadRecord | undefined, f: Omit<ReadRecord, "id" | "uris" | "bookmarks" | "listed"> & { uri: string }): string {
  const { uri, ...rest } = f;
  const r: ReadRecord = {
    id: prev?.id ?? newId(),
    bookmarks: prev?.bookmarks ?? [],
    ...rest,
    // A fingerprint can't be read for some sources: the one known stays.
    fp: rest.fp || prev?.fp || "",
    uris: [uri, ...(prev?.uris ?? []).filter((u) => u !== uri)].slice(0, 6),
    listed: true,
  };
  persist([r, ...records.value.filter((x) => x.id !== r.id)]);
  return r.id;
}

export function updateRecord(id: string, patch: Partial<ReadRecord>) {
  const list = records.value;
  const i = list.findIndex((r) => r.id === id);
  if (i < 0) return;
  const next = list.slice();
  next[i] = { ...list[i], ...patch };
  persist(next);
}

/** Takes a file off Recent (where you were in it is kept). */
export const unlist = (id: string) => updateRecord(id, { listed: false });

/** Empties Recent (where you were in each file is kept). */
export function unlistAll() {
  persist(records.value.map((r) => (r.listed ? { ...r, listed: false } : r)));
}

/** Forgets where you were in every file, and your bookmarks. */
export function forgetAll() {
  persist([]);
}

export const bookmarksOf = (id: string) => recordById(id)?.bookmarks ?? [];

/** Marks the page, or unmarks it if it was. Returns whether it is marked now. */
export function toggleBookmark(id: string, page: number): boolean {
  const list = bookmarksOf(id);
  const on = !list.some((b) => b.page === page);
  updateRecord(id, { bookmarks: on ? [...list, { page, at: Date.now() }].sort((a, b) => a.page - b.page) : list.filter((b) => b.page !== page) });
  return on;
}

// ---------------------------------------------------------------- reading progress

/** How far through: 0 to 1. */
export const progressOf = (r: ReadRecord) => (r.pages > 0 ? Math.min(1, (r.page + r.frac + 1) / r.pages) : 0);
export const isFinished = (r: ReadRecord) => r.pages > 1 && (r.page >= r.pages - 1 || progressOf(r) >= 0.97);
/** Being read: begun, not finished, and opened in the last 60 days. */
export const inProgress = (r: ReadRecord) => (r.page > 0 || r.frac > 0.02) && !isFinished(r) && Date.now() - r.at < 60 * DAY;
/** Short files (receipts, tickets, forms): their copies go a week after they were last opened. */
export const SHORT = 5;
export const SHORT_FOR = 7 * DAY;

// ---------------------------------------------------------------- what can be opened, and the cache

/** What the app keeps and can reach (copies, picked files, shelf folders, all-files access). */
export const storage = signal<Storage | null>(null);

export async function refreshStorage() {
  try {
    storage.value = await native.storage();
  } catch {
    /* as it was */
  }
  return storage.value;
}

/** Whether Paginaut can open `uri` again whenever it likes (PaginautPlugin.canReach, from what it reported). */
export function canReach(uri: string, st: Storage) {
  if (uri.startsWith("file:")) return st.allFiles;
  return st.picked.includes(uri) || st.folders.some((f) => uri.startsWith(f.uri + "/document/"));
}

/** Whether a record's file can be opened from Recent: its copy is kept, or its source can be reached. */
export function reachable(r: ReadRecord, st: Storage | null) {
  if (!st) return true;
  return (!!r.copy && st.copies.some((c) => c.path === r.copy)) || (r.lasting && canReach(r.uris[0], st));
}

/** Recent: files opened lately that can still be opened, the latest first. */
export const recent = computed(() => {
  const st = storage.value;
  return records.value.filter((r) => r.listed && reachable(r, st)).slice(0, MAX_RECENT);
});

/** The cache's size: copies of files lent by other apps. Beyond it, copies go (tidyCache). */
export const CACHE_LIMIT = 512 * 1024 * 1024;

/** Why a copy is still kept, or when it goes. */
export function copyState(r: ReadRecord | undefined): { label: string; tone: "progress" | "short" | "plain" } {
  if (!r) return { label: "not used", tone: "plain" };
  if (r.pages < SHORT) {
    const days = Math.max(1, Math.ceil((r.at + SHORT_FOR - Date.now()) / DAY));
    return { label: `goes in ${days} d`, tone: "short" };
  }
  if (inProgress(r)) return { label: "in progress", tone: "progress" };
  if (isFinished(r)) return { label: "finished", tone: "plain" };
  return { label: "", tone: "plain" };
}

let tidying: Promise<void> | null = null;

/**
 * Lets copies go: those no record uses; short files a week after they were last opened; and, over
 * 512 MB, files not being read (the biggest first), then files being read (the longest unopened
 * first). The open document's copy stays.
 */
export function tidyCache(keep?: string): Promise<void> {
  tidying ??= (async () => {
    const st = await refreshStorage();
    if (!st) return;
    const now = Date.now();
    const byCopy = new Map(records.value.filter((r) => r.copy).map((r) => [r.copy, r]));
    const drop: string[] = [];
    const kept: { path: string; size: number; r: ReadRecord }[] = [];
    let total = 0;
    for (const c of st.copies) {
      if (c.path === keep) {
        total += c.size;
        continue;
      }
      const r = byCopy.get(c.path);
      // (A copy made in the last minute may be for a file still being opened.)
      if (!r) {
        if (now - c.at > 60_000) drop.push(c.path);
        else total += c.size;
      } else if (r.pages < SHORT && now - r.at > SHORT_FOR) drop.push(c.path);
      else {
        kept.push({ path: c.path, size: c.size, r });
        total += c.size;
      }
    }
    if (total > CACHE_LIMIT) {
      const idle = kept.filter((k) => !inProgress(k.r)).sort((a, b) => b.size - a.size);
      const reading = kept.filter((k) => inProgress(k.r)).sort((a, b) => a.r.at - b.r.at);
      for (const k of [...idle, ...reading]) {
        if (total <= CACHE_LIMIT) break;
        drop.push(k.path);
        total -= k.size;
      }
    }
    if (drop.length) {
      await native.dropCopies(drop);
      await refreshStorage();
    }
  })().finally(() => (tidying = null));
  return tidying;
}

/** Removes one copy (Storage); the record (where you were) stays. */
export async function dropCopy(path: string) {
  await native.dropCopies([path]);
  await refreshStorage();
}
