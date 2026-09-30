// Read and Book modes in the UI: loading the reflowed text, hosting the view,
// the text settings panel (Aa) and Book mode's floating bar.
import { useEffect, useRef, useState } from "preact/hooks";
import { signal } from "@preact/signals";
import { AlignJustify, AlignLeft, ChevronLeft, ChevronRight, Minus, Plus, ScrollText, Type, Wrench, X } from "lucide-preact";
import { startLayoutEdit } from "./LayoutPanel";
import { api, newId } from "../api";
import { exitBook, setViewMode } from "../modes";
import { Analysis, type Block, type Flow, type RawPage } from "../reflow/model";
import { overridesOf } from "../reflow/overrides";
import { BookView } from "../reflow/BookView";
import { ReadView, type ReflowView } from "../reflow/ReadView";
import { createPictures, type Pictures } from "../reflow/render";
import { isDark, settings, updateSettings, type ReflowSettings, type Tab } from "../store";
import { mix, themePaper } from "../themes";
import { sourceCount } from "../edits";
import { ocrState, recognize } from "../ocr";
import { readFrom, speaking } from "../speech";

// ---------------------------------------------------------------- loading

interface Loaded {
  docId: number;
  /** The tab's textRev it was read at. */
  rev: number;
  analysis: Promise<Analysis>;
  progress: ReturnType<typeof signal<number>>;
  cancel(): void;
}
const loaded = new WeakMap<Tab, Loaded>();
const pictures = new WeakMap<Tab, { docId: number; pics: Pictures }>();

/**
 * The tab's document analysis: every page's content, extracted once per
 * document (starting at the page you're on). Flows are built from it.
 */
export function analysisOf(tab: Tab): Loaded {
  const info = tab.info.value!;
  const prev = loaded.get(tab);
  if (prev && prev.docId === info.docId && prev.rev === tab.textRev.value) return prev;
  prev?.cancel();
  const progress = signal(0);
  const sid = newId();
  // Pages inserted since opening are part of the document too.
  const count = tab.edits.value ? sourceCount(tab.edits.value) : info.pageCount;
  const pages: (RawPage | undefined)[] = new Array(count);
  let received = 0;
  const analysis = new Promise<Analysis>((resolve, reject) => {
    api
      .reflow(info.docId, tab.edits.value?.order[tab.page.value] ?? 0, sid, (ev) => {
        if (ev.kind === "page") {
          // A page that can't be read is left out rather than stopping the rest.
          try {
            pages[ev.page] = JSON.parse(ev.data) as RawPage;
          } catch (e) {
            console.warn(`reflow: page ${ev.page + 1} left out:`, e);
          }
          progress.value = ++received / count;
        } else if (ev.error) reject(new Error(ev.error));
        else if (!ev.complete) reject(new Error("cancelled"));
        else resolve(new Analysis(pages));
      })
      .catch(reject);
  });
  const entry: Loaded = { docId: info.docId, rev: tab.textRev.value, analysis, progress, cancel: () => api.cancel([sid]).catch(() => {}) };
  analysis.catch(() => loaded.get(tab) === entry && loaded.delete(tab));
  loaded.set(tab, entry);
  return entry;
}

/** The reflowed text for the tab's current page order and your corrections. */
export function flowOf(tab: Tab, a: Analysis): Flow {
  const n = tab.edits.value ? sourceCount(tab.edits.value) : tab.info.value!.pageCount;
  const order = tab.edits.value?.order ?? Array.from({ length: n }, (_, i) => i);
  const t0 = performance.now();
  const f = a.flow(order, overridesOf(tab).value);
  console.debug(`reflow: ${f.blocks.length} blocks from ${n} pages in ${Math.round(performance.now() - t0)} ms`);
  return f;
}

function picturesOf(tab: Tab): Pictures {
  const docId = tab.info.value!.docId;
  const p = pictures.get(tab);
  if (p && p.docId === docId) return p.pics;
  p?.pics.dispose();
  const pics = createPictures(docId);
  pictures.set(tab, { docId, pics });
  return pics;
}

/** Frees a closed tab's reflow data. */
export function releaseReflow(tab: Tab) {
  loaded.get(tab)?.cancel();
  loaded.delete(tab);
  pictures.get(tab)?.pics.dispose();
  pictures.delete(tab);
}

// ---------------------------------------------------------------- typography

