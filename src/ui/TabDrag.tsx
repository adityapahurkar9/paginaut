// Document tabs move: dragged along the strip they change places; dragged off it they go to another window (dropped
// on its tab strip) or into a new window where they're dropped. The tab's right-click menu does the same.
import { signal } from "@preact/signals";
import { AppWindow, FileText, Plus } from "lucide-preact";
import { api, type DropKind } from "../api";
import { closeTab, moveTab, tabs, type Tab } from "../store";
import { moveToNewWindow, moveToWindow, stripCount, stripIndexAt } from "../windows";
import { menu } from "./Toolbar";
import type { MenuItem } from "./Overlays";

/** The tab being dragged (its key), drawn lifted in the strip. */
export const draggingTab = signal<string | null>(null);
/** The page's own label under the pointer, where the system can't show one outside the window (not Windows). */
const ghost = signal<{ name: string; kind: DropKind; x: number; y: number } | null>(null);
/** A tab dragged from another window is over this window's tabs, at this x (page pixels): where it would land. */
export const dropAt = signal<number | null>(null);

/** How far above or below the strip the pointer goes before the tab leaves it. */
const LEAVE = 28;

/** A theme colour as RGB (the native label wears the app's colours). */
function rgb(token: string): [number, number, number] {
  const c = document.createElement("canvas").getContext("2d")!;
  c.fillStyle = getComputedStyle(document.documentElement).getPropertyValue(token).trim() || "#888";
  c.fillRect(0, 0, 1, 1);
  const [r, g, b] = c.getImageData(0, 0, 1, 1).data;
  return [r, g, b];
}

/**
 * The label under the pointer while a tab is dragged, with what letting go does: Windows gets a small native pill
 * that follows the mouse anywhere on screen (dragghost.rs); elsewhere the page draws one inside the window.
 */
function ghostShow(tab: Tab, kind: DropKind, at: { x: number; y: number }, native: { on: boolean | null }) {
  const own = () => (ghost.value = { name: tab.name, kind, ...at });
  if (native.on === false) return own();
  const colors = ["--panel", "--tx", "--acc", "--on-acc"].map(rgb);
  void api.dragGhost(tab.name, kind, colors).then(
    (ok) => {
      native.on = ok;
      if (!ok && draggingTab.value === tab.key) own();
    },
    () => (native.on = false),
  );
}

/**
 * Drags a document tab, as in Chrome: along the strip it slides with the pointer and the other tabs move aside;
 * pulled off the strip a label follows the pointer and says what letting go does (another window, a new one).
 */
