// Shared render cache and request broker.
//
// Consumers (the page viewer, the thumbnail strip) call `sync(group, wants)`
// on every scroll/zoom update with the full list of bitmaps they want right
// now. The manager serves hits from cache, starts missing renders, joins
// duplicate requests, and cancels anything a group no longer wants — both
// the fetch and the native render, which is aborted mid-page.

import { api, fetchRender, newId, type RenderParams } from "../api";

export interface Bitmap {
  image: ImageData;
  width: number;
  height: number;
  /** Device pixels per point this bitmap was rendered at. */
  scale: number;
}

export interface Want {
  key: string;
  params: Omit<RenderParams, "id" | "prio">;
  prio: number;
  onReady: (b: Bitmap) => void;
}

class BitmapCache {
  private map = new Map<string, Bitmap>();
  private bytes = 0;
  constructor(public budget: number) {}

  get(key: string): Bitmap | undefined {
    const b = this.map.get(key);
    if (b) {
      this.map.delete(key);
      this.map.set(key, b);
    }
    return b;
  }

  peek(key: string): Bitmap | undefined {
    return this.map.get(key);
  }

  put(key: string, b: Bitmap) {
    const old = this.map.get(key);
    if (old) this.drop(key, old);
    this.map.set(key, b);
    this.bytes += b.width * b.height * 4;
    for (const [k, v] of this.map) {
      if (this.bytes <= this.budget) break;
      if (k === key) continue;
      this.drop(k, v);
    }
  }

  dropWhere(pred: (key: string) => boolean) {
    for (const [k, v] of [...this.map]) if (pred(k)) this.drop(k, v);
  }

  private drop(k: string, v: Bitmap) {
    this.map.delete(k);
    this.bytes -= v.width * v.height * 4;
  }

  get size() {
    return this.bytes;
  }
}

interface Flight {
  id: number;
  abort: AbortController;
  groups: Set<string>;
  listeners: Map<string, (b: Bitmap) => void>;
}

/** Two pools: small previews (cheap, reused by thumbnails) and everything else. */
class SplitCache {
  private lo: BitmapCache;
  private hi: BitmapCache;
  constructor(loBudget: number, hiBudget: number) {
    this.lo = new BitmapCache(loBudget);
    this.hi = new BitmapCache(hiBudget);
  }
  private pool(key: string) {
    return key.includes("|lo|") ? this.lo : this.hi;
  }
  get(key: string) {
    return this.pool(key).get(key);
  }
  put(key: string, b: Bitmap) {
    this.pool(key).put(key, b);
  }
  dropWhere(pred: (key: string) => boolean) {
    this.lo.dropWhere(pred);
    this.hi.dropWhere(pred);
  }
  get size() {
    return this.lo.size + this.hi.size;
  }
}

export class RenderManager {
  readonly cache: SplitCache;
  private flights = new Map<string, Flight>();
  private groups = new Map<string, Set<string>>();
  private failed = new Map<string, number>();
  private maxInFlight = 48;

  constructor(loBudget: number, hiBudget: number) {
    this.cache = new SplitCache(loBudget, hiBudget);
  }

  get pending() {
    return this.flights.size;
  }

  sync(group: string, wants: Want[]) {
    const keys = new Set(wants.map((w) => w.key));
    const toCancel: number[] = [];
    for (const k of this.groups.get(group) ?? []) {
      if (keys.has(k)) continue;
      const f = this.flights.get(k);
      if (!f) continue;
      f.groups.delete(group);
      f.listeners.delete(group);
      if (f.groups.size === 0) {
        this.flights.delete(k);
        f.abort.abort();
        toCancel.push(f.id);
      }
    }
    this.groups.set(group, keys);

    const now = performance.now();
    // Highest priority first so cache hits paint before new work starts.
    const sorted = [...wants].sort((a, b) => b.prio - a.prio);
    for (const w of sorted) {
      const hit = this.cache.get(w.key);
      if (hit) {
        w.onReady(hit);
        continue;
      }
      const f = this.flights.get(w.key);
      if (f) {
        f.groups.add(group);
        f.listeners.set(group, w.onReady);
        continue;
      }
      const failedAt = this.failed.get(w.key);
      if (failedAt && now - failedAt < 5000) continue;
      if (this.flights.size >= this.maxInFlight) continue;
      this.start(group, w);
    }
    if (toCancel.length) api.cancel(toCancel).catch(() => {});
  }

  /** Stops everything a group asked for (e.g. a closed tab). */
  release(group: string) {
    this.sync(group, []);
    this.groups.delete(group);
  }

  /** Forgets cached bitmaps whose key starts with `prefix` (e.g. a closed document). */
  forget(prefix: string) {
    this.cache.dropWhere((k) => k.startsWith(prefix));
  }

  private start(group: string, w: Want) {
    const id = newId();
    const abort = new AbortController();
    const flight: Flight = { id, abort, groups: new Set([group]), listeners: new Map([[group, w.onReady]]) };
    this.flights.set(w.key, flight);
    fetchRender({ ...w.params, id, prio: w.prio }, abort.signal)
      .then((r) => {
        if (this.flights.get(w.key) !== flight) return;
        this.flights.delete(w.key);
        if (!r) return;
        const bmp: Bitmap = { ...r, scale: w.params.scale };
        this.cache.put(w.key, bmp);
        for (const fn of flight.listeners.values()) fn(bmp);
      })
      .catch((err) => {
        if (this.flights.get(w.key) === flight) this.flights.delete(w.key);
        if (err?.name !== "AbortError") {
          this.failed.set(w.key, performance.now());
          console.warn("render failed", w.key, err);
        }
      });
  }
}

/** One manager for the whole app. Sharp pages re-render in ~50–100 ms, so their pool stays small. */
// Phones have far less memory to spare than desktops.
const phone = /Android/.test(navigator.userAgent);
export const renderer = phone ? new RenderManager(20 * 1024 * 1024, 36 * 1024 * 1024) : new RenderManager(48 * 1024 * 1024, 64 * 1024 * 1024);

/** Cache key for a rendered bitmap. Shared by the viewer and thumbnails so they reuse each other's work. */
/** Content revision per document: bumped when fields or annotations change, so old renders miss. */
const revs = new Map<number, number>();

export function renderKey(doc: number, page: number, rotate: number, kind: string, scale: number, extra = "") {
  return `${doc}.${revs.get(doc) ?? 0}|${page}|${rotate}|${kind}|${scale.toFixed(3)}${extra}`;
}

/** The document's pages changed in memory (form values, annotations): render them again. */
export function bumpRev(doc: number) {
  const old = revs.get(doc) ?? 0;
  revs.set(doc, old + 1);
  renderer.forget(`${doc}.${old}|`);
}
