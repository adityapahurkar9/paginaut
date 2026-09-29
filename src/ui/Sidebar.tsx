import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import { bookScreen } from "../modes";
import { selectWhenShown } from "../annots";
import * as A from "../actions";
import { ChevronDown, ChevronRight, Plus } from "lucide-preact";
import { api, type OutlineItem } from "../api";
import { renderer, renderKey, type Want } from "../render/RenderManager";
import { LO_SCALE, paint } from "../render/Viewer";
import { signal } from "@preact/signals";
import { activeKey, activeTab, settings, showToast, tabs, updateSettings, type Tab } from "../store";
import { metaChanged, specs } from "../edits";
import * as P from "../pages";
import { openMenuAt } from "./Toolbar";
import { extractPages, insertBlankPage, startInsert } from "./PagesUI";
import { startPageSize } from "../pagesize";

// ---------------------------------------------------------------- dragging pages

/**
 * Thumbnails being dragged: within their document (to reorder) or onto another
 * open document (to copy them there). Hovering over another document's tab
 * switches to it, so the pages can be dropped among its thumbnails.
 */
export const pageDrag = signal<{ from: Tab; views: number[]; x: number; y: number; target: Tab | null; gap: number; hover: Tab | null; into: HTMLElement | null } | null>(null);

/** Each shown set of thumbnails (sidebar, page grid): the gap a point is at (null if outside) and its scroller. */
type DropTarget = { tab: Tab; gapAt: (x: number, y: number) => number | null; scroller: HTMLElement };
const dropTargets = new Set<DropTarget>();

function startPageDrag(from: Tab, views: number[], e: PointerEvent, onReorderClickGuard: () => void) {
  let autoTimer = 0;
  let switchTimer = 0;
  let switchTo: Tab | null = null;
  const update = (ev: PointerEvent) => {
    const el = document.elementFromPoint(ev.clientX, ev.clientY)?.closest("[data-tab]") as HTMLElement | null;
    const hover = el ? (tabs.value.find((t) => t.key === el.dataset.tab) ?? null) : null;
    // Hovering over another tab for a moment opens it.
    if (hover !== switchTo) {
      clearTimeout(switchTimer);
      switchTo = hover;
      if (hover && hover !== activeTab.value) switchTimer = window.setTimeout(() => (activeKey.value = hover.key), 550);
    }
    const act = activeTab.value;
    let t: DropTarget | undefined;
    let gap: number | null = null;
    for (const d of dropTargets) {
      if (d.tab !== act) continue;
      gap = d.gapAt(ev.clientX, ev.clientY);
      if (gap !== null) {
        t = d;
        break;
      }
    }
    pageDrag.value = { from, views, x: ev.clientX, y: ev.clientY, target: gap === null ? null : act, gap: gap ?? 0, hover, into: t?.scroller ?? null };
    // Scroll the strip when dragging near its edges.
    clearInterval(autoTimer);
    if (t && gap !== null) {
      const b = t.scroller.getBoundingClientRect();
      const dir = ev.clientY < b.top + 36 ? -1 : ev.clientY > b.bottom - 36 ? 1 : 0;
      if (dir) autoTimer = window.setInterval(() => (t.scroller.scrollTop += dir * 14), 16);
    }
  };
  const up = () => {
    window.removeEventListener("pointermove", update);
    window.removeEventListener("pointerup", up);
    clearInterval(autoTimer);
    clearTimeout(switchTimer);
    const d = pageDrag.value;
    pageDrag.value = null;
    onReorderClickGuard();
    if (!d) return;
    if (d.target === from) P.movePages(from, views, d.gap);
    else if (d.target) void copyPages(from, views, d.target, d.gap);
    else if (d.hover && d.hover !== from) void copyPages(from, views, d.hover, d.hover.edits.value?.order.length ?? 0);
  };
  update(e);
  window.addEventListener("pointermove", update);
  window.addEventListener("pointerup", up);
}