export function startTabDrag(e: PointerEvent, tab: Tab) {
  if (e.button !== 0 || (e.target as HTMLElement).closest(".x")) return;
  const grabbed = e.currentTarget as HTMLElement;
  const strip = grabbed.parentElement;
  if (!strip) return;
  const x0 = e.clientX, y0 = e.clientY;
  /** Where in the tab it was taken hold of. */
  const grab = x0 - grabbed.getBoundingClientRect().left;
  let dragging = false, out = false;
  /** Whether the native label works (null until asked). */
  const native = { on: null as boolean | null };
  let kind: DropKind = "move";
  let labelShown = false;
  let at = { x: x0, y: y0 };
  const setKind = (k: DropKind) => {
    if (k === kind && labelShown) return;
    kind = k;
    labelShown = true;
    ghostShow(tab, kind, at, native);
  };
  const hideLabel = () => {
    if (!labelShown) return;
    labelShown = false;
    ghost.value = null;
    if (native.on !== false) void api.dragGhostHide().catch(() => {});
  };
  const docTabs = () => [...strip.querySelectorAll<HTMLElement>(".tab[data-tab]")];
  const me = () => docTabs().find((el) => el.dataset.tab === tab.key) ?? null;

  /** The tab under the pointer: it slides along (not past the first or last document tab). */
  const follow = () => {
    const el = me();
    if (!el) return;
    const all = docTabs();
    const lo = Math.min(...all.map((x) => x.offsetLeft));
    const hi = Math.max(...all.map((x) => x.offsetLeft + x.offsetWidth)) - el.offsetWidth;
    const want = at.x - strip.getBoundingClientRect().left + strip.scrollLeft - grab;
    el.style.transform = `translateX(${Math.max(lo, Math.min(hi, want)) - el.offsetLeft}px)`;
  };
  /** Moves the tab to the place its middle has reached; the others slide into their new places. */
  const reorder = () => {
    const el = me();
    if (!el) return;
    const middle = at.x - strip.getBoundingClientRect().left + strip.scrollLeft - grab + el.offsetWidth / 2;
    const others = docTabs().filter((x) => x !== el);
    // (Places from offsetLeft, which leaves out the slide: no jitter while the others move.)
    const i = others.filter((x) => x.offsetLeft + x.offsetWidth / 2 < middle).length;
    const before = new Map(others.map((x) => [x, x.offsetLeft]));
    if (!moveTab(tab, i)) return follow();
    requestAnimationFrame(() => {
      for (const [x, was] of before) {
        const dx = was - x.offsetLeft;
        if (!dx) continue;
        x.style.transition = "none";
        x.style.transform = `translateX(${dx}px)`;
        void x.offsetWidth;
        x.style.transition = "transform 0.15s ease";
        x.style.transform = "";
      }
      follow();
    });
  };
  /** Back in its place (animated), styles cleared after. */
  const settle = () => {
    for (const x of docTabs()) {
      x.style.transition = "transform 0.12s ease";
      x.style.transform = "";
    }
    setTimeout(() => docTabs().forEach((x) => (x.style.transition = "")), 160);
  };

  /** The other window showing where the tab would land. */
  let hovered: string | null = null;
  const leaveHovered = () => {
    if (hovered) void api.tabHover(hovered, null).catch(() => {});
    hovered = null;
  };
  /** Off the strip: which window it would go to (asked often; the mouse may be over another window). */
  let asking = false, lastAsk = 0;
  const target = () => {
    const now = performance.now();
    if (asking || now - lastAsk < 50) return;
    asking = true;
    lastAsk = now;
    void api.windowAt().then((t) => {
      asking = false;
      if (!dragging || !out) return;
      if (t) {
        if (hovered !== t.label) leaveHovered();
        hovered = t.label;
        void api.tabHover(t.label, t.x).catch(() => {});
        setKind("join");
      } else {
        leaveHovered();
        // A window's only tab dropped on nothing stays where it is.
        setKind(stripCount() < 2 ? "move" : "new");
      }
    }, () => (asking = false));
  };
  // On the strip itself (it stays put while the tabs in it move), so the pointer is followed outside the window too.
  strip.setPointerCapture(e.pointerId);
  const move = (ev: PointerEvent) => {
    if (!dragging && Math.hypot(ev.clientX - x0, ev.clientY - y0) < 6) return;
    at = { x: ev.clientX, y: ev.clientY };
    if (!dragging) {
      dragging = true;
      draggingTab.value = tab.key;
    }
    if (ghost.value) ghost.value = { ...ghost.value, ...at };
    const r = strip.getBoundingClientRect();
    const wasOut = out;
    out = ev.clientY < r.top - LEAVE || ev.clientY > r.bottom + LEAVE || ev.clientX < 0 || ev.clientX > window.innerWidth;
    if (out) {
      // Off the strip: the tab waits in its place, the label takes over.
      if (!wasOut) {
        const el = me();
        if (el) el.style.transform = "";
        if (!labelShown) setKind(stripCount() < 2 ? "move" : "new");
      }
      return target();
    }
    leaveHovered();
    hideLabel();
    reorder();
  };
  const end = (ev: PointerEvent) => {
    strip.removeEventListener("pointermove", move);
    strip.removeEventListener("pointerup", end);
    strip.removeEventListener("pointercancel", end);
    if (strip.hasPointerCapture(ev.pointerId)) strip.releasePointerCapture(ev.pointerId);
    hideLabel();
    draggingTab.value = null;
    leaveHovered();
    settle();
    const was = dragging;
    dragging = false;
    if (!was || !out || ev.type === "pointercancel") return;
    void dropOutside(tab);
  };
  strip.addEventListener("pointermove", move);
  strip.addEventListener("pointerup", end);
  strip.addEventListener("pointercancel", end);
}

/** Dropped off the strip: onto another window's strip, or a new window there (unless it's the window's only tab). */
async function dropOutside(tab: Tab) {
  const target = await api.windowAt().catch(() => null);
  if (target) return moveToWindow(tab, target.label, target.x);
  if (stripCount() < 2) return;
  return moveToNewWindow(tab, true);
}

const KIND_ICON: Record<DropKind, preact.ComponentChild> = { move: <FileText size={13} />, join: <Plus size={13} strokeWidth={2.6} />, new: <AppWindow size={13} /> };

/** The page's own label (where there's no native one). */
export function TabDragGhost() {
  const g = ghost.value;
  if (!g) return null;
  return (
    <div class={`tab-ghost ${g.kind}`} style={{ left: g.x + 12, top: g.y + 16 }}>
      {KIND_ICON[g.kind]}
      <span>{g.name}</span>
    </div>
  );
}

/** Where a tab dragged from another window would land in this window's strip (left edge, in strip pixels). */
export function dropMarkerLeft(strip: HTMLElement, x: number): number {
  const box = strip.getBoundingClientRect();
  const tabsHere = [...strip.querySelectorAll<HTMLElement>(".tab[data-tab]")].map((el) => el.getBoundingClientRect());
  const i = stripIndexAt(x);
  const edge = i < tabsHere.length ? tabsHere[i].left - 2 : tabsHere.length ? tabsHere[tabsHere.length - 1].right : box.left + 2;
  return edge - box.left;
}

/** A document tab's right-click menu. */
export async function openTabMenu(e: MouseEvent, tab: Tab) {
  e.preventDefault();
  const at = { x: e.clientX, y: e.clientY };
  const others = await api.windowList().catch(() => []);
  const rest = () => tabs.value.filter((t) => t !== tab);
  const items: MenuItem[] = [
    { label: "Move tab to new window", disabled: stripCount() < 2, run: () => void moveToNewWindow(tab) },
    ...(others.length ? [{ head: "Move tab to window" } as MenuItem] : []),
    ...others.map((w): MenuItem => ({ label: w.title, run: () => void moveToWindow(tab, w.label) })),
    { sep: true },
    { label: "Close tab", hint: "Ctrl+W", run: () => void closeTab(tab) },
    {
      label: "Close other tabs",
      disabled: !rest().length,
      run: async () => {
        for (const t of rest()) if (!(await closeTab(t))) return;
      },
    },
  ];
  menu.value = { ...at, items, width: 240 };
}
