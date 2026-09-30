// Compare: the setup dialog, and the Compare tab (the two files side by side with what differs marked, and the
// list of differences).
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { ComponentChildren } from "preact";
import { open } from "@tauri-apps/plugin-dialog";
import { ArrowLeftRight, ChevronDown, ChevronUp, Link2, Link2Off, Minus, MoveHorizontal, Plus, X } from "lucide-preact";
import { OPENABLE } from "../actions";
import type { ChangeKind, CompareChange, CompareMarks, TextBlock } from "../api";
import {
  closeCompare, compare, compareAgain, compareOptions, comparePassword, compareSel, compareSetup, compareSync, readsText, setCompareOption,
  startCompare, stopCompare, swapSides, type CompareSession, type CompareSide,
} from "../compare";
import { OFFICE_EXT } from "../office";
import { Viewer, type PageLayer } from "../render/Viewer";
import { baseName, settings, tabs } from "../store";
import { testHook } from "./PagesUI";

// ---------------------------------------------------------------- setup

async function pickFile(title: string): Promise<string | null> {
  const hook = testHook()?.pick;
  if (hook) return hook.shift() ?? null;
  const res = await open({
    multiple: false,
    title,
    filters: [
      { name: "Documents", extensions: OPENABLE },
      { name: "PDF", extensions: ["pdf"] },
      { name: "Office documents", extensions: OFFICE_EXT },
      { name: "Markdown", extensions: ["md", "markdown"] },
    ],
  });
  return typeof res === "string" ? res : null;
}

export function CompareDialog() {
  const st = compareSetup.value;
  useEffect(() => {
    if (!st) return;
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        compareSetup.value = null;
      }
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [!!st]);
  if (!st) return null;
  const opts = compareOptions();
  const set = (side: "a" | "b", path: string | null) => (compareSetup.value = { ...st, [side]: path });
  const open_ = tabs.value.filter((t) => t.status.value === "ready").slice(0, 8);
  const row = (side: "a" | "b", label: string, note: string) => {
    const path = st[side];
    return (
      <div class="tool-field">
        <label>{label}</label>
        <div class="cmp-pick">
          <span class={`cmp-file ${path ? "" : "none"}`} title={path ?? ""}>
            {path ? baseName(path) : "No file chosen"}
          </span>
          <button class="btn sm" onClick={async () => set(side, (await pickFile(label)) ?? path)}>
            Choose…
          </button>
        </div>
        {open_.length > 0 && (
          <div class="cmp-chips">
            {open_.map((t) => (
              <button key={t.key} class={`cmp-chip ${path === t.path ? "on" : ""}`} title={t.path} onClick={() => set(side, t.path)}>
                {t.name}
              </button>
            ))}
          </div>
        )}
        <span class="hint">{note}</span>
      </div>
    );
  };
  const both = st.a && st.b;
  const textOnly = [st.a, st.b].some((p) => p && readsText(p));
  return (
    <div class="overlay" onPointerDown={(ev) => ev.target === ev.currentTarget && (compareSetup.value = null)}>
      <div class="dialog tool-dialog cmp-dialog" role="dialog" aria-label="Compare files">
        <button class="ib sm close" aria-label="Close" onClick={() => (compareSetup.value = null)}>
          <X size={16} />
        </button>
        <h2>Compare files</h2>
        <span class="hint">
          Two versions of a PDF, or a PDF with the Word, PowerPoint, Excel or Markdown file it was made from: every word that was changed, added,
          removed or moved, side by side.
        </span>
        {row("a", "Original", "Shown on the left.")}
        {row("b", "Changed", "Shown on the right.")}
        <div class="tool-field">
          <label>Options</label>
          <Check on={opts.matchCase} label="Capitals count (“Apple” and “apple” differ)" set={(v) => setCompareOption({ matchCase: v })} />
          <Check on={opts.skipEdges} label="Leave out running headers, footers and page numbers" set={(v) => setCompareOption({ skipEdges: v })} />
          <Check on={opts.pictures} label="Compare drawings and scans too (two PDFs with pages of the same size)" set={(v) => setCompareOption({ pictures: v })} />
          {textOnly && <span class="hint">Word, PowerPoint and Excel files are compared by their words: no two programs lay them out quite the same way.</span>}
        </div>
        <div class="row">
          <button class="btn" onClick={() => (compareSetup.value = null)}>
            Cancel
          </button>
          <button
            class="btn pri"
            disabled={!both || st.a === st.b}
            title={both && st.a === st.b ? "Choose two different files" : ""}
            onClick={() => {
              compareSetup.value = null;
              void startCompare(st.a!, st.b!);
            }}
          >
            Compare
          </button>
        </div>
      </div>
    </div>
  );
}

