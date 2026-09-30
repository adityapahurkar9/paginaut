import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { CaseSensitive, Check, ChevronDown, ChevronUp, Clock, Lock, Regex, Search, WholeWord, X } from "lucide-preact";
import * as A from "../actions";
import * as S from "../search";
import { ordinal, searchOpts, setSearchOpts } from "../search";
import * as P from "../pages";
import { setCoverSeparate, setViewMode } from "../modes";
import { startLayoutEdit } from "./LayoutPanel";
import { clearForm } from "../forms";
import { recognize, removeRecognized } from "../ocr";
import { extractPages, insertBlankPage, startInsert, startMerge, startMergePictures, startSplit } from "./PagesUI";
import { startExport } from "./ConvertPane";
import { toggleGrid } from "./PageGrid";
import { startPageSize } from "../pagesize";
import { startHeaders, startWatermark } from "./StampUI";
import { startLabels, startProperties } from "./MetaUI";
import { startSecure } from "./SecureUI";
import { attachFiles, openPageTable, picturesDialog, showAttachments, startArea } from "../extract";
import { addPicture, toggleTextEdit } from "../textedit";
import { startRedact } from "../redact";
import { officeDialog } from "../office";
import { pickAndConvert } from "./OfficeUI";
import { addBookmarkHere } from "./Sidebar";
import { removeStamps } from "../stamps";
import { helpPanel } from "./HelpDialogs";
import { activeTab, baseName, dialog, findOpen, openFiles, paletteOpen, recent, settings, toast, unlockTab, updateSettings, type Tab } from "../store";

// ---------------------------------------------------------------- menu

export type MenuItem =
  | { sep: true }
  | { head: string }
  | { label: string; hint?: string; check?: boolean; disabled?: boolean; icon?: preact.ComponentChild; run: () => void };

/** Opens below `y`, or above `bottom` (px from the window's bottom edge) for buttons at the bottom of the window. */
export function Menu({ x, y, bottom, items, onClose, width = 220 }: { x: number; y: number; bottom?: number; items: MenuItem[]; onClose: () => void; width?: number }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const down = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    setTimeout(() => window.addEventListener("pointerdown", down), 0);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("pointerdown", down);
      window.removeEventListener("keydown", key);
    };
  }, []);
  const left = Math.max(8, Math.min(x, window.innerWidth - width - 8));
  return (
    <div class="menu" ref={ref} style={bottom === undefined ? { left, top: y, minWidth: width } : { left, bottom, minWidth: width }}>
      {items.map((it, i) =>
        "sep" in it ? (
          <hr key={i} />
        ) : "head" in it ? (
          <div key={i} class="menu-head">{it.head}</div>
        ) : (
          <button
            key={i}
            disabled={it.disabled}
            onClick={() => {
              onClose();
              it.run();
            }}
          >
            <span style={{ minWidth: 16, display: "inline-flex" }}>{it.check ? <Check size={15} /> : (it.icon ?? null)}</span>
            {it.label}
            {it.hint && <span class="hint">{it.hint}</span>}
          </button>
        ),
      )}
    </div>
  );
}

// ---------------------------------------------------------------- search option toggles

const REGEX_TIP = String.raw`Pattern search (regular expression). Examples:
\d{10}   any 10-digit number
\d+(\.\d+)?%   percentages like 12.5%
[A-Z]{5}\d{4}[A-Z]   PAN numbers
Rs\.?\s*[\d,]+   rupee amounts
foo|bar   either word`;

function SearchToggles({ onChange }: { onChange: () => void }) {
  const o = searchOpts.value;
  const t = (key: "matchCase" | "wholeWord" | "regex", label: string, icon: preact.ComponentChild, tip = label) => (
    <button
      class={`ib sm toggle ${o[key] ? "on" : ""}`}
      aria-label={label}
      aria-pressed={o[key]}
      title={tip}
      onClick={() => {
        setSearchOpts({ [key]: !o[key] });
        onChange();
      }}
    >
      {icon}
    </button>
  );
  return (
    <>
      {t("matchCase", "Match case", <CaseSensitive size={16} />)}
      {t("wholeWord", "Whole word", <WholeWord size={16} />)}
      {t("regex", "Regular expression", <Regex size={16} />, REGEX_TIP)}
    </>
  );
}