const FONTS: Record<ReflowSettings["font"], [string, string]> = {
  sitka: ["Sitka", `"Sitka Text", "Sitka Small", Georgia, serif`],
  georgia: ["Georgia", `Georgia, "Cambria", serif`],
  segoe: ["Segoe UI", `"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif`],
};
const MARGINS = { narrow: [30, 0.06], normal: [36, 0.09], wide: [44, 0.12] } as const;

function paperOf(r: ReflowSettings) {
  if (r.paper !== "auto") return r.paper;
  const t = themePaper.value;
  if (t) return t.dark || settings.value.night ? "theme-dark" : "theme";
  return isDark.value || settings.value.night ? "dark" : "white";
}

/** The theme's paper as the reading view's colours (Auto paper). */
function paperVars(paper: string): Record<string, string> {
  const t = themePaper.value;
  if (!t || !paper.startsWith("theme")) return {};
  const dark = paper === "theme-dark";
  // Dark pages: the theme's dark paper.
  const p = dark ? t.darkPaper : t.now;
  const [bg, ink] = [p.paper, p.ink];
  return { "--rf-paper": bg, "--rf-ink": ink, "--rf-ink-soft": mix(ink, bg, 0.55), "--rf-book-desk": mix(bg, "#000000", dark ? 0.55 : 0.25) };
}

function styleVars(r: ReflowSettings): Record<string, string> {
  const [measure, pad] = MARGINS[r.margins];
  return {
    "--rf-font": FONTS[r.font][1],
    "--rf-size": `${r.size}px`,
    "--rf-lh": String(r.spacing),
    "--rf-align": r.justify ? "justify" : "left",
    "--rf-measure": `${measure}em`,
    "--rf-padk": String(pad),
  };
}

export function updateReflow(patch: Partial<ReflowSettings>) {
  updateSettings({ reflow: { ...settings.value.reflow, ...patch } });
}

export const typeOpen = signal(false);

export function zoomText(dir: 1 | -1 | 0) {
  const size = settings.value.reflow.size;
  updateReflow({ size: dir === 0 ? 19 : Math.max(12, Math.min(34, size + dir)) });
}

// ---------------------------------------------------------------- host

export function ReflowHost({ tab }: { tab: Tab }) {
  const host = useRef<HTMLDivElement>(null);
  const mode = tab.mode.value as "read" | "book";
  const [state, setState] = useState<{ flow?: Flow; error?: string; loading?: Loaded }>({});
  const ov = overridesOf(tab).value;
  const r = settings.value.reflow;
  const paper = paperOf(r);

  // Load (or reuse) the reflowed text.
  useEffect(() => {
    const l = analysisOf(tab);
    let alive = true;
    setState((s) => (s.flow ? s : { loading: l }));
    l.analysis.then(
      (a) => alive && setState({ flow: flowOf(tab, a) }),
      (e) => alive && setState({ error: String(e?.message ?? e) }),
    );
    return () => {
      alive = false;
    };
  }, [tab.info.value?.docId, tab.edits.value, ov, tab.textRev.value]);

  // Mount the view for the current mode.
  const flow = state.flow;
  useEffect(() => {
    if (!flow || !host.current) return;
    const start = tab.reflowBlock >= 0 ? tab.reflowBlock : flow.pageStart[tab.page.value] ?? 0;
    const cb = {
      onPage: (p: number) => (tab.page.value = p),
      onInfo: (i: { page: number; total: number; approx: boolean }) => (tab.bookInfo.value = i),
    };
    const pics = picturesOf(tab);
    const view: ReflowView =
      mode === "book" ? new BookView(host.current, flow, pics, cb, start, tab.info.value?.title || tab.name.replace(/\.[^.]+$/, "")) : new ReadView(host.current, flow, pics, cb, start);
    tab.reflowView = view;
    view.focus();
    return () => {
      tab.reflowBlock = view.currentBlock();
      view.setHits([], null);
      tab.flowSearch.value = null;
      tab.reflowView = null;
      tab.bookInfo.value = null;
      view.destroy();
    };
  }, [flow, mode]);

  // Typography: CSS variables on the host; the view keeps its place.
  const vars = { ...styleVars(r), ...paperVars(paper) };
  useEffect(() => tab.reflowView?.restyle(), [JSON.stringify(r)]);

  // Ctrl+wheel / pinch changes the text size.
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    let acc = 0;
    const wheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      acc += e.deltaY;
      if (Math.abs(acc) >= 40) {
        zoomText(acc < 0 ? 1 : -1);
        acc = 0;
      }
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => el.removeEventListener("wheel", wheel);
  }, []);

  // Click a picture (not while selecting text) to see it large.
  const [zoomed, setZoomed] = useState<Extract<Block, { k: "img" }> | null>(null);
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const click = (e: MouseEvent) => {
      const fig = (e.target as HTMLElement).closest<HTMLElement>("figure.rf-img");
      const b = fig && (fig as HTMLElement & { _pic?: Block })._pic;
      if (!b || b.k !== "img" || getSelection()?.toString()) {
        // While reading aloud: a click on a paragraph carries on from there.
        const at = (e.target as HTMLElement).closest<HTMLElement>("[data-b]");
        // Book mode's edges turn the page (see BookView.onPointerDown): not a place to read from.
        const book = el.querySelector(".rf-book")?.getBoundingClientRect();
        const turning = !!book && (e.clientX < book.left + book.width * 0.14 || e.clientX > book.right - book.width * 0.14);
        if (!fig && at && !turning && speaking.value?.tab === tab && !getSelection()?.toString()) readFrom(tab, Number(at.dataset.b));
        return;
      }
      e.stopPropagation();
      setZoomed(b);
    };
    el.addEventListener("click", click, true);
    return () => el.removeEventListener("click", click, true);
  }, []);

  const progress = state.loading?.progress.value ?? 0;
  return (
    <div class={`rf-host ${paper === "theme-dark" ? "paper-dark paper-theme" : `paper-${paper}`} ${mode}`} style={vars}>
      <div class="rf-mount" ref={host} />
      {!flow && !state.error && (
        <div class="rf-loading">
          <span>Preparing {mode === "book" ? "the book" : "reading view"}…</span>
          <div class="rf-bar">
            <div style={{ width: `${Math.round(progress * 100)}%` }} />
          </div>
        </div>
      )}
      {state.error && (
        <div class="rf-loading">
          <strong>Couldn't reflow this document</strong>
          <span class="error-text">{state.error}</span>
        </div>
      )}
      {flow && flow.scans > 0 && flow.scans >= (flow.pageStart.length * 0.8) && <ScanNote tab={tab} />}
      {mode === "book" && flow && <BookBar tab={tab} />}
      {typeOpen.value && <TypePanel book={mode === "book"} />}
      {zoomed && <Lightbox pic={zoomed} pics={picturesOf(tab)} onClose={() => setZoomed(null)} />}
    </div>
  );
}

