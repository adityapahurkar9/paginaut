// Unsaved page edits for one tab: page order and extra rotation per original
// page. Edits are plain arrays, so undo/redo is just a stack of snapshots, and
// saving hands both arrays to the engine in one go.
import type { PageSpec } from "./render/Viewer";

export interface Edits {
  /** Original page indices in display order (deleted pages are absent). */
  order: number[];
  /** Extra rotation in degrees (0/90/180/270), indexed by original page. */
  rot: number[];
}

export const identity = (n: number): Edits => ({ order: Array.from({ length: n }, (_, i) => i), rot: new Array(n).fill(0) });

export function isIdentity(e: Edits): boolean {
  if (e.order.length !== e.rot.length) return false;
  for (let i = 0; i < e.order.length; i++) if (e.order[i] !== i) return false;
  return e.rot.every((r) => r === 0);
}

/** True when the only changes are rotations (order untouched). */
export function onlyRotations(e: Edits): boolean {
  return e.order.length === e.rot.length && e.order.every((p, i) => p === i);
}

export function specs(e: Edits, sizes: [number, number][]): PageSpec[] {
  return e.order.map((src) => {
    const r = e.rot[src] ?? 0;
    const [w, h] = sizes[src] ?? sizes[0] ?? [612, 792];
    return r % 180 ? { src, rot: r, w: h, h: w } : { src, rot: r, w, h };
  });
}

export function rotate(e: Edits, views: number[], by: number): Edits {
  const rot = e.rot.slice();
  for (const v of views) {
    const src = e.order[v];
    if (src !== undefined) rot[src] = (((rot[src] + by) % 360) + 360) % 360;
  }
  return { order: e.order, rot };
}

export function remove(e: Edits, views: number[]): Edits {
  const drop = new Set(views);
  return { order: e.order.filter((_, i) => !drop.has(i)), rot: e.rot };
}

/** Moves the pages at `views` so they start at position `to` (a gap index in the old order). */
export function move(e: Edits, views: number[], to: number): Edits {
  const set = new Set(views);
  const moving = e.order.filter((_, i) => set.has(i));
  const before = e.order.slice(0, to).filter((_, i) => !set.has(i));
  const after = e.order.slice(to).filter((_, i) => !set.has(i + to));
  return { order: [...before, ...moving, ...after], rot: e.rot };
}

/** A history entry for an edit the engine undoes itself (form fields, annotations). */
export const DOC_STEP = "doc" as const;

/** Records a change to the document that doesn't touch how pages look (bookmarks, labels,
 *  properties): one undo step, unsaved, nothing redrawn. */
export function metaChanged(tab: { history: { push(e: typeof DOC_STEP): void }; historyVersion: { value: number }; docChanged: { value: boolean } }) {
  tab.history.push(DOC_STEP);
  tab.historyVersion.value++;
  tab.docChanged.value = true;
}
/**
 * An engine step that also changed the page list (pages inserted): undoing it
 * undoes the engine step and goes back to these page edits.
 */
export interface DocWithEdits {
  doc: Edits;
  /** Pages whose size the step changed (their sizes are fetched again after undo/redo). */
  resized?: number[];
}
type Entry = Edits | typeof DOC_STEP | DocWithEdits;

export const withDoc = (e: Entry): e is DocWithEdits => typeof e === "object" && "doc" in e;

/** Pages in the document itself (including pages inserted but not saved yet). */
export const sourceCount = (e: Edits) => e.rot.length;

/** Undo/redo history of snapshots. */

/**
 * One undo history for everything: page edits are snapshots kept here,
 * form and annotation edits are steps in the engine's journal (DOC_STEP).
 */
/** What takes an entry's place on the other stack once it's been applied. */
const counterpart = (e: Entry, cur: Edits): Entry => (e === DOC_STEP ? DOC_STEP : withDoc(e) ? { doc: cur, resized: e.resized } : cur);

export class History {
  private undoStack: Entry[] = [];
  private redoStack: Entry[] = [];
  constructor(private limit = 200) {}

  push(prev: Entry) {
    this.undoStack.push(prev);
    if (this.undoStack.length > this.limit) this.undoStack.shift();
    this.redoStack = [];
  }
  /** The entry to go back to: page edits, DOC_STEP (ask the engine to undo), or both. */
  undo(cur: Edits): Entry | null {
    const prev = this.undoStack.pop();
    if (!prev) return null;
    this.redoStack.push(counterpart(prev, cur));
    return prev;
  }
  redo(cur: Edits): Entry | null {
    const next = this.redoStack.pop();
    if (!next) return null;
    this.undoStack.push(counterpart(next, cur));
    return next;
  }
  clear() {
    this.undoStack = [];
    this.redoStack = [];
  }
  get canUndo() {
    return this.undoStack.length > 0;
  }
  get canRedo() {
    return this.redoStack.length > 0;
  }
  /** Both stacks as plain data (a tab moving to another window takes its history along). */
  toJSON(): HistoryData {
    return { undo: this.undoStack, redo: this.redoStack };
  }
  static from(data: HistoryData): History {
    const h = new History();
    h.undoStack = data.undo.slice();
    h.redoStack = data.redo.slice();
    return h;
  }
}

export interface HistoryData {
  undo: Entry[];
  redo: Entry[];
}