// ---------------------------------------------------------------- find bar

export function FindBar({ tab }: { tab: Tab }) {
  const input = useRef<HTMLInputElement>(null);
  const s = tab.search.value;
  const [text, setText] = useState(s?.query ?? "");
  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, [findOpen.value]);
  // Picks up a query started from the command palette.
  useEffect(() => {
    if (s && s.query !== text.trim()) setText(s.query);
  }, [s?.sid]);

  const fsr = tab.mode.value !== "pages" ? tab.flowSearch.value : null;
  const count = tab.mode.value !== "pages"
    ? !fsr
      ? ""
      : fsr.error
        ? fsr.error
        : fsr.hits.length
          ? `${fsr.active + 1} of ${fsr.hits.length}${fsr.limited ? "+" : ""}`
          : "No matches"
    : !s
    ? ""
    : s.error
      ? s.error
      : s.total
        ? `${s.active ? ordinal(s, s.active) + 1 : "–"} of ${s.total}${s.done && !s.limited ? "" : "+"}`
        : s.done
          ? "No matches"
          : "Searching…";
  return (
    <div class="findbar" onKeyDown={(e) => e.stopPropagation()}>
      <Search size={16} style={{ marginLeft: 6, color: "var(--tx2)" }} />
      <input
        ref={input}
        class="inp"
        placeholder="Find in document"
        value={text}
        onInput={(e) => {
          const v = (e.currentTarget as HTMLInputElement).value;
          setText(v);
          A.runSearch(tab, v);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") A.stepSearch(e.shiftKey ? -1 : 1);
          else if (e.key === "Escape") A.closeFind();
          else if (e.key === "F3") A.stepSearch(e.shiftKey ? -1 : 1);
        }}
      />
      <SearchToggles onChange={() => A.runSearch(tab, text)} />
      <span class={`count ${(fsr ? fsr.error : s?.error) ? "error-text" : ""}`}>{count}</span>
      <button class="ib sm" aria-label="Previous match" title="Previous (Shift+Enter)" onClick={() => A.stepSearch(-1)}>
        <ChevronUp size={16} />
      </button>
      <button class="ib sm" aria-label="Next match" title="Next (Enter)" onClick={() => A.stepSearch(1)}>
        <ChevronDown size={16} />
      </button>
      <button class="ib sm" aria-label="Close find" title="Close (Esc)" onClick={A.closeFind}>
        <X size={15} />
      </button>
    </div>
  );
}

// ---------------------------------------------------------------- command palette

type Group = "Commands" | "In this document" | "Recent files";

interface Item {
  label: string;
  sub?: string;
  key?: string;
  group: Group;
  run: () => void;
  when?: () => boolean;
  /** Search results: page number and snippet. */
  page?: number;
  snippet?: string;
  occurrence?: number;
}

const hasDoc = () => activeTab.value?.status.value === "ready";
const withTab = (fn: (t: Tab) => unknown) => () => {
  const t = activeTab.value;
  if (t && t.status.value === "ready") fn(t);
};

