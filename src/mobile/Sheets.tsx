// Sheets that slide up over the reader or the library: view settings, contents and comments,
// document info, the overflow menu, and the app's settings.
import { useEffect, useRef, useState } from "preact/hooks";
import type { ComponentChildren } from "preact";
import { getVersion } from "@tauri-apps/api/app";
import { BookOpen, ChevronRight, FileText, Info, Minus, Plus, Printer, ScrollText, Share2, Volume2, Wrench, X } from "lucide-preact";
import { api, type OutlineItem, type Properties, type Security } from "../api";
import { native, shareRepaired } from "./native";
import { back, pushLayer } from "./nav";
import { bookmarksOf, refreshStorage, storage, toggleBookmark, unlistAll } from "./records";
import { sizeText, StorageSheet } from "./Storage";
import { ShelfSheet } from "./ShelfView";
import { doc, errorText, isTablet, layout, look, mode, page, say, settings, sheet, updateSettings, updateText, wantsDark, type Layout, type Mode, type Sheet, type Theme } from "./state";
import { PRESETS, lookFor, type Theme as Look } from "../themedata";
import { displayFont } from "./theme";
import { startReading } from "./speech";
import { goTo } from "./views";

/** Opens a sheet (replacing one already open; the back button closes it). */
export function openSheet(s: Exclude<Sheet, null>) {
  if (!sheet.value) pushLayer(() => (sheet.value = null));
  sheet.value = s;
}

/**
 * Pulling a sheet down closes it: from anywhere on it, once its list (if any) is scrolled to the
 * top. Let go before it's a quarter of the way down (and not flicked) and it settles back.
 */
function useDragDown(open: Sheet) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    // The same sheet element shows the next sheet (Storage back to Settings): not left pulled away.
    el.style.transition = "";
    el.style.transform = "";
    let y0 = 0, t0 = 0, dy = 0, live = false, maybe = false;
    const scrollerAt = (t: EventTarget | null) => (t as HTMLElement | null)?.closest?.<HTMLElement>(".sheet-scroll") ?? null;
    const start = (e: TouchEvent) => {
      const t = e.target as HTMLElement;
      const sc = scrollerAt(t);
      // A slider or a list partway down keeps the gesture.
      maybe = e.touches.length === 1 && !t.closest("input") && !(sc && sc.scrollTop > 0);
      live = false;
      dy = 0;
      y0 = e.touches[0].clientY;
      t0 = e.timeStamp;
    };
    const move = (e: TouchEvent) => {
      if (!maybe) return;
      const d = e.touches[0].clientY - y0;
      if (!live) {
        if (d < -4) maybe = false; // scrolling the list up
        if (d < 8) return;
        live = true;
        el.style.transition = "none";
      }
      dy = Math.max(0, d);
      el.style.transform = `translateY(${dy}px)`;
      e.preventDefault();
    };
    const end = (e: TouchEvent) => {
      if (!live) return;
      live = maybe = false;
      const fast = dy / Math.max(1, e.timeStamp - t0) > 0.6;
      el.style.transition = "transform 0.2s var(--ease)";
      if (dy > el.offsetHeight / 4 || (fast && dy > 30)) {
        el.style.transform = "translateY(100%)";
        setTimeout(back, 180);
      } else el.style.transform = "";
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
    };
  }, [open]);
  return ref;
}

export function Sheets() {
  const s = sheet.value;
  const ref = useDragDown(s);
  if (!s) return null;
  const d = doc.value;
  return (
    <div class={`sheet-scrim scrim-${s} ${d && settings.value.night ? "night" : ""}`} onClick={(e) => e.target === e.currentTarget && back()}>
      <div class={`sheet sheet-${s}`} role="dialog" ref={ref}>
        <div class="sheet-grip" />
        {s === "view" && d && <ViewSheet />}
        {s === "contents" && d && <ContentsSheet docId={d.info.docId} rec={d.rec} />}
        {s === "info" && d && <InfoSheet />}
        {s === "more" && d && <MoreSheet />}
        {s === "settings" && <SettingsSheet />}
        {s === "storage" && <StorageSheet />}
        {s === "shelf" && <ShelfSheet />}
      </div>
    </div>
  );
}