function Check({ on, label, set }: { on: boolean; label: string; set: (v: boolean) => void }) {
  return (
    <label class="check-row">
      <input type="checkbox" checked={on} onChange={(e) => set((e.target as HTMLInputElement).checked)} />
      {label}
    </label>
  );
}

// ---------------------------------------------------------------- scrolling together

type Side = "a" | "b";

interface Pane {
  /** The place `offset` px below the top of the pane: [page or block, y]. */
  point(offset: number): [number, number];
  /** Scrolls a place to `offset` px below the top. */
  go(pos: [number, number], offset: number): void;
  /** Scrolls to a change's marks. */
  show(m: CompareMarks, i: number): void;
  height(): number;
  isDoc: boolean;
}

const panes: Record<Side, Pane | null> = { a: null, b: null };
const quiet: Record<Side, number> = { a: 0, b: 0 };
let anchorsBy: Record<Side, [number, number, number, number][]> = { a: [], b: [] };

function setAnchors(list: [number, number, number, number][]) {
  const by = (k: 0 | 2) => list.slice().sort((x, y) => x[k] - y[k] || x[k + 1] - y[k + 1]);
  anchorsBy = { a: by(0), b: by(2) };
}

/** The other side's place for a place on `side`, from the nearest matching words before it. */
function across(side: Side, pos: [number, number]): [number, number] | null {
  const list = anchorsBy[side];
  const k = side === "a" ? 0 : 2;
  const o = side === "a" ? 2 : 0;
  let lo = 0, hi = list.length - 1, best = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    const m = list[mid];
    if (m[k] < pos[0] || (m[k] === pos[0] && m[k + 1] <= pos[1])) {
      best = mid;
      lo = mid + 1;
    } else hi = mid - 1;
  }
  if (best < 0) return list.length ? [list[0][o], 0] : null;
  const m = list[best];
  const other = side === "a" ? panes.b : panes.a;
  const docs = panes[side]?.isDoc && other?.isDoc;
  // Within a page, the rest of the way down follows (text read from a file only has blocks).
  if (m[k] === pos[0] || !docs) return [m[o], m[o + 1] + (docs ? pos[1] - m[k + 1] : 0)];
  // Pages without words after it: the pages go on together, up to the next words that match.
  const guess: [number, number] = [m[o] + (pos[0] - m[k]), pos[1]];
  const n = list[best + 1];
  return n && (guess[0] > n[o] || (guess[0] === n[o] && guess[1] > n[o + 1])) ? [n[o], n[o + 1]] : guess;
}

function scrolled(side: Side) {
  if (!compareSync.value || performance.now() < quiet[side]) return;
  const other: Side = side === "a" ? "b" : "a";
  const p = panes[side], q = panes[other];
  if (!p || !q) return;
  const off = p.height() / 3;
  const target = across(side, p.point(off));
  if (!target) return;
  quiet[other] = performance.now() + 150;
  q.go(target, off);
}

/** Picks a change and scrolls both sides to it (only the other side when it was clicked on one). */
function pick(i: number, from?: Side) {
  compareSel.value = i;
  const c = compare.value?.outcome?.changes[i];
  if (!c) return;
  const now = performance.now();
  quiet.a = quiet.b = now + 250;
  if (from !== "a") panes.a?.show(c.a, i);
  if (from !== "b") panes.b?.show(c.b, i);
}

function step(dir: 1 | -1) {
  const n = compare.value?.outcome?.changes.length ?? 0;
  if (!n) return;
  const cur = compareSel.value;
  pick(cur < 0 ? (dir > 0 ? 0 : n - 1) : (cur + dir + n) % n);
}

// ---------------------------------------------------------------- the Compare tab