function commands(): Item[] {
  const c = (label: string, run: () => void, key?: string, when?: () => boolean): Item => ({ label, run, key, group: "Commands", when });
  return [
    c("Open file…", A.openDialog, "Ctrl+O"),
    c("Find in document", A.openFind, "Ctrl+F", hasDoc),
    c("Go to page…", () => setTimeout(() => (document.getElementById("page-input") as HTMLInputElement | null)?.focus(), 0), "Ctrl+G", hasDoc),
    c("Fit width", A.fitWidth, "Ctrl+2", hasDoc),
    c("Fit page", A.fitPage, "Ctrl+1", hasDoc),
    c("Actual size (100%)", () => A.zoomTo(1), "Ctrl+0", hasDoc),
    c("Zoom in", A.zoomIn, "Ctrl+=", hasDoc),
    c("Zoom out", A.zoomOut, "Ctrl+−", hasDoc),
    c("Read mode (reflowed text, one column)", () => A.toggleMode("read"), "Ctrl+H", hasDoc),
    c("Book mode (reflowed text, two pages, full screen)", () => A.toggleMode("book"), "F5", hasDoc),
    c("Fix reading layout (columns, order, pictures)", withTab((t) => void startLayoutEdit(t)), "", hasDoc),
    c("Recognize text (OCR)…", withTab((t) => void recognize(t)), "", hasDoc),
    c("Convert Office files to PDF (Word, Excel, PowerPoint)…", () => void pickAndConvert(), ""),
    c("Convert to Word…", withTab((t) => startExport(t, "docx")), "", hasDoc),
    c("Convert tables to Excel…", withTab((t) => startExport(t, "xlsx")), "", hasDoc),
    c("Convert to PowerPoint…", withTab((t) => startExport(t, "pptx")), "", hasDoc),
    c("Convert pages to images (PNG, JPEG, TIFF)…", withTab((t) => startExport(t, "images")), "", hasDoc),
    c("Convert to text…", withTab((t) => startExport(t, "txt")), "", hasDoc),
    c("Convert web pages (HTML) to PDF…", () => void pickAndConvert(true), ""),
    c("Convert images to one PDF…", () => void startMergePictures(), ""),
    c("Office suite choices: programs that open Office files, get LibreOffice…", () => (officeDialog.value = true), ""),
    c("Remove recognized text", withTab((t) => void removeRecognized(t)), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Clear form", withTab((t) => void clearForm(t)), "", () => !!activeTab.value?.forms.value?.fields),
    c("Save a flattened copy of the form…", withTab((t) => void P.saveFlattened(t)), "", () => !!activeTab.value?.forms.value?.fields),
    c(settings.value.highlightFields ? "Hide form field highlights" : "Highlight form fields", () => updateSettings({ highlightFields: !settings.value.highlightFields }), ""),
    c("Toggle sidebar", A.toggleSidebar, "F4"),
    c("Dark pages (night mode)", A.toggleNight, "Ctrl+Shift+N"),
    c("Toggle light / dark theme", A.toggleTheme, "Ctrl+Shift+D"),
    c("Full screen", A.toggleFullscreen, "F11"),
    c("Copy selected text", A.copySelection, "Ctrl+C", () => !!activeTab.value?.hasSelection.value),
    c("Rotate page right", withTab((t) => P.rotatePages(t, 90)), "R", hasDoc),
    c("Rotate page left", withTab((t) => P.rotatePages(t, -90)), "Shift+R", hasDoc),
    c("Rotate page 180°", withTab((t) => P.rotatePages(t, 180)), "", hasDoc),
    c("Delete page", withTab((t) => P.deletePages(t)), "Del", hasDoc),
    c("Insert pages from a file…", withTab((t) => void startInsert(t)), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Merge files into one PDF…", () => void startMerge(activeTab.value?.status.value === "ready" ? activeTab.value : null), ""),
    c("Page grid (organize pages)", withTab((t) => toggleGrid(t)), "Ctrl+Shift+G", hasDoc),
    c("Crop and resize pages (page size)…", withTab((t) => startPageSize(t)), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Page numbers, headers and footers…", withTab((t) => startHeaders(t)), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Watermark…", withTab((t) => startWatermark(t)), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Page labels (how pages are numbered)…", withTab((t) => startLabels(t)), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Document properties (title, author)…", withTab((t) => startProperties(t)), "Ctrl+D", hasDoc),
    c("Protect with a password (encrypt, restrict printing or copying)…", withTab((t) => startSecure(t, "protect")), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Remove password / unlock…", withTab((t) => startSecure(t, "protect")), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Reduce file size (compress)…", withTab((t) => startSecure(t, "compress")), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Repair a damaged PDF…", withTab((t) => startSecure(t, "repair")), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Redact: remove or cover text and areas…", withTab((t) => startRedact(t)), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Sanitize: remove metadata, scripts, hidden text…", withTab((t) => startRedact(t)), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Edit text and pictures (change words on the page)", withTab((t) => toggleTextEdit(t)), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Add a picture to the page…", withTab((t) => void addPicture(t)), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Save all pictures (extract images)…", withTab((t) => (picturesDialog.value = t)), "", hasDoc),
    c("Table on this page to Excel / CSV (extract table)…", withTab((t) => void openPageTable(t)), "", hasDoc),
    c("Select an area to copy or save (PDF, SVG, PNG, text)…", withTab((t) => startArea(t)), "", hasDoc),
    c("Attached files (attachments)", showAttachments, "", () => hasDoc() && !!activeTab.value?.attachments.value?.length),
    c("Attach a file to the document…", withTab((t) => void attachFiles(t)), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Add a bookmark for this page", withTab((t) => void addBookmarkHere(t)), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Remove page numbers, headers and footers", withTab((t) => void removeStamps(t, "Header")), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Remove watermark", withTab((t) => void removeStamps(t, "Watermark")), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Split into files…", withTab((t) => startSplit(t)), "", hasDoc),
    c("Save selected pages as a new PDF (extract)…", withTab((t) => void extractPages(t)), "", hasDoc),
    c("Insert blank page", withTab((t) => void insertBlankPage(t)), "", () => hasDoc() && !!activeTab.value?.info.value?.isPdf),
    c("Undo", withTab((t) => P.undo(t)), "Ctrl+Z", () => !!activeTab.value?.history.canUndo),
    c("Redo", withTab((t) => P.redo(t)), "Ctrl+Y", () => !!activeTab.value?.history.canRedo),
    c("Save", withTab((t) => P.save(t)), "Ctrl+S", () => !!activeTab.value?.dirty.value),
    c("Save as…", withTab((t) => P.save(t, true)), "Ctrl+Shift+S", hasDoc),
    c("Single page", withTab((t) => void setViewMode(t, "single")), "", hasDoc),
    c("Double page (two side by side)", withTab((t) => void setViewMode(t, "double")), "", hasDoc),
    c(settings.value.coverSeparate ? "Double page: cover with the other pages" : "Double page: cover on its own", () => setCoverSeparate(!settings.value.coverSeparate), ""),
    c(settings.value.autoSaveRotations ? "Turn off auto-save for rotations" : "Turn on auto-save for rotations",
      () => updateSettings({ autoSaveRotations: !settings.value.autoSaveRotations }), ""),
    c("Next tab", A.nextTab, "Ctrl+Tab"),
    c("Close tab", A.closeActive, "Ctrl+W", () => !!activeTab.value),
    c("Keyboard shortcuts", () => (helpPanel.value = "shortcuts"), "F1"),
    c("About Paginaut", () => (helpPanel.value = "about"), ""),
  ];
}

/** Snippet centred on its own match (the n-th in the line), with matches emphasised. */
function Snippet({ text, query, occurrence }: { text: string; query: string; occurrence: number }) {
  let re: RegExp | null = null;
  try {
    const o = searchOpts.value;
    const body = o.regex ? query : query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    re = new RegExp(o.wholeWord ? `\\b(?:${body})\\b` : body, o.matchCase ? "g" : "gi");
  } catch {
    re = null;
  }
  if (!re) return <span>{text.slice(0, 160)}</span>;
  const matches = [...text.matchAll(re)].filter((m) => m[0]);
  const own = matches[Math.min(occurrence, matches.length - 1)];
  // Keep ~40 characters of context before this hit.
  const start = own && (own.index ?? 0) > 40 ? (own.index ?? 0) - 40 : 0;
  const t = (start ? "…" : "") + text.slice(start, start + 160);
  const shift = start ? start - 1 : 0;
  const parts: preact.ComponentChild[] = [];
  let last = 0;
  for (const m of matches) {
    const at = (m.index ?? 0) - shift;
    if (at < last || at >= t.length) continue;
    const cls = m === own ? "own" : "";
    parts.push(t.slice(last, at), <mark class={cls}>{m[0]}</mark>);
    last = at + m[0].length;
  }
  parts.push(t.slice(last));
  return <span>{parts}</span>;
}

export function Palette() {
  if (!paletteOpen.value) return null;
  return <PaletteInner />;
}

function PaletteInner() {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const tab = activeTab.value;
  const doc = tab && tab.status.value === "ready" ? tab : null;
  const search = doc?.search.value ?? null;
  useEffect(() => input.current?.focus(), []);

  // Searching the document as you type (hits get highlighted; the page only moves when you pick one).
  const startSearch = (text: string) => {
    if (!doc) return;
    if (text.trim().length < 2) S.clearSearch(doc);
    else S.runSearch(doc, text, () => {}, () => {}, { snippets: true, delay: 220 });
  };

  const close = (keepSearch = false) => {
    paletteOpen.value = false;
    if (doc && !keepSearch && !findOpen.value) S.clearSearch(doc);
    activeTab.value?.viewer?.focus();
  };

  const list = useMemo(() => {
    const ql = q.trim().toLowerCase();
    const cmds = commands().filter((c) => (!c.when || c.when()) && (!ql || c.label.toLowerCase().includes(ql)));
    const files: Item[] = recent.value
      .filter((r) => !ql || r.name.toLowerCase().includes(ql) || r.path.toLowerCase().includes(ql))
      .slice(0, ql ? 6 : 6)
      .map((r) => ({ label: r.name, sub: r.path, group: "Recent files" as Group, run: () => openFiles([r.path]) }));
    const hits: Item[] =
      doc && search && search.query === q.trim()
        ? S.listHits(search, 60).map((h) => ({
            label: `${h.page + 1}`,
            page: h.page,
            snippet: h.snippet,
            occurrence: h.occurrence,
            group: "In this document" as Group,
            run: () => {
              findOpen.value = true;
              const v = doc.viewer?.viewOfSource(h.page);
              if (v !== undefined) S.selectHit(doc, h.page, h.index);
            },
          }))
        : [];
    // While searching, keep commands short so results are visible.
    return [...(ql.length >= 2 ? cmds.slice(0, 5) : cmds), ...hits, ...files.slice(0, ql ? 4 : 6)];
  }, [q, search]);

  const run = (it: Item | undefined) => {
    if (!it) return;
    close(it.group === "In this document");
    it.run();
  };

  const docHeader = search && search.query === q.trim()
    ? search.error
      ? search.error
      : `In this document · ${search.total}${search.done && !search.limited ? "" : "+"} ${search.total === 1 ? "match" : "matches"}${search.limited ? " (stopped: too many)" : ""}`
    : "In this document";

  return (
    <div class="overlay" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div class="palette" onKeyDown={(e) => e.stopPropagation()}>
        <div class="top">
          <Search size={17} style={{ color: "var(--tx2)" }} />
          <input
            ref={input}
            placeholder={doc ? "Search this document, or type a command…" : "Type a command or a file name…"}
            value={q}
            onInput={(e) => {
              const v = (e.currentTarget as HTMLInputElement).value;
              setQ(v);
              setSel(0);
              startSearch(v);
            }}
            onKeyDown={(e) => {
              if (e.key === "Escape") close();
              else if (e.key === "ArrowDown") {
                e.preventDefault();
                setSel((s) => Math.min(list.length - 1, s + 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setSel((s) => Math.max(0, s - 1));
              } else if (e.key === "Enter") run(list[sel]);
            }}
          />
          {doc && <SearchToggles onChange={() => startSearch(q)} />}
          <kbd>Esc</kbd>
        </div>
        <div class="list">
          {list.map((c, i) => (
            <>
              {(i === 0 || list[i - 1].group !== c.group) && (
                <div class="head">{c.group === "In this document" ? docHeader : c.group}</div>
              )}
              {c.group === "In this document" ? (
                <button class={`cmd hit ${i === sel ? "sel" : ""}`} onMouseEnter={() => setSel(i)} onClick={() => run(c)}>
                  <span class="pn">p. {c.label}</span>
                  <span class="snip">
                    <Snippet text={c.snippet ?? ""} query={q.trim()} occurrence={c.occurrence ?? 0} />
                  </span>
                </button>
              ) : (
                <button class={`cmd ${i === sel ? "sel" : ""}`} onMouseEnter={() => setSel(i)} onClick={() => run(c)}>
                  {c.group === "Recent files" && <Clock size={14} style={{ color: "var(--tx3)" }} />}
                  <span>{c.label}</span>
                  {c.sub && <span class="sub">{c.sub}</span>}
                  <span style={{ flex: 1 }} />
                  {c.key && <kbd>{c.key}</kbd>}
                </button>
              )}
            </>
          ))}
          {doc && search && search.query === q.trim() && !search.total && search.done && !search.error && (
            <div class="empty-note">No matches for “{q.trim()}” in this document.</div>
          )}
          {!list.length && !doc && <div class="empty-note">Nothing matches “{q}”.</div>}
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- password

export function PasswordDialog({ tab }: { tab: Tab }) {
  const [pw, setPw] = useState("");
  const [wrong, setWrong] = useState(false);
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);
  const submit = async () => {
    setBusy(true);
    const ok = await unlockTab(tab, pw);
    setBusy(false);
    if (!ok) {
      setWrong(true);
      input.current?.select();
    }
  };
  return (
    <div class="overlay" style={{ position: "absolute" }}>
      <form
        class="dialog"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <h2 style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <Lock size={18} /> Password required
        </h2>
        <span style={{ color: "var(--tx2)" }}>{baseName(tab.path)} is protected. Enter its password to open it.</span>
        <input ref={input} class="inp" type="password" value={pw} onInput={(e) => setPw((e.currentTarget as HTMLInputElement).value)} aria-label="Password" />
        {wrong && <span class="error-text">That password didn't work. Try again.</span>}
        <div class="row">
          <button type="button" class="btn" onClick={() => A.closeActive()}>
            Cancel
          </button>
          <button type="submit" class="btn pri" disabled={busy || !pw}>
            Open
          </button>
        </div>
      </form>
    </div>
  );
}

// ---------------------------------------------------------------- ask dialog

export function AskDialog() {
  const d = dialog.value;
  return d ? <AskInner key={d.title} /> : null;
}

function AskInner() {
  const d = dialog.value!;
  const primary = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    primary.current?.focus();
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        d.resolve("cancel");
      }
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, []);
  return (
    <div class="overlay">
      <div class="dialog" role="alertdialog" aria-label={d.title}>
        <h2>{d.title}</h2>
        <span style={{ color: "var(--tx2)" }}>{d.text}</span>
        <div class="row">
          {[...d.buttons].reverse().map((b) => (
            <button key={b.value} ref={b.primary ? primary : undefined} class={`btn ${b.primary ? "pri" : ""}`} onClick={() => d.resolve(b.value)}>
              {b.label}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- toast

export function Toast() {
  const t = toast.value;
  if (!t) return null;
  return (
    <div class="toast" key={t.id}>
      {t.text}
    </div>
  );
}
