import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import { bookScreen } from "../modes";
import { selectWhenShown } from "../annots";
import * as A from "../actions";
import { ChevronDown, ChevronRight } from "lucide-preact";
import { api, type OutlineItem } from "../api";
import { renderer, renderKey, type Want } from "../render/RenderManager";
import { LO_SCALE, paint } from "../render/Viewer";
import { activeTab, settings, updateSettings, type Tab } from "../store";
import { specs } from "../edits";
import * as P from "../pages";

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

function Thumbnails({ tab, width, visible }: { tab: Tab; width: number; visible: boolean }) {
  const scroller = useRef<HTMLDivElement>(null);
  const grid = useRef<HTMLDivElement>(null);
  const [scroll, setScroll] = useState({ top: 0, height: 600 });
  const canvases = useRef(new Map<number, HTMLCanvasElement>());
  const userScrolled = useRef(0);
  const anchor = useRef(0);
  const suppressClick = useRef(false);
  const [drag, setDrag] = useState<{ views: number[]; gap: number; x: number; y: number } | null>(null);
  const info = tab.info.value!;
  const sizes = tab.sizes.value;
  const edits = tab.edits.value;
  const current = tab.page.value;
  const sel = tab.pageSel.value;
  const group = `thumbs:${info.docId}`;
  const pages = useMemo(
    () => (edits ? specs(edits, sizes ?? Array.from({ length: info.pageCount }, () => info.firstSize)) : []),
    [edits, sizes],
  );
  const count = pages.length;

  // Grid geometry. Columns step up as the panel widens; within a step, thumbnails grow with it.
  const inner = width - PAD * 2;
  const cols = Math.max(1, Math.floor((inner + 10) / 108));
  const cellW = inner / cols;
  const boxW = Math.max(60, Math.min(cellW - 14, 360));
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

  const startDrag = (i: number, e: PointerEvent) => {
    if (e.button !== 0) return;
    const sx = e.clientX, sy = e.clientY;
    let views: number[] | null = null;
    let autoTimer = 0;
    const move = (ev: PointerEvent) => {
      if (!views) {
        if (Math.hypot(ev.clientX - sx, ev.clientY - sy) < 6) return;
        views = sel.has(i) ? [...sel].sort((a, b) => a - b) : [i];
        if (!sel.has(i)) tab.pageSel.value = new Set([i]);
      }
      setDrag({ views, gap: gapAt(ev.clientX, ev.clientY), x: ev.clientX, y: ev.clientY });
      // Scroll the strip when dragging near its edges.
      const el = scroller.current!;
      const b = el.getBoundingClientRect();
      const dir = ev.clientY < b.top + 36 ? -1 : ev.clientY > b.bottom - 36 ? 1 : 0;
      clearInterval(autoTimer);
      if (dir) autoTimer = window.setInterval(() => (el.scrollTop += dir * 14), 16);
    };
    const up = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      clearInterval(autoTimer);
      if (views) {
        suppressClick.current = true;
        setDrag(null);
        P.movePages(tab, views, gapAt(ev.clientX, ev.clientY));
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const cells = [];
  for (let r = firstRow; r <= lastRow; r++) {
    for (let c = 0; c < cols; c++) {
      const i = r * cols + c;
      if (i >= count) break;
      const spec = pages[i];
      const b = fitBox(i);
      const dragged = drag?.views.includes(i);
      cells.push(
        <button
          key={`${i}:${spec.src}:${spec.rot}`}
          class={`thumb ${i === current ? "on" : ""} ${sel.has(i) ? "sel" : ""} ${dragged ? "dragged" : ""}`}
          style={{ left: PAD + c * cellW, top: r * rowH + 6, width: cellW }}
          onPointerDown={(e) => startDrag(i, e)}
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
      {drag && (
        <div class="drag-ghost" style={{ left: drag.x + 12, top: drag.y + 12 }}>
          {drag.views.length === 1 ? `Page ${drag.views[0] + 1}` : `${drag.views.length} pages`}
        </div>
      )}
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

function Outline({ tab }: { tab: Tab }) {
  const items = tab.outline.value;
  if (items === null) return <div class="empty-note">Loading…</div>;
  if (!items.length) return <div class="empty-note">This document has no outline. Scanned files usually don't.</div>;
  return (
    <div class="side-scroll" style={{ padding: "2px 6px 12px" }}>
      {items.map((it, i) => (
        <OutlineNode key={i} item={it} depth={0} tab={tab} />
      ))}
    </div>
  );
}

function OutlineNode({ item, depth, tab }: { item: OutlineItem; depth: number; tab: Tab }) {
  const [open, setOpen] = useState(depth === 0 && (item.k?.length ?? 0) < 12);
  const kids = item.k ?? [];
  return (
    <>
      <button
        class="outline-row"
        style={{ paddingLeft: depth * 14 }}
        onClick={() => item.p >= 0 && A.goToSource(item.p, item.y)}
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
        <span class="t" style={{ fontWeight: depth === 0 ? 600 : 400 }}>
          {item.t || "Untitled"}
        </span>
        {item.p >= 0 && <span class="pn">{item.p + 1}</span>}
      </button>
      {open && kids.map((k, i) => <OutlineNode key={i} item={k} depth={depth + 1} tab={tab} />)}
    </>
  );
}