const KIND: Record<ChangeKind, string> = { changed: "Changed", added: "Added", removed: "Removed", moved: "Moved", picture: "Picture or drawing" };

export function CompareView({ active }: { active: boolean }) {
  const s = compare.value;
  useEffect(() => {
    if (!active) return;
    const key = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement)?.closest?.("input, textarea")) return;
      if (e.key === "F7" || (e.altKey && (e.key === "ArrowDown" || e.key === "ArrowUp"))) {
        e.preventDefault();
        e.stopPropagation();
        step(e.key === "ArrowUp" || e.shiftKey ? -1 : 1);
      }
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [active]);
  if (!s) return null;
  return (
    <div class="cmp" style={{ display: active ? "flex" : "none" }}>
      <CompareBar s={s} />
      <div class="cmp-body">
        <PaneBox side="a" s={s} active={active} />
        <PaneBox side="b" s={s} active={active} />
        <ChangeList s={s} />
      </div>
      {comparePassword.value && <PasswordAsk />}
    </div>
  );
}

function CompareBar({ s }: { s: CompareSession }) {
  const opts = compareOptions();
  const busy = s.state === "opening" || s.state === "comparing";
  const zoom = (f: number | "fit") => {
    for (const p of [panes.a, panes.b]) (p as DocPaneApi | null)?.zoom?.(f);
  };
  const docs = !!(s.a.info || s.b.info);
  return (
    <div class="cmp-bar">
      <div class="cmp-names">
        <span class="cmp-name a" title={s.a.path}>
          {s.a.name}
        </span>
        <button class="ib sm" title="Swap sides" aria-label="Swap sides" disabled={busy} onClick={swapSides}>
          <ArrowLeftRight size={15} />
        </button>
        <span class="cmp-name b" title={s.b.path}>
          {s.b.name}
        </span>
      </div>
      <div class="cmp-status">
        {busy ? (
          <>
            <span class="spin" />
            <span>{s.progress}</span>
            {s.fraction >= 0 && (
              <div class="rf-bar cmp-progress">
                <div style={{ width: `${Math.round(s.fraction * 100)}%` }} />
              </div>
            )}
            <button class="btn ghost sm" onClick={stopCompare}>
              Stop
            </button>
          </>
        ) : s.state === "error" ? (
          <>
            <span class="error-text">{s.error}</span>
            <button class="btn sm" onClick={() => compareAgain()}>
              Compare again
            </button>
          </>
        ) : (
          <Summary s={s} />
        )}
      </div>
      <div class="cmp-tools">
        <button class={`cmp-chip ${opts.matchCase ? "on" : ""}`} disabled={busy} title="Capitals count" onClick={() => compareAgain({ matchCase: !opts.matchCase })}>
          Aa
        </button>
        <button class={`cmp-chip ${opts.skipEdges ? "" : "on"}`} disabled={busy} title="Compare running headers, footers and page numbers too" onClick={() => compareAgain({ skipEdges: !opts.skipEdges })}>
          Headers
        </button>
        {s.a.info && s.b.info && (
          <button class={`cmp-chip ${opts.pictures ? "on" : ""}`} disabled={busy} title="Compare drawings and scans too" onClick={() => compareAgain({ pictures: !opts.pictures })}>
            Drawings
          </button>
        )}
        <div class="sep" />
        <button class={`ib sm ${compareSync.value ? "on" : ""}`} title={compareSync.value ? "Scrolling together (click to scroll each side on its own)" : "Scroll the sides together"} aria-label="Scroll together" onClick={() => (compareSync.value = !compareSync.value)}>
          {compareSync.value ? <Link2 size={15} /> : <Link2Off size={15} />}
        </button>
        {docs && (
          <>
            <button class="ib sm" title="Zoom out" aria-label="Zoom out" onClick={() => zoom(1 / 1.2)}>
              <Minus size={15} />
            </button>
            <button class="ib sm" title="Fit width" aria-label="Fit width" onClick={() => zoom("fit")}>
              <MoveHorizontal size={15} />
            </button>
            <button class="ib sm" title="Zoom in" aria-label="Zoom in" onClick={() => zoom(1.2)}>
              <Plus size={15} />
            </button>
          </>
        )}
        <div class="sep" />
        <button class="btn ghost sm" onClick={() => (compareSetup.value = { a: s.a.path, b: s.b.path })}>
          Other files…
        </button>
        <button class="ib sm" title="Close the comparison" aria-label="Close the comparison" onClick={closeCompare}>
          <X size={15} />
        </button>
      </div>
    </div>
  );
}