function ScanNote({ tab }: { tab: Tab }) {
  const [shown, setShown] = useState(true);
  if (!shown) return null;
  const running = ocrState(tab).value?.phase === "running";
  return (
    <div class="rf-note">
      {running ? "Recognizing the text of this scan… Its pages are shown as pictures until it's done." : "This is a scanned document, so its pages are shown as pictures until its text is recognized."}
      {!running && (
        <button class="btn sm pri" onClick={() => void recognize(tab)}>
          Recognize text
        </button>
      )}
      <button class="ib sm" aria-label="Dismiss" onClick={() => setShown(false)}>
        <X size={14} />
      </button>
    </div>
  );
}

// ---------------------------------------------------------------- picture viewer

function Lightbox({ pic, pics, onClose }: { pic: Extract<Block, { k: "img" }>; pics: Pictures; onClose: () => void }) {
  const [url, setUrl] = useState("");
  useEffect(() => {
    const [x0, y0, x1, y1] = pic.box;
    // As large as fits the window, rendered sharp for it.
    const w = Math.min(window.innerWidth * 0.94, (window.innerHeight * 0.92 * (x1 - x0)) / (y1 - y0));
    pics.url(pic, w).then(setUrl).catch(() => onClose());
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [pic]);
  return (
    <div class="rf-lightbox" onPointerDown={(e) => e.stopPropagation()} onClick={onClose}>
      {url ? <img src={url} alt="" draggable={false} /> : <span class="spin" />}
    </div>
  );
}

// ---------------------------------------------------------------- Aa panel

export function TypePanel({ book }: { book: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const down = (e: PointerEvent) => {
      const t = e.target as HTMLElement;
      if (!ref.current?.contains(t) && !t.closest(".type-btn")) typeOpen.value = false;
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && (typeOpen.value = false);
    window.addEventListener("pointerdown", down);
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("pointerdown", down);
      window.removeEventListener("keydown", key, true);
    };
  }, []);
  return (
    <div class={`type-panel ${book ? "in-book" : ""}`} ref={ref} onPointerDown={(e) => e.stopPropagation()}>
      <TypeControls />
    </div>
  );
}

