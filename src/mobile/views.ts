// The reader's views (the page viewer, or Read/Book's reflowed text), so sheets and search can move them.
import { signal } from "@preact/signals";
import type { Viewer } from "../render/Viewer";
import type { ReflowView } from "../reflow/ReadView";
import { mode, page } from "./state";

export const views: { viewer: Viewer | null; reflow: ReflowView | null } = { viewer: null, reflow: null };

/** Book mode's own page numbers (its pages aren't the document's). */
export const bookInfo = signal<{ page: number; total: number; approx: boolean } | null>(null);

let navAt = 0;
/** The view just moved because the app moved it (not a swipe): the bars stay as they are. */
export const movedByApp = () => performance.now() - navAt < 600;

/** Shows page `p` (0-based), at `y` points from its top when given. */
export function goTo(p: number, y?: number) {
  navAt = performance.now();
  page.value = p;
  if (mode.value === "pages") views.viewer?.goToPage(p, y);
  else views.reflow?.goToPage(p);
}
