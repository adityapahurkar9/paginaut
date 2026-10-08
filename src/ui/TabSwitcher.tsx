// Alt+` moves between this window's tabs, most recently used first. Hold Alt and press ` to step forward (~, the
// same key with Shift, steps back); let go of Alt to switch. A quick press goes straight back to the tab before.
// Rather than a row of cards across a dimmed screen, the tabs drop down from the tab strip as a list, with the
// chosen tab's page shown large beside it.
import { signal } from "@preact/signals";
import { useEffect, useRef } from "preact/hooks";
import { FileDiff, House } from "lucide-preact";
import { COMPARE, compare } from "../compare";
import { renderer, renderKey, type Bitmap } from "../render/RenderManager";
import { LO_SCALE, paint } from "../render/Viewer";
import { activeKey, dialog, isHome, settingsOpen, tabs, tabsByUse, type Tab } from "../store";
import { tabIcon } from "./App";

interface State {
  keys: string[];
  sel: number;
  /** Shown only once Alt is held a moment: a quick Alt+` just flips to the tab before. */
  shown: boolean;
}
const state = signal<State | null>(null);
let showTimer = 0;

function open(back: boolean) {
  const keys = tabsByUse();
  if (keys.length < 2) return;
  state.value = { keys, sel: back ? keys.length - 1 : 1, shown: false };
  clearTimeout(showTimer);
  showTimer = window.setTimeout(() => state.value && (state.value = { ...state.value, shown: true }), 140);
}

function step(by: number) {
  const s = state.value;
  if (!s) return;
  const n = s.keys.length;
  state.value = { ...s, sel: (s.sel + by + n) % n, shown: true };
}

function close(commit: boolean) {
  const s = state.value;
  clearTimeout(showTimer);
  state.value = null;
  // (A tab that closed meanwhile isn't switched to.)
  const key = s?.keys[s.sel];
  if (commit && key && tabsByUse().includes(key)) activeKey.value = key;
}

/** Alt+` (Alt+~ with Shift): opens the switcher, or steps through it. */
function press(back: boolean) {
  if (!state.value) open(back);
  else step(back ? -1 : 1);
}

const isSwitchKey = (e: KeyboardEvent) => e.code === "Backquote" && e.altKey && !e.ctrlKey && !e.metaKey;
/** The last Alt+` was seen pressed (not only let go of, see the keyup handler). */
let pressSeen = false;

/** The keyboard handler's first stop: true when the key was the switcher's (Alt+`, or any key while it's open). */
export function switcherKey(e: KeyboardEvent): boolean {
  if (isSwitchKey(e)) {
    pressSeen = true;
    if (!e.repeat || state.value) press(e.shiftKey);
    return true;
  }
  if (!state.value) return false;
  if (e.key === "ArrowDown" || e.key === "ArrowRight" || e.key === "Tab") step(e.shiftKey && e.key === "Tab" ? -1 : 1);
  else if (e.key === "ArrowUp" || e.key === "ArrowLeft") step(-1);
  else if (e.key === "Enter") close(true);
  else if (e.key === "Escape") close(false);
  return true;
}

window.addEventListener("keyup", (e) => {
  if (dialog.value || settingsOpen.value) return;
  // Some Windows set-ups (input methods) take the press of Alt+` for themselves and only its release reaches the
  // page: then the release counts as the press.
  if (isSwitchKey(e)) {
    e.preventDefault();
    if (!pressSeen) press(e.shiftKey);
    pressSeen = false;
    return;
  }
  // Letting go of Alt switches.
  if (state.value && e.key === "Alt") {
    e.preventDefault();
    close(true);
  }
});
// The window losing focus (Windows' own Alt+Tab) leaves things as they were.
window.addEventListener("blur", () => close(false));

/** For the command palette: opens the switcher shown, for picking with the mouse or arrows and Enter. */
export function openSwitcher() {
  open(false);
  if (state.value) state.value = { ...state.value, shown: true };
}