/** Copies pages (display indices of `from`) into `to` at a gap, as shown in `from` (rotations too). */
async function copyPages(from: Tab, views: number[], to: Tab, at: number) {
  const e = from.edits.value;
  const info = from.info.value;
  if (!e || !info) return;
  if (!to.info.value?.isPdf) return showToast(`Pages can only be added to a PDF. Save ${to.name} as a PDF first (Save as).`, 4500);
  const pages = views.map((v) => e.order[v]);
  try {
    const r = await P.insertPages(to, at, { fromDoc: info.docId, pages, rot: pages.map((p) => e.rot[p] ?? 0) });
    activeKey.value = to.key;
    showToast(`Copied ${r.count === 1 ? "1 page" : `${r.count} pages`} from ${from.name} · Ctrl+Z to undo`, 3500);
  } catch (err) {
    showToast(`Couldn't copy the pages: ${err}`, 5000);
  }
}

/** The page being dragged, following the pointer (in every view, so it survives switching tabs). */
export function PageDragGhost() {
  const d = pageDrag.value;
  if (!d) return null;
  const what = d.views.length === 1 ? `Page ${d.views[0] + 1}` : `${d.views.length} pages`;
  const into = d.target && d.target !== d.from ? d.target : !d.target && d.hover && d.hover !== d.from ? d.hover : null;
  return (
    <div class="drag-ghost" style={{ left: d.x + 12, top: d.y + 12 }}>
      {into ? `${what} · copy to ${into.name}` : what}
    </div>
  );
}

const MIN_W = 160;
const MAX_W = 520;

