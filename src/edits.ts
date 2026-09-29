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

/** Undo/redo history of snapshots. */
/** A history entry for an edit the engine undoes itself (form fields, annotations). */
export const DOC_STEP = "doc" as const;
type Entry = Edits | typeof DOC_STEP;

/**
 * One undo history for everything: page edits are snapshots kept here,
 * form and annotation edits are steps in the engine's journal (DOC_STEP).
 */
export class History {
  private undoStack: Entry[] = [];
  private redoStack: Entry[] = [];
  constructor(private limit = 200) {}

  push(prev: Entry) {
    this.undoStack.push(prev);
    if (this.undoStack.length > this.limit) this.undoStack.shift();
    this.redoStack = [];
  }
  /** The entry to go back to: page edits, or DOC_STEP (ask the engine to undo). */
  undo(cur: Edits): Entry | null {
    const prev = this.undoStack.pop();
    if (!prev) return null;
    this.redoStack.push(prev === DOC_STEP ? DOC_STEP : cur);
    return prev;
  }
  redo(cur: Edits): Entry | null {
    const next = this.redoStack.pop();
    if (!next) return null;
    this.undoStack.push(next === DOC_STEP ? DOC_STEP : cur);
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
}