function Seg<T extends string>({ value, options, onChange }: { value: T; options: [T, ComponentChildren][]; onChange: (v: T) => void }) {
  return (
    <div class="seg" role="radiogroup">
      {options.map(([v, label]) => (
        <button key={v} role="radio" aria-checked={v === value} class={v === value ? "on" : ""} onClick={() => onChange(v)}>
          {label}
        </button>
      ))}
    </div>
  );
}

function Row({ label, children }: { label: string; children: ComponentChildren }) {
  return (
    <div class="row">
      <span class="row-label">{label}</span>
      {children}
    </div>
  );
}

function Toggle({ on, onChange, label }: { on: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button class="row toggle-row" role="switch" aria-checked={on} onClick={() => onChange(!on)}>
      <span class="row-label">{label}</span>
      <span class={`switch ${on ? "on" : ""}`}>
        <span />
      </span>
    </button>
  );
}

// ---------------------------------------------------------------- view

function ViewSheet() {
  const st = settings.value;
  const m = mode.value;
  const t = st.text;

  return (
    <>
      <Seg<Mode>
        value={m}
        onChange={(v) => (mode.value = v)}
        options={[
          ["pages", <><FileText size={17} /> Pages</>],
          ["read", <><ScrollText size={17} /> Read</>],
          ["book", <><BookOpen size={17} /> Book</>],
        ]}
      />
      <p class="hint">{m === "pages" ? "The document as it was laid out." : m === "read" ? "Just the text, sized for your screen." : "Just the text, as pages you turn."}</p>
      {m === "pages" ? (
        <Row label="Pages">
          <Seg<Layout>
            value={layout.value === "double" && !isTablet ? "paged" : layout.value}
            onChange={(v) => (layout.value = v)}
            options={[["scroll", "Scroll"], ["paged", "One by one"], ...(isTablet ? [["double", "Two"] as [Layout, string]] : [])]}
          />
        </Row>
      ) : (
        <div class="rows">
          <Row label="Text size">
            <div class="stepper">
              <button class="ib" aria-label="Smaller text" onClick={() => updateText({ size: Math.max(12, t.size - 1) })}>
                <Minus size={18} />
              </button>
              <span>{t.size}</span>
              <button class="ib" aria-label="Larger text" onClick={() => updateText({ size: Math.min(34, t.size + 1) })}>
                <Plus size={18} />
              </button>
            </div>
          </Row>
          <Row label="Font">
            <Seg value={t.font} onChange={(v) => updateText({ font: v })} options={[["serif", <span style={{ fontFamily: "serif" }}>Serif</span>], ["sans", "Sans"]]} />
          </Row>
          <Row label="Margins">
            <Seg value={t.margins} onChange={(v) => updateText({ margins: v })} options={[["narrow", "Narrow"], ["normal", "Normal"], ["wide", "Wide"]]} />
          </Row>
          <Row label="Paper">
            <Seg value={t.paper} onChange={(v) => updateText({ paper: v })} options={[["auto", look.value.id === "standard" ? "Plain" : look.value.name], ["sepia", "Sepia"]]} />
          </Row>
          <Toggle label="Justify lines" on={t.justify} onChange={(v) => updateText({ justify: v })} />
        </div>
      )}
    </>
  );
}

// ---------------------------------------------------------------- contents and comments

type Comment = { p: number; n: number; t: string; text: string; author: string; date: number };

const KIND: Record<string, string> = {
  Text: "Note", FreeText: "Text box", Highlight: "Highlight", Underline: "Underline", StrikeOut: "Strikeout", Squiggly: "Squiggle",
  Ink: "Drawing", Square: "Rectangle", Circle: "Ellipse", Line: "Line", Polygon: "Polygon", PolyLine: "Lines", Stamp: "Stamp",
  FileAttachment: "Attachment", Caret: "Insert", Redact: "Redaction", Sound: "Sound",
};

