// The open document: its pages (the desktop's viewer) or its text reflowed (Read, Book), with bars a tap
// shows or hides.
import { useEffect, useRef, useState } from "preact/hooks";
import { ArrowLeft, ChevronDown, ChevronUp, EllipsisVertical, ListTree, Moon, Pause, Play, Search, SlidersHorizontal, Square, Sun, X } from "lucide-preact";
import { api } from "../api";
import { Viewer, type FitMode } from "../render/Viewer";
import type { Flow } from "../reflow/model";
import { BookView } from "../reflow/BookView";
import { ReadView, type ReflowView } from "../reflow/ReadView";
import { createPictures } from "../reflow/render";
import { analysisOf } from "./flow";
import { back, pushLayer } from "./nav";
import { leaveReader, savePosition, setPositionSource, startPosition } from "./open";
import { clearFind, find, found, next, prev } from "./search";
import { openSheet } from "./Sheets";
import { nextRate, pauseReading, reading, resumeReading, stopReading } from "./speech";
import { chrome, doc, isDark, mode, page, searching, settings, sheet, updateSettings, type OpenDoc, type TextSettings } from "./state";
import { bookInfo, goTo, movedByApp, views } from "./views";

export function Reader({ d }: { d: OpenDoc }) {
  const night = settings.value.night;
  const m = mode.value;
  return (
    <div class={`reader ${night ? "night" : ""} ${chrome.value ? "chrome" : ""} mode-${m}`}>
      <Pages d={d} />
      {m !== "pages" && <Reflow d={d} key={`${d.info.docId}-${m}`} />}
      {searching.value ? <FindBar /> : <TopBar d={d} />}
      <BottomBar d={d} />
      {reading.value && <SpeechBar />}
    </div>
  );
}

// ---------------------------------------------------------------- taps

/** One tap shows or hides the bars; two zoom (pages). Links and buttons are left alone. */
function useTaps(el: () => HTMLElement | null, onDouble?: (x: number, y: number) => void, onlyMiddle = false) {
  useEffect(() => {
    const host = el();
    if (!host) return;
    let last = 0, lx = 0, ly = 0, timer = 0;
    const click = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      if (t.closest(".pg-link, a, button, input, .rf-lightbox")) return;
      if (onlyMiddle) {
        // Book mode turns pages with taps near the edges.
        const w = host.clientWidth;
        if (e.clientX < w * 0.22 || e.clientX > w * 0.78) return;
      }
      const now = performance.now();
      if (onDouble && now - last < 320 && Math.hypot(e.clientX - lx, e.clientY - ly) < 40) {
        clearTimeout(timer);
        last = 0;
        onDouble(e.clientX, e.clientY);
        return;
      }
      last = now;
      lx = e.clientX;
      ly = e.clientY;
      clearTimeout(timer);
      timer = window.setTimeout(
        () => {
          if (getSelection()?.toString()) return;
          if (sheet.value) return;
          chrome.value = !chrome.value;
        },
        onDouble ? 300 : 0,
      );
    };
    host.addEventListener("click", click);
    return () => {
      host.removeEventListener("click", click);
      clearTimeout(timer);
    };
  }, [el()]);
}

// ---------------------------------------------------------------- pages

function fitFor(layout: string): FitMode {
  return layout === "scroll" ? "width" : "page";
}

