// The page grid: big thumbnails of the whole document in place of the pages,
// for organizing it. Select, drag to reorder (or onto another document's tab),
// rotate, delete, save as a new PDF, insert; double-click opens a page.
import { useLayoutEffect, useRef, useState } from "preact/hooks";
import { FilePlus2, FileOutput, RotateCcw, RotateCw, Trash2 } from "lucide-preact";
import * as A from "../actions";
import * as P from "../pages";
import { settings, updateSettings, type Tab } from "../store";
import { extractPages, startInsert } from "./PagesUI";
import { Thumbnails } from "./Sidebar";

const MIN = 120;
const MAX = 640;

export function toggleGrid(tab: Tab, on = !tab.grid.value) {
  tab.grid.value = on;
  if (!on) tab.viewer?.focus();
}

export function PageGrid({ tab }: { tab: Tab }) {
  const host = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(800);
  const size = settings.value.gridSize;
  const sel = tab.pageSel.value;
  const count = tab.edits.value?.order.length ?? 0;
  const k = sel.size > 1 ? sel.size : 1;
  const which = k > 1 ? `${k} pages` : "page";

  useLayoutEffect(() => {
    const el = host.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    setWidth(el.clientWidth);
    // Ctrl+wheel (and pinch) makes the pages bigger or smaller.
    const wheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      const cur = settings.value.gridSize;
      updateSettings({ gridSize: Math.round(Math.max(MIN, Math.min(MAX, cur * Math.exp(-e.deltaY / 400)))) });
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => {
      ro.disconnect();
      el.removeEventListener("wheel", wheel);
    };
  }, []);

  const open = (view: number) => {
    toggleGrid(tab, false);
    tab.pageSel.value = new Set([view]);
    A.goToPage(view);
  };

  return (
    <div class="page-grid">
      <div class="grid-bar">
        <span class="grid-count">
          {count} pages{sel.size > 1 ? ` · ${sel.size} selected` : ""}
        </span>
        <button class="ib" title={`Rotate ${which} left (Shift+R)`} aria-label="Rotate left" onClick={() => P.rotatePages(tab, -90)}>
          <RotateCcw size={16} />
        </button>
        <button class="ib" title={`Rotate ${which} right (R)`} aria-label="Rotate right" onClick={() => P.rotatePages(tab, 90)}>
          <RotateCw size={16} />
        </button>
        <button class="ib" title={`Delete ${which} (Del)`} aria-label="Delete" onClick={() => P.deletePages(tab)}>
          <Trash2 size={16} />
        </button>
        <button class="ib" title={`Save ${which} as a new PDF`} aria-label="Save as a new PDF" onClick={() => void extractPages(tab)}>
          <FileOutput size={16} />
        </button>
        <button class="ib" title="Insert pages from a file" aria-label="Insert pages" disabled={!tab.info.value?.isPdf} onClick={() => void startInsert(tab)}>
          <FilePlus2 size={16} />
        </button>
        <span class="grid-hint muted">Drag to reorder · double-click to open · Ctrl+A selects all</span>
        <input
          class="grid-size"
          type="range"
          min={MIN}
          max={MAX}
          value={size}
          title="Page size (Ctrl+wheel)"
          aria-label="Page size"
          onInput={(e) => updateSettings({ gridSize: +(e.target as HTMLInputElement).value })}
        />
        <button class="btn pri sm" onClick={() => toggleGrid(tab, false)} title="Back to the pages (Esc, Ctrl+Shift+G)">
          Done
        </button>
      </div>
      <div class="grid-body" ref={host}>
        <Thumbnails tab={tab} width={width} visible cell={size} maxBox={size * 1.3} where="grid" onOpen={open} />
      </div>
    </div>
  );
}