/** The outline entry a page falls under (the last one starting on or before it), for a bookmark's label. */
function sectionOf(items: OutlineItem[], p: number): string {
  let best = "";
  const walk = (list: OutlineItem[]) => {
    for (const it of list) {
      if (it.p < 0 || it.p > p) continue;
      best = it.t;
      if (it.k?.length) walk(it.k);
    }
  };
  walk(items);
  return best;
}

function ContentsSheet({ docId, rec }: { docId: number; rec: string }) {
  const [tab, setTab] = useState<"outline" | "marks" | "comments">("outline");
  const marks = bookmarksOf(rec);
  const [outline, setOutline] = useState<OutlineItem[] | null>(null);
  const [comments, setComments] = useState<Comment[] | null>(null);
  useEffect(() => {
    api.outline(docId).then(setOutline, () => setOutline([]));
    api.docAnnots(docId).then(setComments, () => setComments([]));
  }, [docId]);
  const jump = (p: number, y?: number) => {
    back();
    // After the sheet has gone, so the view isn't hidden while it moves.
    setTimeout(() => goTo(p, y), 0);
  };
  return (
    <>
      <Seg
        value={tab}
        onChange={setTab}
        options={[
          ["outline", "Contents"],
          ["marks", `Bookmarks${marks.length ? ` (${marks.length})` : ""}`],
          ["comments", `Comments${comments?.length ? ` (${comments.length})` : ""}`],
        ]}
      />
      <div class="sheet-scroll">
        {tab === "outline" &&
          (outline === null ? (
            <p class="hint">Reading…</p>
          ) : outline.length === 0 ? (
            <p class="hint">This document has no table of contents.</p>
          ) : (
            <ul class="toc">
              {outline.map((it, i) => (
                <TocItem key={i} it={it} depth={0} onPick={jump} />
              ))}
            </ul>
          ))}
        {tab === "marks" &&
          (marks.length === 0 ? (
            <p class="hint">No bookmarks yet. Tap the bookmark at the top to mark the page you're on.</p>
          ) : (
            <ul class="marks">
              {marks.map((b) => {
                const sec = outline ? sectionOf(outline, b.page) : "";
                return (
                  <li key={b.page} class={b.page === page.value ? "here" : ""}>
                    <button class="mark-go" onClick={() => jump(b.page)}>
                      <strong>Page {b.page + 1}</strong>
                      {sec && <span class="c-text">{sec}</span>}
                    </button>
                    <button class="ib sm" aria-label={`Remove the bookmark on page ${b.page + 1}`} onClick={() => toggleBookmark(rec, b.page)}>
                      <X size={17} />
                    </button>
                  </li>
                );
              })}
            </ul>
          ))}
        {tab === "comments" &&
          (comments === null ? (
            <p class="hint">Reading…</p>
          ) : comments.length === 0 ? (
            <p class="hint">No comments in this document.</p>
          ) : (
            <ul class="comments">
              {comments.map((c) => (
                <li key={`${c.p}-${c.n}`}>
                  <button onClick={() => jump(c.p)}>
                    <span class="c-head">
                      <strong>{KIND[c.t] ?? c.t}</strong> · page {c.p + 1}
                      {c.author && <> · {c.author}</>}
                    </span>
                    {c.text && <span class="c-text">{c.text}</span>}
                  </button>
                </li>
              ))}
            </ul>
          ))}
      </div>
    </>
  );
}

function TocItem({ it, depth, onPick }: { it: OutlineItem; depth: number; onPick: (p: number, y?: number) => void }) {
  const [open, setOpen] = useState(false);
  const kids = it.k?.length ? it.k : null;
  const here = it.p === page.value;
  return (
    <li>
      <div class={`toc-row ${here ? "here" : ""}`} style={{ paddingLeft: `${8 + depth * 18}px` }}>
        {kids ? (
          <button class={`ib sm twist ${open ? "open" : ""}`} aria-label={open ? "Collapse" : "Expand"} onClick={() => setOpen(!open)}>
            <ChevronRight size={16} />
          </button>
        ) : (
          <span class="twist-gap" />
        )}
        <button class="toc-link" disabled={it.p < 0} onClick={() => onPick(it.p, it.y)}>
          <span class="toc-t">{it.t}</span>
          {it.p >= 0 && <span class="toc-p">{it.p + 1}</span>}
        </button>
      </div>
      {kids && open && (
        <ul>
          {kids.map((k, i) => (
            <TocItem key={i} it={k} depth={depth + 1} onPick={onPick} />
          ))}
        </ul>
      )}
    </li>
  );
}