function Pages({ d }: { d: OpenDoc }) {
  const host = useRef<HTMLDivElement>(null);
  const [v, setV] = useState<Viewer | null>(null);
  const layout = settings.value.layout;
  const night = settings.value.night;

  useEffect(() => {
    let shownAt = performance.now();
    const viewer = new Viewer(host.current!, d.info, {
      onPage: (p) => {
        if (mode.value === "pages") page.value = p;
      },
      onZoom: (_z, fit) => {
        // Zoomed in by hand: pages no longer snap one at a time.
        viewer.el.classList.toggle("snap", settings.value.layout !== "scroll" && fit === "page");
      },
      onSelection() {},
      onPageAction() {},
      onScroll: () => {
        // Reading on hides the bars (not when the app moved the view, nor right after they appeared).
        if (mode.value !== "pages" || !chrome.value || sheet.value || searching.value || movedByApp()) return;
        if (performance.now() - shownAt > 700) chrome.value = false;
      },
    });
    const unwatch = chrome.subscribe((c) => c && (shownAt = performance.now()));
    views.viewer = viewer;
    const L = settings.value.layout;
    viewer.setLayoutMode(L === "double" ? "spread" : "single");
    viewer.setSlots(L !== "scroll");
    viewer.restore({ page: startPosition.page, frac: startPosition.frac, zoom: 1, fit: fitFor(L) });
    api
      .pageSizes(d.info.docId)
      .then((sizes) => viewer.setPages(sizes.map(([w, h], i) => ({ src: i, rot: 0, w, h }))))
      .catch(() => {});
    if (mode.value === "pages") viewer.focus();
    setV(viewer);
    return () => {
      unwatch();
      views.viewer = null;
      viewer.destroy();
    };
  }, [d.info.docId]);

  // Where you are, for Recent.
  useEffect(() => {
    if (!v) return;
    setPositionSource(() => {
      if (mode.value !== "pages") return { page: page.value, frac: 0 };
      const st = v.viewState();
      return { page: st.page, frac: st.frac };
    });
  }, [v]);

  useEffect(() => v?.setNight(night), [v, night]);
  useEffect(() => {
    if (!v) return;
    v.setLayoutMode(layout === "double" ? "spread" : "single");
    v.setSlots(layout !== "scroll");
    v.setFit(fitFor(layout));
    v.el.classList.toggle("snap", layout !== "scroll");
  }, [v, layout]);

  // Coming back from Read or Book: the page they were on.
  useEffect(() => {
    if (v && mode.value === "pages") {
      v.goToPage(page.value);
      v.focus();
    }
  }, [v, mode.value]);

  // One by one: a sideways swipe turns the page too, sliding it out and the next one in.
  useEffect(() => {
    if (!v || layout === "scroll") return;
    const el = v.el;
    const sizer = el.querySelector<HTMLElement>(".viewer-sizer")!;
    let x0 = 0, y0 = 0, t0 = 0, dx = 0, state: "" | "maybe" | "side" | "no" = "", busy = false;
    const slide = (x: number, ms: number) => {
      sizer.style.transition = ms ? `transform ${ms}ms cubic-bezier(0.2, 0.8, 0.2, 1)` : "";
      sizer.style.transform = x ? `translateX(${x}px)` : "";
    };
    const start = (e: TouchEvent) => {
      // Zoomed in, the finger pans the page instead.
      state = e.touches.length === 1 && !busy && v.viewState().fit !== "none" ? "maybe" : "no";
      x0 = e.touches[0].clientX;
      y0 = e.touches[0].clientY;
      t0 = e.timeStamp;
      dx = 0;
    };
    const move = (e: TouchEvent) => {
      if (state === "no" || !state) return;
      if (e.touches.length > 1) {
        if (state === "side") slide(0, 160);
        state = "no";
        return;
      }
      const ddx = e.touches[0].clientX - x0, ddy = e.touches[0].clientY - y0;
      if (state === "maybe") {
        if (Math.hypot(ddx, ddy) < 10) return;
        state = Math.abs(ddx) > Math.abs(ddy) ? "side" : "no";
        if (state === "no") return;
      }
      dx = ddx;
      // At the first or last page it gives a little, then stops.
      const p = v.viewState().page, last = d.info.pageCount - 1;
      const edge = (dx > 0 && p <= 0) || (dx < 0 && p >= last);
      slide(edge ? dx / 4 : dx, 0);
      if (e.cancelable) e.preventDefault();
    };
    const end = (e: TouchEvent) => {
      if (state !== "side") return;
      state = "";
      const w = el.clientWidth;
      const p = v.viewState().page, last = d.info.pageCount - 1;
      const dir = dx < 0 ? 1 : -1;
      const far = Math.abs(dx) > w * 0.25 || (Math.abs(dx) > 40 && Math.abs(dx) / Math.max(1, e.timeStamp - t0) > 0.4);
      if (!far || (dir < 0 && p <= 0) || (dir > 0 && p >= last)) return slide(0, 180);
      busy = true;
      slide(-dir * w, 160);
      setTimeout(() => {
        // The next page, brought in from the other side.
        el.classList.remove("snap");
        v.stepRow(dir as 1 | -1);
        slide(dir * w, 0);
        void sizer.offsetWidth;
        el.classList.add("snap");
        slide(0, 200);
        setTimeout(() => {
          slide(0, 0);
          busy = false;
        }, 210);
      }, 160);
    };
    el.addEventListener("touchstart", start, { passive: true });
    el.addEventListener("touchmove", move, { passive: false });
    el.addEventListener("touchend", end);
    el.addEventListener("touchcancel", end);
    return () => {
      el.removeEventListener("touchstart", start);
      el.removeEventListener("touchmove", move);
      el.removeEventListener("touchend", end);
      el.removeEventListener("touchcancel", end);
      slide(0, 0);
    };
  }, [v, layout]);

  useTaps(
    () => host.current,
    (x, y) => {
      if (!v) return;
      const r = v.el.getBoundingClientRect();
      if (v.viewState().fit !== "none") v.setZoom(v.zoomLevel * 2.2, x - r.left, y - r.top);
      else v.setFit(fitFor(settings.value.layout));
    },
  );

  return <div class="stage" ref={host} />;
}