function describe(key: string): { icon: preact.ComponentChild; name: string; sub: string; dirty: boolean; tab: Tab | null } {
  if (isHome(key)) return { icon: <House size={16} />, name: "Home", sub: "Recent files and books", dirty: false, tab: null };
  if (key === COMPARE) {
    const c = compare.value;
    return { icon: <FileDiff size={16} />, name: c ? `${c.a.name} · ${c.b.name}` : "Compare", sub: "Comparison", dirty: false, tab: null };
  }
  const t = tabs.value.find((x) => x.key === key) ?? null;
  if (!t) return { icon: null, name: "", sub: "", dirty: false, tab: null };
  const mode = t.mode.value === "read" ? " · Read mode" : t.mode.value === "book" ? " · Book mode" : "";
  const n = t.edits.value?.order.length ?? t.pageCount.value;
  const sub = t.status.value !== "ready" ? (t.status.value === "loading" ? "Opening…" : t.status.value === "password" ? "Needs a password" : "Couldn't open") : `Page ${Math.min(t.page.value + 1, n)} of ${n}${mode}`;
  return { icon: tabIcon(t), name: t.name, sub, dirty: t.dirty.value, tab: t };
}

/** Room for the preview, in page pixels. */
const BOX_W = 248;
const BOX_H = 322;

/** The chosen tab's page: at once from the small renders the viewer and thumbnails keep, then sharper. */
function Preview({ tab, icon }: { tab: Tab | null; icon: preact.ComponentChild }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const info = tab?.status.value === "ready" ? tab.info.value : null;
  const edits = tab?.edits.value;
  const view = tab ? Math.min(tab.page.value, (edits?.order.length ?? 1) - 1) : 0;
  useEffect(() => {
    const c = canvas.current;
    if (!c || !tab || !info || !edits) return;
    c.width = c.height = 0;
    const src = edits.order[view] ?? 0;
    const rot = edits.rot[src] ?? 0;
    const [pw, ph] = tab.sizes.value?.[src] ?? [612, 792];
    const [w, h] = rot % 180 ? [ph, pw] : [pw, ph];
    const fit = Math.min(BOX_W / w, BOX_H / h);
    c.style.width = `${Math.round(w * fit)}px`;
    c.style.height = `${Math.round(h * fit)}px`;
    const sharp = Math.ceil(fit * (window.devicePixelRatio || 1) * 20) / 20;
    let shown = 0;
    const draw = (b: Bitmap) => {
      if (b.scale <= shown) return;
      shown = b.scale;
      c.width = b.width;
      c.height = b.height;
      paint(c, b);
    };
    const doc = info.docId;
    renderer.sync("switcher", [
      { key: renderKey(doc, src, rot, "lo", LO_SCALE), params: { doc, page: src, scale: LO_SCALE, rotate: rot }, prio: 1500, onReady: draw },
      { key: renderKey(doc, src, rot, "thumb", sharp), params: { doc, page: src, scale: sharp, rotate: rot }, prio: 1400, onReady: draw },
    ]);
    return () => renderer.release("switcher");
  }, [tab, info, view, edits]);
  if (!tab || !info) return <div class="tsw-preview empty">{icon}</div>;
  return (
    <div class="tsw-preview">
      <canvas ref={canvas} />
    </div>
  );
}

export function TabSwitcher() {
  const s = state.value;
  if (!s?.shown) return null;
  // Under the current tab, kept inside the window.
  const at = document.querySelector<HTMLElement>(`.titlebar .tab.on`)?.getBoundingClientRect();
  const left = Math.max(12, Math.min(at?.left ?? 12, window.innerWidth - 640));
  const chosen = describe(s.keys[s.sel]);
  return (
    <div class="tsw" style={{ left }} role="listbox" aria-label="Switch tabs">
      <div class="tsw-list">
        <div class="tsw-head">
          Switch tab <span>Alt+` next · Alt+~ back</span>
        </div>
        {s.keys.map((k, i) => {
          const d = describe(k);
          return (
            <button
              key={k}
              role="option"
              aria-selected={i === s.sel}
              class={`tsw-row ${i === s.sel ? "on" : ""} ${k === activeKey.value ? "now" : ""}`}
              onMouseEnter={() => state.value && (state.value = { ...state.value, sel: i })}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                state.value = { ...s, sel: i };
                close(true);
              }}
            >
              <span class="tsw-icon">{d.icon}</span>
              <span class="tsw-text">
                <span class="tsw-name">{d.name}</span>
                <span class="tsw-sub">{d.sub}</span>
              </span>
              {d.dirty && <span class="dot" title="Unsaved changes" />}
            </button>
          );
        })}
      </div>
      <Preview tab={chosen.tab} icon={chosen.icon} />
    </div>
  );
}