// ---------------------------------------------------------------- info

function size(bytes: number) {
  if (bytes < 1024) return `${bytes} bytes`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function pdfDate(s?: string) {
  // D:YYYYMMDDHHmmSS…
  const m = s?.match(/^(?:D:)?(\d{4})(\d{2})?(\d{2})?(\d{2})?(\d{2})?/);
  if (!m) return s || "";
  const d = new Date(Number(m[1]), Number(m[2] ?? 1) - 1, Number(m[3] ?? 1), Number(m[4] ?? 0), Number(m[5] ?? 0));
  return d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: m[4] ? "short" : undefined });
}

function InfoSheet() {
  const d = doc.value!;
  const info = d.info;
  const [props, setProps] = useState<Properties | null>(null);
  const [sec, setSec] = useState<Security | null>(null);
  useEffect(() => {
    api.properties(info.docId).then(setProps, () => {});
    if (info.isPdf) api.security(info.docId).then(setSec, () => {});
  }, [info.docId]);
  const [w, h] = info.firstSize;
  const mm = (pt: number) => Math.round((pt * 25.4) / 72);
  const rows: [string, string | undefined][] = [
    ["Title", props?.title || info.title],
    ["Author", props?.author || info.author],
    ["Subject", props?.subject],
    ["Keywords", props?.keywords],
    ["File", d.name],
    ["Format", info.isPdf ? "PDF" : info.format],
    ["Pages", String(info.pageCount)],
    ["Page size", info.reflowable ? undefined : `${mm(w)} × ${mm(h)} mm (${(w / 72).toFixed(1)} × ${(h / 72).toFixed(1)} in)`],
    ["File size", size(info.fileSize)],
    ["Created", pdfDate(props?.created)],
    ["Modified", pdfDate(props?.modified)],
    ["Made with", props?.creator],
    ["PDF producer", props?.producer],
    ["Protection", sec ? (sec.method && sec.method !== "None" ? `${sec.method}${sec.bits ? ` ${sec.bits}-bit` : ""}${sec.opens ? ", needs a password to open" : ""}` : "None") : undefined],
  ];
  return (
    <>
      <h3>Document info</h3>
      <dl class="info sheet-scroll">
        {rows
          .filter(([, v]) => v)
          .map(([k, v]) => (
            <div key={k}>
              <dt>{k}</dt>
              <dd>{v}</dd>
            </div>
          ))}
      </dl>
    </>
  );
}

// ---------------------------------------------------------------- more

function MoreSheet() {
  const d = doc.value!;
  const title = d.info.title?.trim() || d.name.replace(/\.[^.]+$/, "");
  const act = (f: () => Promise<void>) => () => {
    back();
    f().catch((e) => say(errorText(e)));
  };
  return (
    <ul class="menu">
      <li>
        <button onClick={act(() => native.share(d.path, d.uri))}>
          <Share2 size={20} /> Share
        </button>
      </li>
      {d.info.repaired && (
        <>
          <li>
            <button onClick={act(() => shareRepaired(d.info.docId, d.name, true))}>
              <Wrench size={20} /> Save as PDF
            </button>
          </li>
          <li>
            <button onClick={act(() => shareRepaired(d.info.docId, d.name, false))}>
              <Wrench size={20} /> Save as repaired EPUB
            </button>
          </li>
        </>
      )}
      <li>
        <button onClick={act(() => native.print(d.info.docId, d.path, title, d.info.isPdf))}>
          <Printer size={20} /> Print
        </button>
      </li>
      <li>
        <button onClick={act(() => startReading())}>
          <Volume2 size={20} /> Read aloud
        </button>
      </li>
      <li>
        <button onClick={() => openSheet("info")}>
          <Info size={20} /> Document info
        </button>
      </li>
    </ul>
  );
}