export function Sidebar() {
  const s = settings.value;
  const [width, setWidth] = useState(s.sidebarWidth);
  const [dragging, setDragging] = useState(false);
  const tab = activeTab.value;
  useEffect(() => setWidth(s.sidebarWidth), [s.sidebarWidth]);

  const startDrag = (e: PointerEvent) => {
    e.preventDefault();
    const x0 = e.clientX;
    const w0 = width;
    let w = w0;
    setDragging(true);
    const move = (ev: PointerEvent) => {
      w = Math.max(MIN_W, Math.min(MAX_W, w0 + ev.clientX - x0));
      setWidth(w);
    };
    const up = () => {
      setDragging(false);
      updateSettings({ sidebarWidth: w });
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const open = s.sidebarOpen && !!tab && !bookScreen.value;
  return (
    <div class={`sidebar ${open ? "" : "closed"} ${dragging ? "dragging" : ""}`} style={{ width: open ? width : 0 }}>
      <div class="sidebar-inner" style={{ width }}>
        <div style={{ padding: "10px 10px 6px" }}>
          <div class="seg">
            <button class={`segb ${s.sidebarTab === "pages" ? "on" : ""}`} onClick={() => updateSettings({ sidebarTab: "pages" })}>
              Pages
            </button>
            <button class={`segb ${s.sidebarTab === "comments" ? "on" : ""}`} onClick={() => updateSettings({ sidebarTab: "comments" })}>
              Comments
            </button>
            <button class={`segb ${s.sidebarTab === "outline" ? "on" : ""}`} onClick={() => updateSettings({ sidebarTab: "outline" })}>
              Outline
            </button>
          </div>
        </div>
        {tab && tab.status.value === "ready" && (
          s.sidebarTab === "pages" ? <Thumbnails key={tab.key} tab={tab} width={width} visible={open} />
          : s.sidebarTab === "comments" ? <Comments key={tab.key} tab={tab} />
          : <Outline key={tab.key} tab={tab} />
        )}
      </div>
      {open && <div class={`resizer ${dragging ? "on" : ""}`} onPointerDown={startDrag} />}
    </div>
  );
}

// ---------------------------------------------------------------- thumbnails

const PAD = 8;
const LABEL = 22;

/**
 * Page thumbnails: the sidebar strip, or the page grid (bigger cells, `cell`
 * px wide at least). Click to select (Ctrl/Shift for more), drag to reorder or
 * into another document, right-click for page actions; `onOpen` on double-click.
 */
export function Thumbnails({ tab, width, visible, cell = 108, maxBox = 360, where = "thumbs", onOpen }: { tab: Tab; width: number; visible: boolean; cell?: number; maxBox?: number; where?: string; onOpen?: (view: number) => void }) {
  const scroller = useRef<HTMLDivElement>(null);
  const grid = useRef<HTMLDivElement>(null);
  const [scroll, setScroll] = useState({ top: 0, height: 600 });
  const canvases = useRef(new Map<number, HTMLCanvasElement>());
  const userScrolled = useRef(0);
  const anchor = useRef(0);
  const suppressClick = useRef(false);
  const pd = pageDrag.value;
  const drag = pd && pd.into && pd.into === scroller.current ? pd : null;
  const info = tab.info.value!;
  const sizes = tab.sizes.value;
  const edits = tab.edits.value;
  const current = tab.page.value;
  const sel = tab.pageSel.value;
  const group = `${where}:${info.docId}`;
  const pages = useMemo(
    () => (edits ? specs(edits, sizes ?? Array.from({ length: info.pageCount }, () => info.firstSize)) : []),
    [edits, sizes],
  );
  const count = pages.length;

  // Grid geometry. Columns step up as the panel widens; within a step, thumbnails grow with it.
  const inner = width - PAD * 2;
  const cols = Math.max(1, Math.floor((inner + 10) / cell));
  const cellW = inner / cols;
  const boxW = Math.max(60, Math.min(cellW - 14, maxBox));
  const boxH = boxW * 1.3;
  const rowH = boxH + LABEL + 12;
  const rows = Math.ceil(count / cols);
  const firstRow = Math.max(0, Math.floor(scroll.top / rowH) - 1);
  const lastRow = Math.min(rows - 1, Math.ceil((scroll.top + scroll.height) / rowH) + 1);

  const fitBox = (i: number) => {
    const { w, h } = pages[i];
    const k = Math.min(boxW / w, boxH / h);
    return { w: Math.round(w * k), h: Math.round(h * k), k };
  };

  /** Render scale and cache key for thumbnail i. Small ones reuse the viewer's low-res pass. */
  const thumbRender = (i: number) => {
    const spec = pages[i];
    const needed = fitBox(i).k * (window.devicePixelRatio || 1); // device px per point
    const scale = needed <= LO_SCALE * 1.15 ? LO_SCALE : Math.ceil(needed * 20) / 20;
    return { scale, key: renderKey(info.docId, spec.src, spec.rot, scale === LO_SCALE ? 'lo' : 'thumb', scale) };
  };

  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const onScroll = () => {
      userScrolled.current = performance.now();
      setScroll({ top: el.scrollTop, height: el.clientHeight });
    };
    const ro = new ResizeObserver(() => setScroll({ top: el.scrollTop, height: el.clientHeight }));
    ro.observe(el);
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      ro.disconnect();
      el.removeEventListener("scroll", onScroll);
      renderer.release(group);
    };
  }, []);

  // Keep the current page in view, unless the user is scrolling the strip.
  useEffect(() => {
    const el = scroller.current;
    if (!el || drag || performance.now() - userScrolled.current < 1500) return;
    const y = Math.floor(current / cols) * rowH;
    if (y < el.scrollTop || y + rowH > el.scrollTop + el.clientHeight) {
      el.scrollTo({ top: y - el.clientHeight / 2 + rowH / 2, behavior: "smooth" });
    }
  }, [current, cols, rowH]);

  // Request bitmaps for visible thumbnails (low priority, below the page view).
  // contentRev: filled-in fields changed the pages.
  const rev = tab.contentRev.value;
  useEffect(() => {
    if (!visible) {
      renderer.sync(group, []);
      return;
    }
    const wants: Want[] = [];
    for (let r = firstRow; r <= lastRow; r++) {
      for (let c = 0; c < cols; c++) {
        const i = r * cols + c;
        if (i >= count) break;
        const spec = pages[i];
        const { scale, key } = thumbRender(i);
        wants.push({
          key,
          params: { doc: info.docId, page: spec.src, scale, rotate: spec.rot },
          prio: 300 - Math.abs(i - current) / 100,
          onReady: (b) => {
            const cv = canvases.current.get(i);
            if (!cv || cv.dataset.key !== key) return;
            cv.width = b.width;
            cv.height = b.height;
            paint(cv, b);
          },
        });
      }
    }
    renderer.sync(group, wants);
  }, [firstRow, lastRow, cols, boxW, visible, pages, rev]);

  // ------------------------------------------------ selection and drag to reorder

  const select = (i: number, e: MouseEvent) => {
    if (e.ctrlKey) {
      const next = new Set(sel);
      if (next.has(i)) next.delete(i);
      else next.add(i);
      tab.pageSel.value = next;
    } else if (e.shiftKey) {
      const [a, b] = [Math.min(anchor.current, i), Math.max(anchor.current, i)];
      tab.pageSel.value = new Set(Array.from({ length: b - a + 1 }, (_, k) => a + k));
      return;
    } else {
      tab.pageSel.value = new Set([i]);
      A.goToPage(i);
    }
    anchor.current = i;
  };

  /** Gap index (0..count) nearest to a point in the grid. */
  const gapAt = (clientX: number, clientY: number) => {
    const r = grid.current!.getBoundingClientRect();
    const x = clientX - r.left - PAD, y = clientY - r.top;
    const row = Math.max(0, Math.min(rows - 1, Math.floor(y / rowH)));
    const col = Math.max(0, Math.min(cols - 1, Math.floor(x / cellW)));
    const right = x - col * cellW > cellW / 2;
    return Math.max(0, Math.min(count, row * cols + col + (right ? 1 : 0)));
  };

  // Where dragged pages would land here (from this document or another one).
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const target: DropTarget = {
      tab,
      scroller: el,
      gapAt: (x, y) => {
        const b = el.getBoundingClientRect();
        return x >= b.left && x <= b.right && y >= b.top && y <= b.bottom && el.offsetParent ? gapAt(x, y) : null;
      },
    };
    dropTargets.add(target);
    return () => void dropTargets.delete(target);
  });

  const startDrag = (i: number, e: PointerEvent) => {
    if (e.button !== 0) return;
    const sx = e.clientX, sy = e.clientY;
    const wait = (ev: PointerEvent) => {
      if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return;
      stop();
      const views = sel.has(i) ? [...sel].sort((a, b) => a - b) : [i];
      if (!sel.has(i)) tab.pageSel.value = new Set([i]);
      startPageDrag(tab, views, ev, () => {
        // The click that ends a drag is ignored; it only reaches the thumbnail when released on it.
        suppressClick.current = true;
        setTimeout(() => (suppressClick.current = false), 0);
      });
    };
    const stop = () => {
      window.removeEventListener("pointermove", wait);
      window.removeEventListener("pointerup", stop);
    };
    window.addEventListener("pointermove", wait);
    window.addEventListener("pointerup", stop);
  };

  /** Right-click: page actions for this page, or the selection it's part of. */
  const pageMenu = (i: number, e: MouseEvent) => {
    e.preventDefault();
    if (!sel.has(i)) tab.pageSel.value = new Set([i]);
    A.goToPage(i);
    const k = sel.has(i) ? sel.size : 1;
    const many = k > 1;
    const isPdf = !!tab.info.value?.isPdf;
    openMenuAt(
      e.clientX,
      e.clientY,
      [
        { label: "Rotate right", hint: "R", run: () => P.rotatePages(tab, 90) },
        { label: "Rotate left", hint: "Shift+R", run: () => P.rotatePages(tab, -90) },
        { label: many ? `Delete ${k} pages` : "Delete page", hint: "Del", run: () => P.deletePages(tab) },
        { sep: true },
        { label: many ? `Save ${k} pages as a new PDF…` : "Save page as a new PDF…", run: () => void extractPages(tab) },
        { label: many ? `Crop or resize ${k} pages…` : "Crop or resize…", disabled: !isPdf, run: () => startPageSize(tab) },
        { sep: true },
        { label: "Insert pages from a file…", disabled: !isPdf, run: () => void startInsert(tab) },
        { label: "Insert blank page after", disabled: !isPdf, run: () => void insertBlankPage(tab) },
      ],
      240,
    );
  };

  const cells = [];
  for (let r = firstRow; r <= lastRow; r++) {
    for (let c = 0; c < cols; c++) {
      const i = r * cols + c;
      if (i >= count) break;
      const spec = pages[i];
      const b = fitBox(i);
      const dragged = pd?.from === tab && pd.views.includes(i);
      cells.push(
        <button
          key={`${i}:${spec.src}:${spec.rot}`}
          class={`thumb ${i === current ? "on" : ""} ${sel.has(i) ? "sel" : ""} ${dragged ? "dragged" : ""}`}
          style={{ left: PAD + c * cellW, top: r * rowH + 6, width: cellW }}
          onPointerDown={(e) => startDrag(i, e)}
          onContextMenu={(e) => pageMenu(i, e)}
          onDblClick={onOpen && (() => onOpen(i))}
          onClick={(e) => {
            if (suppressClick.current) {
              suppressClick.current = false;
              return;
            }
            select(i, e);
          }}
          aria-label={`Page ${i + 1}${spec.src !== i ? ` (was page ${spec.src + 1})` : ""}`}
          title={spec.src !== i ? `Originally page ${spec.src + 1}` : undefined}
        >
          <div style={{ height: boxH, display: "flex", alignItems: "center" }}>
            <div class="tp" style={{ width: b.w, height: b.h }}>
              <canvas
                data-key={thumbRender(i).key}
                ref={(el) => {
                  if (el) canvases.current.set(i, el);
                  else canvases.current.delete(i);
                }}
              />
            </div>
          </div>
          <span>{tab.labels.value ? tab.labels.value[spec.src] || i + 1 : i + 1}</span>
        </button>,
      );
    }
  }

  // Drop marker between thumbnails.
  let marker = null;
  if (drag) {
    const g = drag.gap;
    const atRowEnd = g > 0 && g % cols === 0;
    const row = Math.floor((atRowEnd ? g - 1 : g) / cols);
    const col = atRowEnd ? cols : g % cols;
    marker = <div class="drop-marker" style={{ left: PAD + col * cellW - 2, top: row * rowH + 8, height: boxH + 4 }} />;
  }

  return (
    <div class="side-scroll" ref={scroller}>
      <div class="thumbs" ref={grid} style={{ height: rows * rowH + 12 }}>
        {cells}
        {marker}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- outline

// ---------------------------------------------------------------- comments list

const KIND_LABEL: Record<string, string> = {
  Highlight: "Highlight", Underline: "Underline", StrikeOut: "Strike-through", Squiggly: "Squiggly", Ink: "Drawing", Text: "Note",
  FreeText: "Text box", Square: "Rectangle", Circle: "Ellipse", Line: "Line", Stamp: "Stamp", Polygon: "Polygon", PolyLine: "Lines",
  Caret: "Insert", FileAttachment: "Attachment",
};

function Comments({ tab }: { tab: Tab }) {
  const [list, setList] = useState<{ p: number; n: number; t: string; text: string; author: string; date: number }[] | null>(null);
  const rev = tab.contentRev.value;
  const info = tab.info.value;
  useEffect(() => {
    if (!info?.isPdf) return setList([]);
    let alive = true;
    api.docAnnots(info.docId).then((l) => alive && setList(l)).catch(() => alive && setList([]));
    return () => {
      alive = false;
    };
  }, [info?.docId, rev]);
  if (!list) return <div class="empty-note">Loading…</div>;
  if (!list.length)
    return <div class="empty-note">No comments yet. Use the Comment tab to highlight, draw, or add notes and text.</div>;
  const order = tab.edits.value?.order ?? [];
  const view = (src: number) => order.indexOf(src);
  const sorted = [...list].filter((c) => view(c.p) >= 0).sort((a, b) => view(a.p) - view(b.p));
  return (
    <div class="comments">
      {sorted.map((c) => (
        <button
          key={c.n}
          class="comment-row"
          onClick={() => {
            A.goToPage(view(c.p));
            selectWhenShown(tab, c.p, c.n);
          }}
        >
          <span class="c-head">
            <strong>{KIND_LABEL[c.t] ?? c.t}</strong>
            <span>p. {view(c.p) + 1}</span>
          </span>
          {c.text && <span class="c-text">{c.text}</span>}
          <span class="c-meta">{[c.author, c.date > 0 ? new Date(c.date * 1000).toLocaleString() : ""].filter(Boolean).join(" · ")}</span>
        </button>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------- outline (bookmarks), editable

type Path = number[];

const clone = (t: OutlineItem[]): OutlineItem[] => t.map((n) => ({ ...n, k: n.k ? clone(n.k) : undefined }));
const kidsOf = (tree: OutlineItem[], parent: Path): OutlineItem[] => {
  let list = tree;
  for (const i of parent) {
    const n = list[i];
    n.k = n.k ?? [];
    list = n.k;
  }
  return list;
};
const nodeAt = (tree: OutlineItem[], path: Path) => kidsOf(tree, path.slice(0, -1))[path[path.length - 1]];
const samePath = (a: Path | null, b: Path) => !!a && a.length === b.length && a.every((v, i) => v === b[i]);
const isInside = (inner: Path, outer: Path) => inner.length > outer.length && outer.every((v, i) => v === inner[i]);

/** Writes the bookmarks (one undo step) and shows them as the document has them now. */
async function saveOutline(tab: Tab, tree: OutlineItem[]) {
  const info = tab.info.value;
  if (!info) return;
  try {
    await api.setOutline(info.docId, tree);
    tab.outline.value = await api.outline(info.docId);
    metaChanged(tab);
  } catch (e) {
    showToast(`Couldn't change the bookmarks: ${e}`, 5000);
  }
}

/** How a page (document index) is shown: its label if the file has them, else its position; "–" once deleted. */
const pageName = (tab: Tab, src: number) => {
  const at = tab.edits.value?.order.indexOf(src) ?? src;
  return at < 0 ? "–" : tab.labels.value?.[src] || String(at + 1);
};

/** Where the view is: the current page (document index). */
const here = (tab: Tab) => tab.edits.value?.order[tab.page.value] ?? tab.page.value;

/** A new bookmark for the page you're on, titled with the selected text if any. */
async function newBookmark(tab: Tab): Promise<OutlineItem> {
  const sel = (await tab.viewer?.copySelection().catch(() => null))?.replace(/\s+/g, " ").trim();
  return { t: sel && sel.length <= 120 ? sel : `Page ${tab.page.value + 1}`, p: here(tab), y: 0 };
}

/** Adds a bookmark for the page you're on at the end of the list, and starts renaming it. */
export async function addBookmarkHere(tab: Tab) {
  const tree = clone(tab.outline.value ?? []);
  tree.push(await newBookmark(tab));
  await saveOutline(tab, tree);
  renaming.value = { tab, path: [tree.length - 1] };
  updateSettings({ sidebarOpen: true, sidebarTab: "outline" });
}

type Drop = { path: Path; where: "before" | "after" | "inside" };
const renaming = signal<{ tab: Tab; path: Path } | null>(null);

function Outline({ tab }: { tab: Tab }) {
  const items = tab.outline.value;
  const [drop, setDrop] = useState<Drop | null>(null);
  const [dragging, setDragging] = useState<Path | null>(null);
  const editable = !!tab.info.value?.isPdf;
  if (items === null) return <div class="empty-note">Loading…</div>;

  const add = async (inside: Path | null) => {
    const tree = clone(items);
    const b = await newBookmark(tab);
    let path: Path;
    if (inside) {
      const list = kidsOf(tree, inside);
      list.push(b);
      path = [...inside, list.length - 1];
    } else {
      tree.push(b);
      path = [tree.length - 1];
    }
    await saveOutline(tab, tree);
    renaming.value = { tab, path };
  };

  const move = (from: Path, d: Drop) => {
    if (samePath(from, d.path) || isInside(d.path, from)) return;
    const tree = clone(items);
    const node = nodeAt(tree, from);
    // Mark, take out, then put in at the target (found again by identity, since indices shift).
    const target = nodeAt(tree, d.path);
    const fromList = kidsOf(tree, from.slice(0, -1));
    fromList.splice(from[from.length - 1], 1);
    const find = (list: OutlineItem[], parent: Path): Path | null => {
      for (let i = 0; i < list.length; i++) {
        if (list[i] === target) return [...parent, i];
        const inner = list[i].k ? find(list[i].k!, [...parent, i]) : null;
        if (inner) return inner;
      }
      return null;
    };
    const at = find(tree, [])!;
    if (d.where === "inside") kidsOf(tree, at).push(node);
    else kidsOf(tree, at.slice(0, -1)).splice(at[at.length - 1] + (d.where === "after" ? 1 : 0), 0, node);
    void saveOutline(tab, tree);
  };

  const startDrag = (path: Path, e: PointerEvent) => {
    if (!editable || e.button !== 0 || renaming.value) return;
    const sx = e.clientX, sy = e.clientY;
    let active = false;
    let target: Drop | null = null;
    const onMove = (ev: PointerEvent) => {
      if (!active) {
        if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return;
        active = true;
        setDragging(path);
      }
      const row = (document.elementFromPoint(ev.clientX, ev.clientY) as HTMLElement | null)?.closest<HTMLElement>("[data-opath]");
      if (!row) return setDrop((target = null));
      const p = row.dataset.opath!.split(".").map(Number);
      const r = row.getBoundingClientRect();
      const f = (ev.clientY - r.top) / r.height;
      target = { path: p, where: f < 0.3 ? "before" : f > 0.7 ? "after" : "inside" };
      setDrop(target);
    };
    const up = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", up);
      if (!active) return;
      setDragging(null);
      setDrop(null);
      // The click that ends the drag must not jump to the bookmark.
      suppress.current = true;
      setTimeout(() => (suppress.current = false), 0);
      if (target) move(path, target);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", up);
  };
  const suppress = useRef(false);

  const menu = (path: Path, e: MouseEvent) => {
    e.preventDefault();
    if (!editable) return;
    const tree = clone(items);
    const node = nodeAt(tree, path);
    const siblings = kidsOf(tree, path.slice(0, -1));
    const i = path[path.length - 1];
    openMenuAt(e.clientX, e.clientY, [
      { label: "Rename", hint: "F2", run: () => (renaming.value = { tab, path }) },
      { label: `Point to this page (${pageName(tab, here(tab))})`, run: () => { node.p = here(tab); node.y = 0; void saveOutline(tab, tree); } },
      { label: "Add a bookmark inside", run: () => void add(path) },
      { sep: true },
      {
        label: "Move in (under the one above)",
        disabled: i === 0,
        run: () => {
          siblings.splice(i, 1);
          const prev = siblings[i - 1];
          (prev.k = prev.k ?? []).push(node);
          void saveOutline(tab, tree);
        },
      },
      {
        label: "Move out",
        disabled: path.length < 2,
        run: () => {
          siblings.splice(i, 1);
          const parentPath = path.slice(0, -1);
          kidsOf(tree, parentPath.slice(0, -1)).splice(parentPath[parentPath.length - 1] + 1, 0, node);
          void saveOutline(tab, tree);
        },
      },
      { sep: true },
      { label: node.k?.length ? "Delete (with the ones inside)" : "Delete", run: () => { siblings.splice(i, 1); void saveOutline(tab, tree); } },
    ], 250);
  };

  const rename = (path: Path, title: string) => {
    // Enter commits, and the input's blur as it goes away would commit again.
    if (!renaming.value) return;
    renaming.value = null;
    const tree = clone(items);
    const node = nodeAt(tree, path);
    if (!title.trim() || title === node.t) return;
    node.t = title.trim();
    void saveOutline(tab, tree);
  };

  return (
    <div class="side-scroll" style={{ padding: "2px 6px 12px" }}>
      {editable && (
        <div class="outline-bar" title={items.length ? "Drag bookmarks to reorder or nest them · right-click one for more · F2 renames" : undefined}>
          <button class="btn ghost sm" onClick={() => void add(null)} title="A bookmark for the page you're on (selected text becomes its title)">
            <Plus size={14} /> Bookmark
          </button>
        </div>
      )}
      {!items.length && <div class="empty-note">This document has no bookmarks. {editable ? "Add one for the page you're on with + Bookmark." : "Scanned files usually don't."}</div>}
      {items.map((it, i) => (
        <OutlineNode
          key={i}
          item={it}
          path={[i]}
          tab={tab}
          drop={drop}
          dragging={dragging}
          onDrag={startDrag}
          onMenu={menu}
          onRename={rename}
          suppress={suppress}
        />
      ))}
    </div>
  );
}

function OutlineNode(props: {
  item: OutlineItem;
  path: Path;
  tab: Tab;
  drop: Drop | null;
  dragging: Path | null;
  onDrag: (path: Path, e: PointerEvent) => void;
  onMenu: (path: Path, e: MouseEvent) => void;
  onRename: (path: Path, title: string) => void;
  suppress: { current: boolean };
}) {
  const { item, path, tab, drop } = props;
  const depth = path.length - 1;
  const [open, setOpen] = useState(depth === 0 && (item.k?.length ?? 0) < 12);
  const kids = item.k ?? [];
  const edit = renaming.value;
  const editing = edit?.tab === tab && samePath(edit.path, path);
  const mark = drop && samePath(drop.path, path) ? `drop-${drop.where}` : "";
  const dragged = props.dragging && (samePath(props.dragging, path) || isInside(path, props.dragging));
  return (
    <>
      <div
        role="button"
        tabIndex={0}
        class={`outline-row ${mark} ${dragged ? "dragged" : ""}`}
        data-opath={path.join(".")}
        style={{ paddingLeft: depth * 14 }}
        onPointerDown={(e) => props.onDrag(path, e)}
        onClick={() => !props.suppress.current && !editing && item.p >= 0 && A.goToSource(item.p, item.y)}
        onDblClick={() => tab.info.value?.isPdf && (renaming.value = { tab, path })}
        onContextMenu={(e) => props.onMenu(path, e)}
        onKeyDown={(e) => {
          if (e.key === "F2" && tab.info.value?.isPdf) renaming.value = { tab, path };
        }}
        title={item.t}
      >
        <span
          class="tw"
          onClick={(e) => {
            if (!kids.length) return;
            e.stopPropagation();
            setOpen(!open);
          }}
        >
          {kids.length ? open ? <ChevronDown size={14} /> : <ChevronRight size={14} /> : null}
        </span>
        {editing ? (
          <input
            class="inp sm outline-edit"
            value={item.t}
            ref={(el) => {
              if (el && el !== document.activeElement)
                requestAnimationFrame(() => {
                  el.focus();
                  el.select();
                });
            }}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") props.onRename(path, (e.target as HTMLInputElement).value);
              if (e.key === "Escape") renaming.value = null;
            }}
            onBlur={(e) => props.onRename(path, (e.target as HTMLInputElement).value)}
          />
        ) : (
          <span class="t" style={{ fontWeight: depth === 0 ? 600 : 400 }}>
            {item.t || "Untitled"}
          </span>
        )}
        {item.p >= 0 && <span class="pn">{pageName(tab, item.p)}</span>}
      </div>
      {open &&
        kids.map((k, i) => (
          <OutlineNode key={i} {...props} item={k} path={[...path, i]} />
        ))}
    </>
  );
}
