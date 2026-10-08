// Document tabs move: dragged along the strip they change places; dragged off it they go to another window (dropped
// on its tab strip) or into a new window where they're dropped. The tab's right-click menu does the same.
import { signal } from "@preact/signals";
import { api } from "../api";
import { closeTab, moveTab, tabs, type Tab } from "../store";
import { moveToNewWindow, moveToWindow, stripCount } from "../windows";
import { menu } from "./Toolbar";
import type { MenuItem } from "./Overlays";

/** The tab being dragged off the strip, following the pointer. */
const ghost = signal<{ name: string; x: number; y: number } | null>(null);

/** How far above or below the strip the pointer goes before the tab leaves it. */
const LEAVE = 28;

export function startTabDrag(e: PointerEvent, tab: Tab) {
  if (e.button !== 0 || (e.target as HTMLElement).closest(".x")) return;
  const strip = (e.currentTarget as HTMLElement).parentElement;
  if (!strip) return;
  const x0 = e.clientX, y0 = e.clientY;
  let dragging = false, out = false;
  // On the strip itself (it stays put while the tabs in it move), so the pointer is followed outside the window too.
  strip.setPointerCapture(e.pointerId);
  const move = (ev: PointerEvent) => {
    if (!dragging && Math.hypot(ev.clientX - x0, ev.clientY - y0) < 6) return;
    dragging = true;
    const r = strip.getBoundingClientRect();
    out = ev.clientY < r.top - LEAVE || ev.clientY > r.bottom + LEAVE || ev.clientX < 0 || ev.clientX > window.innerWidth;
    if (out) {
      ghost.value = { name: tab.name, x: ev.clientX, y: ev.clientY };
      return;
    }
    ghost.value = null;
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
    if (!dragging || !out || ev.type === "pointercancel") return;
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

export function TabDragGhost() {
  const g = ghost.value;
  if (!g) return null;
  return (
    <div class="drag-ghost" style={{ left: g.x + 12, top: g.y + 12 }}>
      {g.name} · new window
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