/** Text size, font, spacing, margins, alignment and paper for Read and Book modes (also in Settings). */
export function TypeControls() {
  const r = settings.value.reflow;
  const seg = <K extends keyof ReflowSettings>(key: K, opts: [ReflowSettings[K], preact.ComponentChild, string?][]) => (
    <div class="seg">
      {opts.map(([v, label, title]) => (
        <button key={String(v)} class={`segb ${r[key] === v ? "on" : ""}`} title={title} onClick={() => updateReflow({ [key]: v } as Partial<ReflowSettings>)}>
          {label}
        </button>
      ))}
    </div>
  );
  return (
    <>
      <div class="tp-row">
        <button class="ib sm" aria-label="Smaller text" onClick={() => zoomText(-1)}>
          <Minus size={15} />
        </button>
        <span class="tp-size">{r.size}px</span>
        <button class="ib sm" aria-label="Larger text" onClick={() => zoomText(1)}>
          <Plus size={15} />
        </button>
      </div>
      <label>Font</label>
      {seg("font", (Object.keys(FONTS) as ReflowSettings["font"][]).map((f) => [f, <span style={{ fontFamily: FONTS[f][1] }}>{FONTS[f][0]}</span>]))}
      <label>Line spacing</label>
      {seg("spacing", [[1.35, "Tight"], [1.55, "Normal"], [1.8, "Loose"]])}
      <label>Margins</label>
      {seg("margins", [["narrow", "Narrow"], ["normal", "Normal"], ["wide", "Wide"]])}
      <label>Alignment</label>
      {seg("justify", [[true, <AlignJustify size={15} />, "Justified"], [false, <AlignLeft size={15} />, "Left"]])}
      <label>Paper</label>
      {seg("paper", [["auto", "Auto", "Follows the theme and dark pages"], ["white", <span class="sw white" />, "White"], ["sepia", <span class="sw sepia" />, "Sepia"], ["dark", <span class="sw dark" />, "Dark"]])}
    </>
  );
}

// ---------------------------------------------------------------- Book mode bar

function BookBar({ tab }: { tab: Tab }) {
  const [shown, setShown] = useState(true);
  const over = useRef(false);
  useEffect(() => {
    let timer = window.setTimeout(() => !typeOpen.value && setShown(false), 2500);
    const move = (e: PointerEvent) => {
      // Show when the mouse moves near the top, or at all while a panel is open.
      if (e.clientY > 140 && !over.current && !typeOpen.value) return;
      setShown(true);
      clearTimeout(timer);
      timer = window.setTimeout(() => !over.current && !typeOpen.value && setShown(false), 2200);
    };
    window.addEventListener("pointermove", move);
    return () => {
      clearTimeout(timer);
      window.removeEventListener("pointermove", move);
    };
  }, []);
  const i = tab.bookInfo.value;
  const v = tab.reflowView;
  return (
    <div class={`readbar ${shown || typeOpen.value ? "" : "hidden"}`} onPointerEnter={() => (over.current = true)} onPointerLeave={() => (over.current = false)}>
      <button class="ib" aria-label="Previous page" title="Previous (← or Page Up)" onClick={() => v?.prev()}>
        <ChevronLeft size={18} strokeWidth={1.7} />
      </button>
      <span class="rb-page">{i ? `${i.page}–${Math.min(i.page + 1, i.total)} of ${i.approx ? "~" : ""}${i.total.toLocaleString()}` : ""}</span>
      <button class="ib" aria-label="Next page" title="Next (→, Space or Page Down)" onClick={() => v?.next()}>
        <ChevronRight size={18} strokeWidth={1.7} />
      </button>
      <div class="sep" />
      <button class={`ib type-btn ${typeOpen.value ? "on" : ""}`} aria-label="Text settings" title="Text size, font, spacing, paper" onClick={() => (typeOpen.value = !typeOpen.value)}>
        <Type size={18} strokeWidth={1.7} />
      </button>
      <button class="ib" aria-label="Fix layout" title="Fix layout: correct columns, reading order, pictures and text" onClick={() => void startLayoutEdit(tab)}>
        <Wrench size={18} strokeWidth={1.7} />
      </button>
      <button class="ib" aria-label="Read mode" title="Read mode (one scrolling column)" onClick={() => void setViewMode(tab, "read")}>
        <ScrollText size={18} strokeWidth={1.7} />
      </button>
      <div class="sep" />
      <button class="tb" title="Leave Book mode (Esc)" onClick={exitBook}>
        <X size={16} />
        <span>Exit</span>
      </button>
    </div>
  );
}
