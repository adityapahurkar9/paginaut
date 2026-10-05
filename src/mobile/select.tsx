// Selecting text on the pages by touch: press and hold a word, drag on to take more, then the
// handles adjust either end and a small menu copies it. (Read and Book use Android's own.)
import { useEffect, useState } from "preact/hooks";
import { signal } from "@preact/signals";
import type { Viewer } from "../render/Viewer";
import { native } from "./native";
import { say } from "./state";

/** Bumped whenever the selection changes (the viewer's onSelection), so its handles follow. */
export const selTick = signal(0);

/** A long press is selecting: page turning and taps leave this touch alone. */
let holding = false;
export const pressHolding = () => holding;

/** Copies text to the clipboard (the WebView's clipboard API, else the old way through a field). */
export async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    /* not allowed here: fall back */
  }
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.style.cssText = "position:fixed;opacity:0;top:0;left:0";
  document.body.append(ta);
  ta.select();
  document.execCommand("copy");
  ta.remove();
}

const PRESS_MS = 450;

/** Press and hold on a page selects the word there; dragging on (finger still down) extends it. */
export function usePressSelect(v: Viewer | null) {
  useEffect(() => {
    if (!v) return;
    const el = v.el;
    // pressing: the press is being looked up (the finger is ours, but nothing to drag yet).
    let timer = 0, x0 = 0, y0 = 0, down = false, pressing = false, dragged = false;
    const start = (e: TouchEvent) => {
      clearTimeout(timer);
      down = e.touches.length === 1;
      if (!down) return;
      x0 = e.touches[0].clientX;
      y0 = e.touches[0].clientY;
      dragged = false;
      timer = window.setTimeout(async () => {
        holding = pressing = true;
        const found = await v.selectWordAt(x0, y0);
        pressing = false;
        if (found) navigator.vibrate?.(15);
        if (!found || !down) holding = false;
        selTick.value++;
      }, PRESS_MS);
    };
    const move = (e: TouchEvent) => {
      const t = e.touches[0];
      if (!holding) {
        if (Math.hypot(t.clientX - x0, t.clientY - y0) > 10 || e.touches.length > 1) clearTimeout(timer);
        return;
      }
      if (e.cancelable) e.preventDefault();
      // Drag on from the word once it's found (a finger never holds perfectly still).
      if (pressing || (!dragged && Math.hypot(t.clientX - x0, t.clientY - y0) < 12)) return;
      dragged = true;
      v.moveSelectionEnd("b", t.clientX, t.clientY);
    };
    const end = () => {
      clearTimeout(timer);
      down = false;
      if (holding) {
        // The click that follows this touch mustn't count as a tap (it would clear the selection).
        setTimeout(() => {
          holding = false;
          selTick.value++;
        }, 350);
      }
    };
    // Android's own long-press menu (for links, pictures) has nothing to offer on a page.
    const menu = (e: Event) => e.preventDefault();
    el.addEventListener("touchstart", start, { passive: true });
    el.addEventListener("touchmove", move, { passive: false });
    el.addEventListener("touchend", end);
    el.addEventListener("touchcancel", end);
    el.addEventListener("contextmenu", menu);
    return () => {
      clearTimeout(timer);
      el.removeEventListener("touchstart", start);
      el.removeEventListener("touchmove", move);
      el.removeEventListener("touchend", end);
      el.removeEventListener("touchcancel", end);
      el.removeEventListener("contextmenu", menu);
    };
  }, [v]);
}

/** The selection's handles and its menu (Copy, Share, Select… paragraph or page), over the pages. */
export function PageSelection({ v }: { v: Viewer }) {
  void selTick.value;
  // "Select…" shows the paragraph and page choices, until the selection goes.
  const [more, setMore] = useState(false);
  const has = v.hasSelection;
  useEffect(() => {
    if (!has) setMore(false);
  }, [has]);
  // Follow the pages as they scroll.
  const [, setScroll] = useState(0);
  useEffect(() => {
    let raf = 0;
    const on = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => setScroll((n) => n + 1));
    };
    v.el.addEventListener("scroll", on, { passive: true });
    return () => {
      cancelAnimationFrame(raf);
      v.el.removeEventListener("scroll", on);
    };
  }, [v]);
  const ends = v.selectionEnds();
  if (!ends) return null;
  // Positions are on screen; the overlay sits in #app (inside the system bars).
  const o = document.getElementById("app")!.getBoundingClientRect();
  const H = o.height;
  const menuTop = ends.top - o.top > 64 ? ends.top - o.top - 56 : Math.min(H - 60, ends.bottom - o.top + 34);
  const menuX = Math.max(8, Math.min(o.width - 288, (ends.start[0] + ends.end[0]) / 2 - o.left - 140));

  const drag = (end: "a" | "b") => (e: TouchEvent) => {
    e.stopPropagation();
    // The handle hangs below the line: keep the point the finger moves at the line, not under it.
    const p = end === "a" ? ends.start : ends.end;
    const dx = p[0] - e.touches[0].clientX, dy = p[1] - 6 - e.touches[0].clientY;
    const move = (ev: TouchEvent) => {
      if (ev.cancelable) ev.preventDefault();
      const t = ev.touches[0];
      v.moveSelectionEnd(end, t.clientX + dx, t.clientY + dy);
    };
    const up = () => {
      removeEventListener("touchmove", move);
      removeEventListener("touchend", up);
      removeEventListener("touchcancel", up);
    };
    addEventListener("touchmove", move, { passive: false });
    addEventListener("touchend", up);
    addEventListener("touchcancel", up);
  };

  return (
    <div class="sel-ui">
      <span class="sel-handle start" style={{ left: `${ends.start[0] - o.left}px`, top: `${ends.start[1] - o.top}px` }} onTouchStart={drag("a")} />
      <span class="sel-handle end" style={{ left: `${ends.end[0] - o.left}px`, top: `${ends.end[1] - o.top}px` }} onTouchStart={drag("b")} />
      {!holding && (
        <div class="sel-menu" style={{ left: `${menuX}px`, top: `${menuTop}px` }}>
          <button
            onClick={async () => {
              const text = await v.copySelection();
              if (!text) return;
              await copyText(text);
              v.clearSelection();
              say("Copied");
            }}
          >
            Copy
          </button>
          {more ? (
            <>
              <button onClick={() => void v.selectParagraph()}>Paragraph</button>
              <button onClick={() => v.selectAllOnPage(v.selectionView ?? undefined)}>Page</button>
            </>
          ) : (
            <>
              <button
                onClick={async () => {
                  const text = await v.copySelection();
                  if (text) await native.shareText(text).catch((e) => say(String(e)));
                }}
              >
                Share
              </button>
              <button onClick={() => setMore(true)}>Select…</button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