// ---------------------------------------------------------------- settings

/** A theme in small: its bar, a page, the reading paper and the accent (in its light or dark look, as shown now). */
function ThemeCard({ t, on }: { t: Look; on: boolean }) {
  const l = lookFor(t, wantsDark.value);
  const p = l.pal;
  const font = displayFont(t.id) ?? undefined;
  return (
    <button class={`theme-card ${on ? "on" : ""}`} aria-pressed={on} onClick={() => updateSettings({ look: t.id })}>
      <span class="tc-prev" aria-hidden="true" style={{ background: p.canvas }}>
        <i style={{ background: p.panel, boxShadow: `0 1px 0 ${p.canvas}` }} />
        <span class="tc-page" style={{ background: p.panel, color: p.tx, fontFamily: font }}>
          Aa
        </span>
        <span class="tc-page" style={{ background: l.paper.paper, color: l.paper.ink, fontFamily: font }}>
          Aa
        </span>
        <b style={{ background: p.acc }} />
      </span>
      <span class="tc-name">{t.name}</span>
    </button>
  );
}

function ThemePicker() {
  const cur = look.value;
  const strip = useRef<HTMLDivElement>(null);
  // The chosen theme in view when Settings opens.
  useEffect(() => strip.current?.querySelector<HTMLElement>(".theme-card.on")?.scrollIntoView({ inline: "center", block: "nearest" }), []);
  return (
    <div class="theme-pick">
      <span class="row-label">Colour theme</span>
      <div class="theme-strip" ref={strip}>
        {PRESETS.map((t) => (
          <ThemeCard key={t.id} t={t} on={t.id === cur.id} />
        ))}
      </div>
      <p class="hint">{cur.blurb}</p>
    </div>
  );
}

function SettingsSheet() {
  const [version, setVersion] = useState("");
  useEffect(() => {
    void getVersion().then(setVersion, () => {});
    void refreshStorage();
  }, []);
  const kept = storage.value?.copies.reduce((n, c) => n + c.size, 0) ?? 0;
  return (
    <>
      <h3>Settings</h3>
      <div class="sheet-scroll settings-scroll">
        <ThemePicker />
        <Row label="Light or dark">
          <Seg<Theme>
            value={settings.value.theme}
            onChange={(v) => updateSettings({ theme: v })}
            options={[["system", "System"], ["light", "Light"], ["dark", "Dark"]]}
          />
        </Row>
        {(!look.value.light || !look.value.dark) && (
          <p class="hint theme-only">
            {look.value.name} is {look.value.dark ? "dark" : "light"} only.
          </p>
        )}
        <button
          class="row"
          onClick={() => {
            // In place of Settings: closing it goes back to the library.
            sheet.value = "storage";
          }}
        >
          <span class="row-label">Storage</span>
          <span class="row-value">
            {sizeText(kept)} of copies <ChevronRight size={18} />
          </span>
        </button>
        <button class="row link-row" onClick={() => unlistAll()}>
          <span class="row-label">Clear the Recent list</span>
        </button>
        <div class="about">
          <strong>Paginaut {version}</strong>
          <p>A quiet reader for PDF, e-books, comics, XPS and Markdown. No account, no ads, no tracking: it doesn't use the internet at all.</p>
          <p>Free software under the GNU AGPL 3.0, built on MuPDF by Artifex Software. RAR comics: unarr (LGPL 3.0) and libarchive (BSD).</p>
          <details class="notice">
            <summary>libarchive's licence</summary>
            <p>Copyright (c) 2003-2018 Tim Kientzle and the libarchive authors. All rights reserved.</p>
            <p>
              Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met: 1. Redistributions of source code
              must retain the above copyright notice, this list of conditions and the following disclaimer in this position and unchanged. 2. Redistributions in binary form must reproduce the
              above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.
            </p>
            <p>
              THIS SOFTWARE IS PROVIDED BY THE AUTHOR(S) ``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
              A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE AUTHOR(S) BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
              LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
              STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
            </p>
          </details>
        </div>
      </div>
    </>
  );
}