function Summary({ s }: { s: CompareSession }) {
  const out = s.outcome;
  if (!out) return null;
  const n = out.changes.length;
  const count = (k: ChangeKind) => out.changes.filter((c) => c.kind === k).length;
  const parts = (["changed", "added", "removed", "moved", "picture"] as ChangeKind[]).map((k) => [k, count(k)] as const).filter(([, c]) => c > 0);
  return (
    <>
      <strong>{n === 0 ? "No differences" : n === 1 ? "1 difference" : `${n} differences`}</strong>
      {parts.map(([k, c]) => (
        <span key={k} class={`cmp-count k-${k}`}>
          {c} {KIND[k].toLowerCase()}
        </span>
      ))}
      {out.rough && <span class="hint" title="The files differ so much that the comparison was cut short: differences are shown in large pieces.">(rough)</span>}
    </>
  );
}

function PasswordAsk() {
  const q = comparePassword.value!;
  const [pw, setPw] = useState("");
  return (
    <div class="overlay" style={{ position: "absolute" }}>
      <form
        class="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          q.answer(pw);
        }}
      >
        <h2>{q.name} is protected</h2>
        <span class="muted">{q.wrong ? "That password didn't work. Try again." : "Type its password to compare it."}</span>
        <input class="inp" type="password" autoFocus value={pw} onInput={(e) => setPw((e.target as HTMLInputElement).value)} onKeyDown={(e) => e.stopPropagation()} />
        <div class="row">
          <button type="button" class="btn" onClick={() => q.answer(null)}>
            Cancel
          </button>
          <button class="btn pri">Open</button>
        </div>
      </form>
    </div>
  );
}

// ---------------------------------------------------------------- the two sides

function PaneBox({ side, s, active }: { side: Side; s: CompareSession; active: boolean }) {
  const d: CompareSide = s[side];
  const out = s.outcome;
  const blocks = out ? (side === "a" ? out.aBlocks : out.bBlocks) : null;
  let body: ComponentChildren;
  if (d.info && d.sizes) body = <DocPane key={`${d.info.docId}`} side={side} d={d} active={active} />;
  else if (blocks) body = <TextPane key={`${side}:${s.run}`} side={side} blocks={blocks} />;
  else body = <div class="state-msg">{s.state === "error" ? "" : readsText(d.path) ? "Reading its words…" : "Opening…"}</div>;
  return (
    <div class={`cmp-side side-${side}`}>
      <div class="cmp-side-head" title={d.path}>
        <span class={`cmp-dot side-${side}`} />
        {d.name}
        {!d.info && readsText(d.path) && <span class="hint"> · its words</span>}
        {d.info?.converted && <span class="hint"> · as made by {d.info.converted.engineName}</span>}
      </div>
      <div class="cmp-pane">{body}</div>
    </div>
  );
}

interface DocPaneApi extends Pane {
  zoom(f: number | "fit"): void;
}

