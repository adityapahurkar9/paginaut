// Document tabs move: dragged along the strip they change places; dragged off it they go to another window (dropped
// on its tab strip) or into a new window where they're dropped. The tab's right-click menu does the same.
import { signal } from "@preact/signals";
import { api } from "../api";
import { closeTab, moveTab, tabs, type Tab } from "../store";
import { moveToNewWindow, moveToWindow, stripCount } from "../windows";
import { menu } from "./Toolbar";
import type { MenuItem } from "./Overlays";

/** The tab being dragged (its key), drawn lifted in the strip. */
export const draggingTab = signal<string | null>(null);
/** The page's own ghost under the pointer, where the system can't show one outside the window (not Windows). */
const ghost = signal<{ name: string; hint: string; x: number; y: number } | null>(null);

/** How far above or below the strip the pointer goes before the tab leaves it. */
const LEAVE = 28;

/** A theme colour as RGB (the native ghost wears the app's colours). */
function rgb(token: string): [number, number, number] {
  const c = document.createElement("canvas").getContext("2d")!;
  c.fillStyle = getComputedStyle(document.documentElement).getPropertyValue(token).trim() || "#888";
  c.fillRect(0, 0, 1, 1);
  const [r, g, b] = c.getImageData(0, 0, 1, 1).data;
  return [r, g, b];
}

/**
 * What's under the pointer while a tab is dragged: Windows gets a small native label that follows the mouse
 * anywhere on screen (dragghost.rs); elsewhere the page draws one inside the window.
 */
function ghostShow(tab: Tab, hint: string, at: { x: number; y: number }, native: { on: boolean | null }) {
  const own = () => (ghost.value = { name: tab.name, hint, ...at });
  if (native.on === false) return own();
  const colors = ["--panel", "--tx", "--tx3", "--acc"].map(rgb);
  void api.dragGhost(tab.name, hint, colors).then(
    (ok) => {
      native.on = ok;
      if (!ok && draggingTab.value === tab.key) own();
    },
    () => (native.on = false),
  );
}

export function startTabDrag(e: PointerEvent, tab: Tab) {
  if (e.button !== 0 || (e.target as HTMLElement).closest(".x")) return;
  const strip = (e.currentTarget as HTMLElement).parentElement;
  if (!strip) return;
  const x0 = e.clientX, y0 = e.clientY;
  let dragging = false, out = false;
  /** Whether the native ghost works (null until asked). */
  const native = { on: null as boolean | null };
  let hint = "";
  /** Off the strip: which window it would go to (asked now and then; the mouse may be over another window). */
  let asking = false, lastAsk = 0;
  const target = () => {
    const now = performance.now();
    if (asking || now - lastAsk < 120) return;
    asking = true;
    lastAsk = now;
    void api.windowAt().then((t) => {
      asking = false;
      if (!dragging || !out) return;
      const next = t ? `Move to ${t.title}` : stripCount() < 2 ? "Its only tab: stays here" : "Open in a new window";
      if (next !== hint) {
        hint = next;
        ghostShow(tab, hint, ghost.value ?? { x: x0, y: y0 }, native);
      }
    }, () => (asking = false));
  };
  // On the strip itself (it stays put while the tabs in it move), so the pointer is followed outside the window too.
  strip.setPointerCapture(e.pointerId);
  const move = (ev: PointerEvent) => {
    if (!dragging && Math.hypot(ev.clientX - x0, ev.clientY - y0) < 6) return;
    const at = { x: ev.clientX, y: ev.clientY };
    if (!dragging) {
      dragging = true;
      draggingTab.value = tab.key;
      ghostShow(tab, hint, at, native);
    }
    if (ghost.value) ghost.value = { ...ghost.value, ...at };
    const r = strip.getBoundingClientRect();
    const wasOut = out;
    out = ev.clientY < r.top - LEAVE || ev.clientY > r.bottom + LEAVE || ev.clientX < 0 || ev.clientX > window.innerWidth;
    if (out) return target();
    if (wasOut) {
      hint = "";
      ghostShow(tab, hint, at, native);
    }
    // Its place: after every other tab whose middle the pointer has passed.
    let i = 0;
    for (const el of strip.querySelectorAll<HTMLElement>(".tab[data-tab]")) {
      if (el.dataset.tab === tab.key) continue;
      const b = el.getBoundingClientRect();
      if (ev.clientX > b.left + b.width / 2) i++;
    }
    moveTab(tab, i);
  };
  const end = (ev: PointerEvent) => {
    strip.removeEventListener("pointermove", move);
    strip.removeEventListener("pointerup", end);
    strip.removeEventListener("pointercancel", end);
    if (strip.hasPointerCapture(ev.pointerId)) strip.releasePointerCapture(ev.pointerId);
    ghost.value = null;
    draggingTab.value = null;
    if (dragging && native.on !== false) void api.dragGhostHide().catch(() => {});
    dragging = false;
    if (!out || ev.type === "pointercancel") return;
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

/** The page's own ghost (where there's no native one). */
export function TabDragGhost() {
  const g = ghost.value;
  if (!g) return null;
  return (
    <div class="drag-ghost tab-ghost" style={{ left: g.x + 14, top: g.y + 18 }}>
      <strong>{g.name}</strong>
      {g.hint && <span>{g.hint}</span>}
    </div>
  );
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