// ---------------------------------------------------------------- Read and Book

const FONTS = {
  serif: `"Noto Serif", "Droid Serif", Georgia, serif`,
  sans: `Roboto, "Noto Sans", system-ui, sans-serif`,
};
/** Line length (em) and Book's page margin (share of the page); on a phone the column fills the width, so its side margin (px). */
const MARGINS = { narrow: [32, 0.05, 12], normal: [36, 0.08, 22], wide: [40, 0.12, 40] } as const;

function textVars(t: TextSettings): Record<string, string> {
  const [measure, pad, side] = MARGINS[t.margins];
  return {
    "--rf-font": FONTS[t.font],
    "--rf-size": `${t.size}px`,
    "--rf-lh": String(t.spacing),
    "--rf-align": t.justify ? "justify" : "left",
    "--rf-measure": `${measure}em`,
    "--rf-padk": String(pad),
    "--rf-side": `${side}px`,
  };
}

function Reflow({ d }: { d: OpenDoc }) {
  const host = useRef<HTMLDivElement>(null);
  const m = mode.value as "read" | "book";
  const [flow, setFlow] = useState<Flow | null>(null);
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const t = settings.value.text;
  const paper = t.paper === "sepia" && !settings.value.night ? "sepia" : settings.value.night || isDark.value ? "dark" : "white";

  useEffect(() => {
    let alive = true;
    analysisOf(d.info.docId, d.info.pageCount, page.value, (f) => alive && setProgress(f)).then(
      (a) => alive && setFlow(a.flow(Array.from({ length: d.info.pageCount }, (_, i) => i))),
      (e) => alive && setError(String(e?.message ?? e)),
    );
    return () => {
      alive = false;
    };
  }, [d.info.docId]);

  useEffect(() => {
    if (!flow || !host.current) return;
    const start = flow.pageStart[page.value] ?? 0;
    const cb = {
      onPage: (p: number) => {
        page.value = p;
      },
      onInfo: (i: { page: number; total: number; approx: boolean }) => (bookInfo.value = i),
    };
    const pics = createPictures(d.info.docId);
    const title = d.info.title || d.name.replace(/\.[^.]+$/, "");
    const view: ReflowView = m === "book" ? new BookView(host.current, flow, pics, cb, start, title) : new ReadView(host.current, flow, pics, cb, start);
    views.reflow = view;
    return () => {
      views.reflow = null;
      bookInfo.value = null;
      view.destroy();
      pics.dispose();
    };
  }, [flow, m]);

  useEffect(() => views.reflow?.restyle(), [JSON.stringify(t)]);

  useTaps(() => host.current, undefined, m === "book");

  return (
    <div class={`rf-host paper-${paper} ${m}`} style={textVars(t)}>
      <div class="rf-mount" ref={host} />
      {!flow && !error && (
        <div class="rf-loading">
          <span>{m === "book" ? "Setting the pages…" : "Preparing the text…"}</span>
          <div class="rf-bar">
            <div style={{ width: `${Math.round(progress * 100)}%` }} />
          </div>
        </div>
      )}
      {error && (
        <div class="rf-loading">
          <strong>This document's text can't be reflowed</strong>
          <span>{error}</span>
          <button class="btn" onClick={() => (mode.value = "pages")}>
            Show the pages
          </button>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- bars

function TopBar({ d }: { d: OpenDoc }) {
  const title = d.info.title?.trim() || d.name.replace(/\.[^.]+$/, "");
  return (
    <header class="bar top">
      <button class="ib" aria-label="Back to files" onClick={() => leaveReader()}>
        <ArrowLeft size={22} />
      </button>
      <div class="bar-title">
        <strong>{title}</strong>
      </div>
      <button class="ib" aria-label="Find in document" onClick={openFind}>
        <Search size={21} />
      </button>
      <button class="ib" aria-label="Contents and comments" onClick={() => openSheet("contents")}>
        <ListTree size={21} />
      </button>
      <button class="ib" aria-label="More" onClick={() => openSheet("more")}>
        <EllipsisVertical size={21} />
      </button>
    </header>
  );
}

function openFind() {
  if (searching.value) return;
  searching.value = true;
  chrome.value = true;
  pushLayer(() => {
    searching.value = false;
    clearFind();
  });
}

function FindBar() {
  const input = useRef<HTMLInputElement>(null);
  const [q, setQ] = useState(found.value?.query ?? "");
  useEffect(() => input.current?.focus(), []);
  const f = found.value;
  const count = !f ? "" : f.error ? "Not valid" : f.total === 0 ? (f.done ? "No matches" : "Searching…") : `${f.index + 1} of ${f.total}${f.done ? "" : "+"}`;
  return (
    <header class="bar top find">
      <button class="ib" aria-label="Close find" onClick={() => back()}>
        <ArrowLeft size={22} />
      </button>
      <input
        ref={input}
        type="search"
        enterKeyHint="search"
        autocapitalize="off"
        autocorrect="off"
        spellcheck={false}
        placeholder="Find in document"
        value={q}
        onInput={(e) => {
          const v = (e.target as HTMLInputElement).value;
          setQ(v);
          find(v);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            if (found.value?.query === q.trim()) next();
            else find(q, true);
            input.current?.blur();
          }
        }}
      />
      {q && (
        <button
          class="ib sm"
          aria-label="Clear"
          onClick={() => {
            setQ("");
            clearFind();
            input.current?.focus();
          }}
        >
          <X size={18} />
        </button>
      )}
      <span class="find-count">{count}</span>
      <button class="ib" aria-label="Previous match" disabled={!f?.total} onClick={prev}>
        <ChevronUp size={22} />
      </button>
      <button class="ib" aria-label="Next match" disabled={!f?.total} onClick={next}>
        <ChevronDown size={22} />
      </button>
    </header>
  );
}

function BottomBar({ d }: { d: OpenDoc }) {
  const book = mode.value === "book" ? bookInfo.value : null;
  const total = d.info.pageCount;
  const [drag, setDrag] = useState<number | null>(null);
  const shown = drag ?? page.value;
  const night = settings.value.night;
  // Book mode numbers its own pages; the slider still moves through the document's.
  const label = book && drag === null ? `${book.page + 1}${book.approx ? "+" : ""} / ${book.total}` : `${shown + 1} / ${total}`;
  return (
    <footer class="bar bottom">
      <button class="ib" aria-label="View" onClick={() => openSheet("view")}>
        <SlidersHorizontal size={21} />
      </button>
      <div class="scrub">
        <span class="scrub-label">{label}</span>
        {total > 1 && (
          <input
            type="range"
            min={0}
            max={total - 1}
            value={shown}
            aria-label="Page"
            onInput={(e) => {
              const p = Number((e.target as HTMLInputElement).value);
              setDrag(p);
              if (mode.value === "pages") goTo(p);
            }}
            onChange={(e) => {
              setDrag(null);
              goTo(Number((e.target as HTMLInputElement).value));
              savePosition();
            }}
          />
        )}
      </div>
      <button class="ib" aria-label={night ? "Turn night mode off" : "Night mode"} onClick={() => updateSettings({ night: !night })}>
        {night ? <Sun size={21} /> : <Moon size={21} />}
      </button>
    </footer>
  );
}

/** Read aloud's controls, over the page while it reads (with the bars or without). */
function SpeechBar() {
  const r = reading.value!;
  return (
    <div class="speech" role="group" aria-label="Read aloud">
      <button class="ib" aria-label={r.playing ? "Pause" : "Carry on reading"} onClick={() => (r.playing ? pauseReading() : resumeReading())}>
        {r.playing ? <Pause size={20} /> : <Play size={20} />}
      </button>
      <button class="speech-rate" aria-label="Speed" onClick={nextRate}>
        {r.rate}×
      </button>
      <button class="ib" aria-label="Stop reading" onClick={stopReading}>
        <Square size={18} />
      </button>
    </div>
  );
}

// Keep the place saved as you read.
let saveTimer = 0;
page.subscribe(() => {
  if (!doc.value) return;
  clearTimeout(saveTimer);
  saveTimer = window.setTimeout(savePosition, 1500);
});