function DocPane({ side, d, active }: { side: Side; d: CompareSide; active: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  const viewer = useRef<Viewer | null>(null);
  const out = compare.value?.outcome ?? null;
  const run = compare.value?.run;
  const sel = compareSel.value;

  useEffect(() => {
    const info = d.info!;
    const v = new Viewer(host.current!, info, { onPage() {}, onZoom() {}, onSelection() {}, onPageAction() {}, onScroll: () => scrolled(side) });
    v.setPages(d.sizes!.map(([w, h], src) => ({ src, rot: 0, w, h })));
    v.restore({ page: 0, frac: 0, zoom: 1, fit: "width" });
    viewer.current = v;
    const api: DocPaneApi = {
      isDoc: true,
      point: (off) => {
        const p = v.pointAt(off);
        return [p.page, p.y];
      },
      go: ([page, y], off) => v.scrollToPoint(page, y, off),
      show: (m) => {
        const b = m.boxes[0];
        v.scrollToPoint(b ? b[0] : m.at[0], b ? b[2] : m.at[1], v.el.clientHeight / 3);
      },
      height: () => v.el.clientHeight,
      zoom: (f) => (f === "fit" ? v.setFit("width") : v.zoomBy(f)),
    };
    panes[side] = api;
    return () => {
      if (panes[side] === api) panes[side] = null;
      v.destroy();
      viewer.current = null;
    };
  }, []);

  useEffect(() => viewer.current?.setActive(active), [active]);
  const night = settings.value.night;
  useEffect(() => viewer.current?.setNight(night), [night]);

  // The marks: boxes over what differs, per page.
  const layer = useMemo<PageLayer | null>(() => {
    if (!out) return null;
    const byPage = new Map<number, [number, number[]][]>();
    out.changes.forEach((c, i) => {
      for (const b of c[side].boxes) {
        const l = byPage.get(b[0]) ?? [];
        l.push([i, b]);
        byPage.set(b[0], l);
      }
    });
    return {
      z: 4,
      mount(page) {
        const list = byPage.get(page.src);
        if (!list) return null;
        const el = document.createElement("div");
        for (const [i, b] of list) {
          const m = document.createElement("div");
          const caret = b[3] - b[1] < 0.5;
          m.className = `cmp-mark side-${side} k-${out.changes[i].kind}${caret ? " caret" : ""}${i === compareSel.peek() ? " on" : ""}`;
          m.dataset.i = String(i);
          m.style.left = `${b[1]}px`;
          m.style.top = `${b[2]}px`;
          m.style.width = `${caret ? 0 : b[3] - b[1]}px`;
          m.style.height = `${b[4] - b[2]}px`;
          m.addEventListener("pointerdown", (e) => {
            e.stopPropagation();
            pick(i, side);
          });
          el.append(m);
        }
        return el;
      },
    };
  }, [out, run]);
  useEffect(() => {
    const v = viewer.current;
    if (!v || !layer) return;
    v.addLayer(layer);
    return () => v.removeLayer(layer);
  }, [layer]);
  // The picked change stands out.
  useEffect(() => {
    const root = host.current;
    if (!root) return;
    root.querySelectorAll(".cmp-mark.on").forEach((e) => e.classList.remove("on"));
    if (sel >= 0) root.querySelectorAll(`.cmp-mark[data-i="${sel}"]`).forEach((e) => e.classList.add("on"));
  }, [sel, layer]);

  return <div class="cmp-viewer" ref={host} />;
}

/** Text read from an Office file: paragraphs, slides, sheets with their rows, what differs marked. */
function TextPane({ side, blocks }: { side: Side; blocks: TextBlock[] }) {
  const root = useRef<HTMLDivElement>(null);
  const out = compare.value?.outcome;
  const sel = compareSel.value;
  // Marks per block and cell: [from, to, change].
  const marks = useMemo(() => {
    const m = new Map<string, [number, number, number][]>();
    out?.changes.forEach((c, i) => {
      for (const [b, cell, from, to] of c[side].spans) {
        const k = `${b}:${cell}`;
        const l = m.get(k) ?? [];
        l.push([from, to, i]);
        m.set(k, l);
      }
    });
    for (const l of m.values()) l.sort((x, y) => x[0] - y[0]);
    return m;
  }, [out]);

  const text = (b: number, cell: number, t: string) => {
    const list = marks.get(`${b}:${cell}`);
    if (!list) return t;
    const parts: ComponentChildren[] = [];
    let at = 0;
    for (const [from, to, i] of list) {
      if (from < at) continue;
      if (from > at) parts.push(t.slice(at, from));
      const kind = out!.changes[i].kind;
      parts.push(
        from === to ? (
          <span key={`${i}:${from}`} class={`cmp-caret side-${side} k-${kind}`} data-i={i} />
        ) : (
          <mark key={`${i}:${from}`} class={`cmp-mark-t side-${side} k-${kind}`} data-i={i}>
            {t.slice(from, to)}
          </mark>
        ),
      );
      at = to;
    }
    parts.push(t.slice(at));
    return parts;
  };

  const body = useMemo(() => {
    const out: ComponentChildren[] = [];
    let rows: number[] = [];
    const cellClass = (t: string) => (t.length > 40 ? "long" : "");
    // A table's rows; a spreadsheet's as a grid with its column letters and row numbers.
    const flush = () => {
      if (!rows.length) return;
      const key = `t${rows[0]}`;
      const sheet = rows.every((bi) => blocks[bi].refs?.length);
      if (!sheet) {
        out.push(
          <table class="cmp-table" key={key}>
            <tbody>
              {rows.map((bi) => (
                <tr key={bi} data-b={bi}>
                  {blocks[bi].t.map((t, ci) => (
                    <td key={ci} class={cellClass(t)}>
                      {text(bi, ci, t)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>,
        );
      } else {
        const col = (r: string) => r.replace(/[0-9]+$/, "");
        const num = (c: string) => [...c].reduce((n, ch) => n * 26 + ch.charCodeAt(0) - 64, 0);
        const cols = [...new Set(rows.flatMap((bi) => blocks[bi].refs!.map(col)))].sort((x, y) => num(x) - num(y));
        out.push(
          <table class="cmp-table grid" key={key}>
            <thead>
              <tr>
                <th />
                {cols.map((c) => (
                  <th key={c}>{c}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((bi) => {
                const b = blocks[bi];
                const at = new Map(b.refs!.map((r, ci) => [col(r), ci]));
                return (
                  <tr key={bi} data-b={bi}>
                    <th>{b.refs![0].replace(/^[A-Z]+/, "")}</th>
                    {cols.map((c) => {
                      const ci = at.get(c);
                      return ci === undefined ? <td key={c} /> : <td key={c} class={cellClass(b.t[ci])} title={b.refs![ci]}>{text(bi, ci, b.t[ci])}</td>;
                    })}
                  </tr>
                );
              })}
            </tbody>
          </table>,
        );
      }
      rows = [];
    };
    blocks.forEach((b, bi) => {
      if (b.k === "row") {
        rows.push(bi);
        return;
      }
      flush();
      if (b.k === "sheet") out.push(<h2 key={bi} class="cmp-head" data-b={bi}>{b.t[0]}</h2>);
      else if (b.k === "slide") out.push(<h2 key={bi} class="cmp-head" data-b={bi}>{b.t[0]}</h2>);
      else if (b.k === "h") out.push(<h3 key={bi} data-b={bi}>{text(bi, 0, b.t[0])}</h3>);
      else out.push(<p key={bi} data-b={bi}>{text(bi, 0, b.t[0])}</p>);
    });
    flush();
    return out;
  }, [blocks, marks]);

  useEffect(() => {
    const el = root.current!;
    const top = (e: Element) => e.getBoundingClientRect().top - el.getBoundingClientRect().top + el.scrollTop;
    const all = () => Array.from(el.querySelectorAll<HTMLElement>("[data-b]"));
    let cache: HTMLElement[] | null = null;
    const list = () => (cache ??= all());
    const api: Pane = {
      isDoc: false,
      point: (off) => {
        const l = list();
        const y = el.scrollTop + off;
        let lo = 0, hi = l.length - 1, best = 0;
        while (lo <= hi) {
          const mid = (lo + hi) >> 1;
          if (top(l[mid]) <= y) {
            best = mid;
            lo = mid + 1;
          } else hi = mid - 1;
        }
        return [Number(l[best]?.dataset.b ?? 0), 0];
      },
      go: ([b], off) => {
        const e = el.querySelector(`[data-b="${b}"]`);
        if (e) el.scrollTop = Math.max(0, top(e) - off);
      },
      show: (m, i) => {
        const e = el.querySelector(`[data-i="${i}"]`) ?? el.querySelector(`[data-b="${m.at[0]}"]`);
        if (e) el.scrollTop = Math.max(0, top(e) - el.clientHeight / 3);
      },
      height: () => el.clientHeight,
    };
    panes[side] = api;
    return () => {
      if (panes[side] === api) panes[side] = null;
    };
  }, []);
  useEffect(() => {
    const el = root.current;
    if (!el) return;
    el.querySelectorAll(".on[data-i]").forEach((e) => e.classList.remove("on"));
    if (sel >= 0) el.querySelectorAll(`[data-i="${sel}"]`).forEach((e) => e.classList.add("on"));
  }, [sel, body]);

  return (
    <div
      class="cmp-text"
      ref={root}
      onScroll={() => scrolled(side)}
      onPointerDown={(e) => {
        const m = (e.target as HTMLElement).closest<HTMLElement>("[data-i]");
        if (m) pick(Number(m.dataset.i), side);
      }}
    >
      <div class="cmp-sheet">{body}</div>
    </div>
  );
}

// ---------------------------------------------------------------- list of differences

function where(s: CompareSession, c: CompareChange) {
  if (c.place) return c.place;
  const page = (side: Side) => (s[side].info ? c[side].at[0] + 1 : null);
  const pa = page("a"), pb = page("b");
  if (pa !== null && pb !== null) return pa === pb ? `Page ${pa}` : `Page ${pa} · ${pb}`;
  const p = pa ?? pb;
  return p !== null ? `Page ${p}` : "";
}

function ChangeList({ s }: { s: CompareSession }) {
  const out = s.outcome;
  const sel = compareSel.value;
  const list = useRef<HTMLDivElement>(null);
  const [kinds, setKinds] = useState<ChangeKind | "all">("all");
  useEffect(() => {
    if (out) setAnchors(out.anchors);
  }, [out]);
  useEffect(() => {
    list.current?.querySelector(".cmp-item.on")?.scrollIntoView({ block: "nearest" });
  }, [sel]);
  const shown = useMemo(() => (out ? out.changes.map((c, i) => [c, i] as const).filter(([c]) => kinds === "all" || c.kind === kinds) : []), [out, kinds]);
  if (!out) return <aside class="cmp-list" />;
  const present = (["changed", "added", "removed", "moved", "picture"] as ChangeKind[]).filter((k) => out.changes.some((c) => c.kind === k));
  const MAX = 3000;
  return (
    <aside class="cmp-list" aria-label="Differences">
      <div class="cmp-list-head">
        <strong>Differences</strong>
        <span class="hint">F7</span>
        <button class="ib sm" title="Previous (Shift+F7)" aria-label="Previous difference" disabled={!out.changes.length} onClick={() => step(-1)}>
          <ChevronUp size={15} />
        </button>
        <button class="ib sm" title="Next (F7)" aria-label="Next difference" disabled={!out.changes.length} onClick={() => step(1)}>
          <ChevronDown size={15} />
        </button>
      </div>
      {present.length > 1 && (
        <div class="cmp-filter">
          <button class={`cmp-chip ${kinds === "all" ? "on" : ""}`} onClick={() => setKinds("all")}>
            All
          </button>
          {present.map((k) => (
            <button key={k} class={`cmp-chip k-${k} ${kinds === k ? "on" : ""}`} onClick={() => setKinds(k)}>
              {k === "picture" ? "Drawings" : KIND[k]}
            </button>
          ))}
        </div>
      )}
      <div class="cmp-items" ref={list}>
        {out.changes.length === 0 && (
          <p class="muted small cmp-none">
            {s.a.name} and {s.b.name} have the same words
            {out.picturePages ? ` and drawings (${out.picturePages} pages compared)` : ""}.
          </p>
        )}
        {shown.slice(0, MAX).map(([c, i]) => (
          <button key={i} class={`cmp-item k-${c.kind} ${sel === i ? "on" : ""}`} onClick={() => pick(i)}>
            <span class="cmp-item-top">
              <span class="cmp-kind">{KIND[c.kind]}</span>
              <span class="cmp-where">{where(s, c)}</span>
            </span>
            {c.kind === "changed" && (
              <span class="cmp-words">
                <del>{c.old}</del> <ins>{c.new}</ins>
              </span>
            )}
            {c.kind === "removed" && (
              <span class="cmp-words">
                <del>{c.old}</del>
              </span>
            )}
            {(c.kind === "added" || c.kind === "moved") && (
              <span class="cmp-words">
                <ins>{c.new}</ins>
              </span>
            )}
          </button>
        ))}
        {shown.length > MAX && <p class="muted small cmp-none">And {shown.length - MAX} more: the files differ a lot.</p>}
      </div>
      <div class="cmp-foot hint">
        {out.words[0].toLocaleString()} words · {out.words[1].toLocaleString()} words
      </div>
    </aside>
  );
}
