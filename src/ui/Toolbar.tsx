// Menu tabs (File · Home · View · Tools · Help) and the toolbar under them.
// The left part of the toolbar changes with the tab; find stays on the right.
// Page, zoom and the view modes sit in the middle of the status bar (ViewControls).
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { signal } from "@preact/signals";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { ComponentChild } from "preact";
import {
  BookOpen, ChevronDown, ChevronsRight, ChevronUp, Columns2, Combine, Expand, File, FileOutput, FolderOpen, FormInput, Hand, Highlighter, Info, Keyboard,
  Minus, Moon, MousePointer2, MoveHorizontal, PanelLeft, PanelRight, Plus, Redo2, RefreshCw, RotateCcw, RotateCw, Save, ScanText, ScrollText,
  Search, Signature, SquareSplitHorizontal, Sun, SunMoon, Trash2, FilePlus2, LayoutGrid, Type, Undo2, Wrench, Underline, Strikethrough, SquareDashed,
  PenLine, StickyNote, TypeOutline, Square, Circle, Slash, MoveUpRight, Stamp, MessagesSquare, Check, X, CalendarDays, UserRound,
  FileLock2, ShieldCheck, TextCursorInput,
} from "lucide-preact";
import * as A from "../actions";
import * as P from "../pages";
import { setCoverSeparate, setViewMode, viewModeOf, type ViewMode } from "../modes";
import { typeOpen } from "./Reflow";
import { clearForm } from "../forms";
import { annotTool, PALETTE, selectedAnnot, setStyle, STAMPS, styleOf, toolStyle, type AnnotTool } from "../annots";
import { startLayoutEdit } from "./LayoutPanel";
import { loadSigs, newSig, openSignPane, pickSig, sigs, sigsOf, type SigKind } from "../sign";
import { startSigning } from "../dsc";
import { recognize, removeRecognized } from "../ocr";
import { extractPages, insertBlankPage, startInsert, startMerge, startSplit } from "./PagesUI";
import { toggleGrid } from "./PageGrid";
import { startPageSize } from "../pagesize";
import { startHeaders, startWatermark } from "./StampUI";
import { startLabels, startProperties } from "./MetaUI";
import { startSecure } from "./SecureUI";
import { openPageTable, picturesDialog, showAttachments, startArea } from "../extract";
import { textEdit, toggleTextEdit } from "../textedit";
import { activeTab, isDark, openFiles, recent, settings, updateSettings, type Settings, type Tab } from "../store";
import { tool } from "./App";
import { helpPanel } from "./HelpDialogs";
import { Menu, type MenuItem } from "./Overlays";

type MenuState = { x: number; y: number; bottom?: number; items: MenuItem[]; width?: number } | null;
export const menu = signal<MenuState>(null);

/** "up": centred above the button (for the status bar). */
function openMenu(e: MouseEvent, items: MenuItem[], align: "left" | "right" | "up" = "left", width = 220) {
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
  menu.value =
    align === "up"
      ? { x: r.left + r.width / 2 - width / 2, y: 0, bottom: window.innerHeight - r.top + 4, items, width }
      : { x: align === "left" ? r.left : r.right - width, y: r.bottom + 4, items, width };
}

/** A menu at the pointer (right-click); opens upwards in the lower part of the window. */
export function openMenuAt(x: number, y: number, items: MenuItem[], width = 220) {
  menu.value = y > window.innerHeight * 0.55 ? { x, y: 0, bottom: window.innerHeight - y, items, width } : { x, y, items, width };
}

// ---------------------------------------------------------------- File menu

export const fileMenuOpen = signal(false);

function fileItems(): MenuItem[] {
  const tab = activeTab.value;
  const ready = tab?.status.value === "ready" ? tab : null;
  const isPdf = !!ready?.info.value?.isPdf;
  const files = recent.value.slice(0, 6);
  return [
    { label: "Open…", hint: "Ctrl+O", icon: <FolderOpen size={15} />, run: A.openDialog },
    { label: isPdf || !ready ? "Save" : "Save as PDF…", hint: "Ctrl+S", icon: <Save size={15} />, disabled: !ready || (isPdf && !ready.dirty.value), run: () => ready && void P.save(ready) },
    { label: "Save as…", hint: "Ctrl+Shift+S", icon: <FileOutput size={15} />, disabled: !ready, run: () => ready && void P.save(ready, true) },
    { sep: true },
    { label: "Document properties…", hint: "Ctrl+D", disabled: !ready, run: () => ready && startProperties(ready) },
    { sep: true },
    { label: "Auto-save rotations", check: settings.value.autoSaveRotations, run: () => updateSettings({ autoSaveRotations: !settings.value.autoSaveRotations }) },
    ...(files.length ? [{ sep: true } as MenuItem, { head: "Recent" } as MenuItem] : []),
    // Same-named files are told apart by their folder.
    ...files.map((f) => ({
      label: f.name,
      hint: files.some((g) => g !== f && g.name.toLowerCase() === f.name.toLowerCase()) ? f.path.split(/[\\/]/).slice(-2, -1)[0] : undefined,
      run: () => void openFiles([f.path]),
    })),
    { sep: true },
    { label: "Close tab", hint: "Ctrl+W", disabled: !tab, run: A.closeActive },
    { label: "Exit", hint: "Alt+F4", run: () => void getCurrentWindow().close() },
  ];
}

// ---------------------------------------------------------------- menu tabs

const TABS: [Settings["ribbon"], string][] = [["home", "Home"], ["comment", "Comment"], ["sign", "Sign"], ["view", "View"], ["tools", "Tools"], ["help", "Help"]];

export function Ribbon() {
  const cur = settings.value.ribbon;
  const fileBtn = useRef<HTMLButtonElement>(null);
  // Alt+F (from the keyboard handler) opens the File menu under its button.
  useEffect(() => {
    if (!fileMenuOpen.value || !fileBtn.current) return;
    fileMenuOpen.value = false;
    const r = fileBtn.current.getBoundingClientRect();
    menu.value = { x: r.left, y: r.bottom + 4, items: fileItems(), width: 260 };
  }, [fileMenuOpen.value]);
  return (
    <div class="ribbon" role="tablist">
      <button ref={fileBtn} class="rtab file" onClick={(e) => openMenu(e, fileItems(), "left", 260)}>
        File
      </button>
      {TABS.map(([key, label]) => (
        <button key={key} role="tab" aria-selected={cur === key} class={`rtab ${cur === key ? "on" : ""}`} onClick={() => updateSettings(key === "sign" ? { ribbon: key, signPane: true } : { ribbon: key })}>
          {label}
        </button>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------- toolbar

/** Toolbar button with a text label. */
function TB(p: { icon: ComponentChild; label: string; title?: string; on?: boolean; disabled?: boolean; menu?: boolean; pri?: boolean; onClick: (e: MouseEvent) => void }) {
  return (
    <button class={`tb ${p.on ? "on" : ""} ${p.pri ? "pri" : ""}`} disabled={p.disabled} title={p.title ?? p.label} onClick={p.onClick}>
      {p.icon}
      <span>{p.label}</span>
      {p.menu && <ChevronDown size={12} class="chev" />}
    </button>
  );
}

/** Icon-only toolbar button. */
function IB(p: { icon: ComponentChild; label: string; title?: string; on?: boolean; disabled?: boolean; onClick: (e: MouseEvent) => void }) {
  return (
    <button class={`ib ${p.on ? "on" : ""}`} disabled={p.disabled} aria-label={p.label} title={p.title ?? p.label} onClick={p.onClick}>
      {p.icon}
    </button>
  );
}

const I = (C: typeof Save, size = 18) => <C size={size} strokeWidth={1.7} />;

export function Toolbar() {
  const tab = activeTab.value;
  const ready = tab?.status.value === "ready" ? tab : null;
  const which = settings.value.ribbon;
  const bar = useRef<HTMLDivElement>(null);
  const left = useRef<HTMLDivElement>(null);
  const moreBtn = useRef<HTMLButtonElement>(null);
  // In a narrow window the buttons that don't fit are hidden and go into the » panel:
  // `shown` stay in the bar, the panel holds the ones from `start` on (separators at the cut dropped).
  const [cut, setCut] = useState<{ shown: number; start: number } | null>(null);
  const [moreOpen, setMoreOpen] = useState(false);
  useLayoutEffect(() => {
    const el = left.current;
    if (!el) return;
    const measure = () => {
      const kids = [...el.children] as HTMLElement[];
      let shown = kids.findIndex((k) => k.offsetLeft + k.offsetWidth > el.clientWidth + 0.5);
      if (shown < 0) {
        setCut((c) => (c ? null : c));
        setMoreOpen(false);
        return;
      }
      let start = shown;
      while (shown > 0 && kids[shown - 1].classList.contains("sep")) shown--;
      while (start < kids.length && kids[start].classList.contains("sep")) start++;
      setCut((c) => (c && c.shown === shown && c.start === start ? c : { shown, start }));
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    if (bar.current) ro.observe(bar.current);
    // Buttons come and go within a group (e.g. the thickness picker for the pen).
    const mo = new MutationObserver(measure);
    mo.observe(el, { childList: true, subtree: true });
    return () => {
      ro.disconnect();
      mo.disconnect();
    };
  }, [which]);
  useEffect(() => setMoreOpen(false), [which]);
  const tools = (
    <>
      <IB icon={I(PanelLeft)} label="Sidebar" title="Sidebar (F4)" on={settings.value.sidebarOpen} onClick={A.toggleSidebar} />
      <div class="sep" />
      {which === "home" && <HomeTools tab={ready} />}
      {which === "comment" && <CommentTools tab={ready} />}
      {which === "sign" && <SignTools tab={ready} />}
      {which === "view" && <ViewTools tab={ready} />}
      {which === "tools" && <ToolsTools tab={ready} />}
      {which === "help" && <HelpTools />}
    </>
  );
  return (
    <div class="toolbar" ref={bar}>
      {cut && (
        <style>{`.tb-left > :nth-child(n+${cut.shown + 1}) { visibility: hidden; } .tb-more-panel > :nth-child(-n+${cut.start}) { display: none; }`}</style>
      )}
      <div class="tb-left" key={which} ref={left}>
        {tools}
      </div>
      {cut && (
        <button
          ref={moreBtn}
          class={`ib sm tb-morebtn ${moreOpen ? "on" : ""}`}
          aria-label="More tools"
          aria-expanded={moreOpen}
          title="More tools"
          onClick={() => setMoreOpen(!moreOpen)}
        >
          <ChevronsRight size={16} />
        </button>
      )}
      <div class="grow" />
      <IB icon={I(Search)} label="Find" title="Find (Ctrl+F) · Commands (Ctrl+K)" disabled={!ready} onClick={A.openFind} />
      {cut && moreOpen && moreBtn.current && (
        <MorePanel anchor={moreBtn.current} onClose={() => setMoreOpen(false)}>
          {tools}
        </MorePanel>
      )}
      {menu.value && <Menu {...menu.value} onClose={() => (menu.value = null)} />}
    </div>
  );
}

/** Panel under the » button, right-aligned to it, with the toolbar buttons that didn't fit. Closes after a button is used. */
function MorePanel({ anchor, onClose, children }: { anchor: HTMLElement; onClose: () => void; children: ComponentChild }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const down = (e: PointerEvent) => {
      const t = e.target as Node;
      if (!ref.current?.contains(t) && !anchor.contains(t)) onClose();
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("pointerdown", down);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("pointerdown", down);
      window.removeEventListener("keydown", key);
    };
  }, [anchor]);
  const r = anchor.getBoundingClientRect();
  return (
    <div
      class="tb-more-panel"
      ref={ref}
      // Top-right corner under the »'s bottom-right; never wider than the room to its left.
      style={{ top: r.bottom + 4, right: window.innerWidth - r.right, maxWidth: Math.min(440, r.right - 8) }}
      onClick={(e) => (e.target as HTMLElement).closest("button") && onClose()}
    >
      {children}
    </div>
  );
}

function HomeTools({ tab }: { tab: Tab | null }) {
  if (tab) void tab.historyVersion.value; // refresh when history changes
  const isPdf = !!tab?.info.value?.isPdf;
  const dirty = !!tab?.dirty.value;
  const saving = !!tab?.saving.value;
  return (
    <>
      <IB icon={I(FolderOpen)} label="Open" title="Open (Ctrl+O)" onClick={A.openDialog} />
      <TB
        icon={saving ? <span class="spin" /> : I(Save, 17)}
        label={saving ? "Saving…" : isPdf || !tab ? "Save" : "Save as PDF"}
        title={dirty ? "Save changes (Ctrl+S)" : isPdf ? "No changes to save (Ctrl+S)" : "Save as PDF (Ctrl+S)"}
        pri={dirty}
        disabled={!tab || saving || (isPdf && !dirty)}
        onClick={() => tab && void P.save(tab)}
      />
      <TB icon={I(FileOutput, 17)} label="Save as" title="Save a copy as (Ctrl+Shift+S)" disabled={!tab || saving} onClick={() => tab && void P.save(tab, true)} />
      <div class="sep" />
      <IB icon={I(MousePointer2)} label="Select text" title="Select text (V)" on={tool.value === "select"} onClick={() => (tool.value = "select")} />
      <IB icon={I(Hand)} label="Hand tool" title="Hand (H) · or hold the middle button" on={tool.value === "hand"} onClick={() => (tool.value = "hand")} />
      <div class="sep" />
      <IB icon={I(Undo2)} label="Undo" title="Undo (Ctrl+Z)" disabled={!tab?.history.canUndo} onClick={() => tab && P.undo(tab)} />
      <IB icon={I(Redo2)} label="Redo" title="Redo (Ctrl+Y)" disabled={!tab?.history.canRedo} onClick={() => tab && P.redo(tab)} />
      <IB icon={I(RotateCcw)} label="Rotate left" title="Rotate page left (Shift+R)" disabled={!tab} onClick={() => tab && P.rotatePages(tab, -90)} />
      <IB icon={I(RotateCw)} label="Rotate right" title="Rotate page right (R)" disabled={!tab} onClick={() => tab && P.rotatePages(tab, 90)} />
      <IB icon={I(Trash2)} label="Delete page" title="Delete page (Del)" disabled={!tab} onClick={() => tab && P.deletePages(tab)} />
    </>
  );
}

export const MODES: [ViewMode, string, typeof File, string][] = [
  ["single", "Single", File, "Single page: the pages as they are"],
  ["double", "Double", Columns2, "Double page: two pages side by side"],
  ["read", "Read", ScrollText, "Read mode: the text reflowed into one comfortable column (Ctrl+H)"],
  ["book", "Book", BookOpen, "Book mode: the text as a two-page book, full screen, with page turns (F5)"],
];

function ViewTools({ tab }: { tab: Tab | null }) {
  const s = settings.value;
  const vm = tab ? viewModeOf(tab) : "single";
  const themeMenu = (e: MouseEvent) =>
    openMenu(e, [
      { label: "Light", check: s.theme === "light", run: () => updateSettings({ theme: "light" }) },
      { label: "Dark", check: s.theme === "dark", run: () => updateSettings({ theme: "dark" }) },
      { label: "Match Windows", check: s.theme === "system", run: () => updateSettings({ theme: "system" }) },
    ]);
  return (
    <>
      {MODES.map(([mode, label, icon, title]) => (
        <TB key={mode} icon={I(icon, 17)} label={label} title={title} on={!!tab && viewModeOf(tab) === mode} disabled={!tab} onClick={() => tab && void setViewMode(tab, mode)} />
      ))}
      <TB icon={I(LayoutGrid, 17)} label="Page grid" title="All pages as big thumbnails, to organize them (Ctrl+Shift+G)" on={!!tab?.grid.value} disabled={!tab} onClick={() => tab && toggleGrid(tab)} />
      <div class="sep" />
      {vm === "double" && (
        <TB icon={I(SquareSplitHorizontal, 17)} label="Cover on its own" title="Show the first page alone, like the cover of a printed book" on={s.coverSeparate} onClick={() => setCoverSeparate(!s.coverSeparate)} />
      )}
      {(vm === "single" || vm === "double") && (
        <>
          <TB icon={I(MoveHorizontal, 17)} label="Fit width" title="Fit width (Ctrl+2)" on={tab?.fit.value === "width"} disabled={!tab} onClick={A.fitWidth} />
          <TB icon={I(Expand, 17)} label="Fit page" title="Fit page (Ctrl+1)" on={tab?.fit.value === "page"} disabled={!tab} onClick={A.fitPage} />
        </>
      )}
      {(vm === "read" || vm === "book") && (
        <>
          <TB icon={I(Type, 17)} label="Text" title="Text size, font, spacing, margins, paper" on={typeOpen.value} onClick={() => (typeOpen.value = !typeOpen.value)} />
          <TB icon={I(Wrench, 17)} label="Fix layout" title="See how the page is read, and correct columns, order, pictures and text" onClick={() => tab && void startLayoutEdit(tab)} />
        </>
      )}
      <div class="sep" />
      <TB icon={I(SunMoon, 17)} label="Dark pages" title="Night mode: dark pages, photos keep their colours (Ctrl+Shift+N)" on={s.night} onClick={A.toggleNight} />
      <TB icon={isDark.value ? I(Moon, 17) : I(Sun, 17)} label="Theme" menu onClick={themeMenu} />
      <TB icon={I(Expand, 17)} label="Full screen" title="Full screen (F11)" onClick={A.toggleFullscreen} />
    </>
  );
}

/** The Forms menu (Tools tab and the form bar). */
export function formItems(tab: Tab | null): MenuItem[] {
  const fields = tab?.forms.value?.fields ?? 0;
  const none = !tab || !fields;
  return [
    { head: none ? "No fillable fields in this document" : `${fields} fillable field${fields === 1 ? "" : "s"}` },
    { label: "Highlight fields", check: settings.value.highlightFields, run: () => updateSettings({ highlightFields: !settings.value.highlightFields }) },
    { label: "Clear form", disabled: none, run: () => tab && void clearForm(tab) },
    { sep: true },
    { label: "Save a flattened copy…", hint: "not fillable", disabled: none, run: () => tab && void P.saveFlattened(tab) },
  ];
}

// ---------------------------------------------------------------- Comment tab

const TOOL_BTNS: [AnnotTool, string, typeof Save, string][] = [
  ["highlight", "Highlight", Highlighter, "Highlight text (on scans: drag over the area) — H"],
  ["underline", "Underline", Underline, "Underline text"],
  ["strike", "Strike", Strikethrough, "Strike through text (on scans: drag a line)"],
  ["area", "Area", SquareDashed, "Highlight an area"],
  ["pen", "Pen", PenLine, "Draw freehand"],
  ["note", "Note", StickyNote, "Add a sticky note"],
  ["text", "Text", TypeOutline, "Add a text box"],
  ["rect", "Box", Square, "Rectangle"],
  ["ellipse", "Circle", Circle, "Ellipse"],
  ["line", "Line", Slash, "Line"],
  ["arrow", "Arrow", MoveUpRight, "Arrow"],
];

function CommentTools({ tab }: { tab: Tab | null }) {
  const tool = annotTool.value;
  const kind = toolStyle(tool);
  const st = styleOf(kind);
  const pick = (t: AnnotTool) => {
    annotTool.value = tool === t ? null : t;
    selectedAnnot.value = null;
  };
  const stampMenu = (e: MouseEvent) =>
    openMenu(e, [
      { head: "Fill & Sign" },
      { label: "✓  Tick", run: () => pick("stamp:check") },
      { label: "✗  Cross", run: () => pick("stamp:cross") },
      { label: "●  Dot", run: () => pick("stamp:dot") },
      { label: "—  Strike line", run: () => pick("fs-strike") },
      { label: "Today's date", run: () => pick("stamp:date") },
      { label: "My name", run: () => pick("stamp:name") },
      { head: "Stamps" },
      ...STAMPS.map(([name, label]) => ({ label, run: () => pick(`stamp:${name}` as AnnotTool) })),
    ], "left", 220);
  const colorMenu = (e: MouseEvent) =>
    openMenu(e, PALETTE.map((c) => ({ label: c, icon: <span class="sw-dot" style={{ background: c }} />, check: c === st.color, run: () => setStyle(kind, { color: c }) })), "left", 170);
  return (
    <>
      <IB icon={I(MousePointer2)} label="Select" title="Select and read (Esc)" on={!tool} onClick={() => (annotTool.value = null)} />
      <div class="sep" />
      {/* The everyday tools have labels; the rest are icons (names in their tooltips) so the bar fits. */}
      {TOOL_BTNS.map(([t, label, icon, title]) =>
        ["highlight", "pen", "note", "text"].includes(t) ? (
          <TB key={t} icon={I(icon, 17)} label={label} title={title} on={tool === t} disabled={!tab} onClick={() => pick(t)} />
        ) : (
          <IB key={t} icon={I(icon)} label={label} title={title} on={tool === t} disabled={!tab} onClick={() => pick(t)} />
        ),
      )}
      <TB icon={I(Stamp, 17)} label="Stamp" menu on={!!tool?.startsWith("stamp:")} disabled={!tab} onClick={stampMenu} />
      <div class="sep" />
      <button class="tb" title="Colour for this tool" onClick={colorMenu}>
        <span class="sw-dot big" style={{ background: st.color }} />
        <ChevronDown size={12} class="chev" />
      </button>
      {(kind === "pen" || kind === "shape") && (
        <select class="inp sm" value={String(st.width)} title="Line thickness" onChange={(e) => setStyle(kind, { width: Number((e.currentTarget as HTMLSelectElement).value) })}>
          {[0.5, 1, 1.5, 2, 3, 5, 8].map((w) => <option key={w} value={String(w)}>{w} pt</option>)}
        </select>
      )}
      {kind === "text" && (
        <select class="inp sm" value={String(st.fontSize)} title="Text size" onChange={(e) => setStyle(kind, { fontSize: Number((e.currentTarget as HTMLSelectElement).value) })}>
          {[8, 9, 10, 11, 12, 14, 16, 18, 24, 32].map((z) => <option key={z} value={String(z)}>{z} pt</option>)}
        </select>
      )}
      <div class="sep" />
      <TB icon={I(MessagesSquare, 17)} label="All comments" title="List every comment in the side panel" onClick={() => updateSettings({ sidebarOpen: true, sidebarTab: "comments" })} />
    </>
  );
}

// ---------------------------------------------------------------- Sign tab

function SignTools({ tab }: { tab: Tab | null }) {
  const tool = annotTool.value;
  useEffect(() => {
    if (!sigs.value) void loadSigs();
  }, []);
  const pick = (t: AnnotTool) => {
    annotTool.value = tool === t ? null : t;
    selectedAnnot.value = null;
  };
  // With none saved yet, the button goes straight to making one (in the Sign pane).
  const library = (kind: SigKind) => (e: MouseEvent) => {
    const saved = sigsOf(kind);
    if (!saved.length) return newSig(kind);
    openMenu(e, [
      ...saved.map((s) => ({
        label: s.name,
        icon: <img class="menu-sig" src={`data:image/png;base64,${s.png}`} alt="" />,
        run: () => pickSig(s.id),
      })),
      { sep: true },
      { label: kind === "initials" ? "New initials…" : "New signature…", icon: <Plus size={15} />, run: () => newSig(kind) },
    ], "left", 280);
  };
  const on = (kind: SigKind) => !!tool?.startsWith("sig:") && sigsOf(kind).some((s) => tool === `sig:${s.id}`);
  return (
    <>
      <TB
        icon={I(PanelRight, 17)}
        label="Sign panel"
        title="Signatures, Fill & Sign and certificates, on the right"
        on={settings.value.signPane}
        onClick={() => updateSettings({ signPane: !settings.value.signPane })}
      />
      <IB icon={I(MousePointer2)} label="Select" title="Select and read (Esc)" on={!tool} onClick={() => (annotTool.value = null)} />
      <div class="sep" />
      <TB icon={I(Signature, 17)} label="Signature" menu on={on("signature")} disabled={!tab} onClick={library("signature")} />
      <TB icon={<span class="initials-ic">AB</span>} label="Initials" menu on={on("initials")} disabled={!tab} onClick={library("initials")} />
      <div class="sep" />
      <TB icon={I(TypeOutline, 17)} label="Text" title="Type anywhere on the page" on={tool === "text"} disabled={!tab} onClick={() => pick("text")} />
      <IB icon={I(Check)} label="Tick" on={tool === "stamp:check"} disabled={!tab} onClick={() => pick("stamp:check")} />
      <IB icon={I(X)} label="Cross" on={tool === "stamp:cross"} disabled={!tab} onClick={() => pick("stamp:cross")} />
      <IB icon={<span class="dot-ic" />} label="Dot" on={tool === "stamp:dot"} disabled={!tab} onClick={() => pick("stamp:dot")} />
      <IB icon={I(Strikethrough)} label="Strike" title="Strike out: drag a straight line across anything (scans too)" on={tool === "fs-strike"} disabled={!tab} onClick={() => pick("fs-strike")} />
      <IB icon={I(CalendarDays)} label="Today's date" on={tool === "stamp:date"} disabled={!tab} onClick={() => pick("stamp:date")} />
      <IB icon={I(UserRound)} label="My name" on={tool === "stamp:name"} disabled={!tab} onClick={() => pick("stamp:name")} />
      <div class="sep" />
      <TB
        icon={I(FileLock2, 17)}
        label="Save locked copy"
        title="Save a copy with signatures, comments and form entries made part of the page, so they can't be moved or edited"
        disabled={!tab?.info.value?.isPdf}
        onClick={() => tab && void P.saveFlattened(tab, true)}
      />
      <TB icon={I(ShieldCheck, 17)} label="Certificate" menu disabled={!tab} onClick={(e) => openMenu(e, [
        { label: "Sign with a DSC token or certificate…", run: () => tab && startSigning(tab) },
        { label: "Signatures in this document", run: () => openSignPane("doc") },
      ], "left", 300)} />
    </>
  );
}

/** Tools that are planned but not built yet show as disabled items with their phase. */
const soon = (label: string, phase: number): MenuItem => ({ label, hint: `Phase ${phase}`, disabled: true, run: () => {} });

function ToolsTools({ tab }: { tab: Tab | null }) {
  return (
    <>
      <TB icon={I(RotateCcw, 17)} label="Rotate" title="Rotate page left (Shift+R) — or hover a page" disabled={!tab} onClick={() => tab && P.rotatePages(tab, -90)} />
      <IB icon={I(RotateCw)} label="Rotate right" title="Rotate page right (R)" disabled={!tab} onClick={() => tab && P.rotatePages(tab, 90)} />
      <TB icon={I(Trash2, 17)} label="Delete" title="Delete page (Del)" disabled={!tab} onClick={() => tab && P.deletePages(tab)} />
      <TB icon={I(FilePlus2, 17)} label="Insert" menu disabled={!tab?.info.value?.isPdf} onClick={(e) => openMenu(e, [
        { label: "Pages from a file…", run: () => tab && void startInsert(tab) },
        { label: "Blank page", run: () => tab && void insertBlankPage(tab) },
      ], "left", 200)} />
      <TB icon={I(TextCursorInput, 17)} label="Edit text" title="Click a paragraph on the page to change its words (Esc to stop)" on={!!tab && textEdit.value === tab} disabled={!tab?.info.value?.isPdf} onClick={() => tab && toggleTextEdit(tab)} />
      <div class="sep" />
      <TB icon={I(FormInput, 17)} label="Forms" menu onClick={(e) => openMenu(e, formItems(tab), "left", 270)} />
      <TB icon={I(Highlighter, 17)} label="Comment" title="Highlight, draw, add notes and text (Comment tab)" onClick={() => updateSettings({ ribbon: "comment" })} />
      <TB icon={I(Signature, 17)} label="Sign" title="Signatures, initials and Fill & Sign (Sign tab)" onClick={() => updateSettings({ ribbon: "sign", signPane: true })} />
      <TB icon={I(ScanText, 17)} label="OCR" menu disabled={!tab} onClick={(e) => openMenu(e, [
        { label: "Recognize text…", run: () => tab && void recognize(tab) },
        { label: "Remove recognized text", disabled: !tab?.info.value?.isPdf, run: () => tab && void removeRecognized(tab) },
      ], "left", 240)} />
      <TB icon={I(Combine, 17)} label="Organize" menu onClick={(e) => openMenu(e, [
        { label: "Page grid", hint: "Ctrl+Shift+G", check: !!tab?.grid.value, disabled: !tab, run: () => tab && toggleGrid(tab) },
        { sep: true },
        { label: "Merge files…", run: () => void startMerge(tab) },
        { label: "Split into files…", disabled: !tab, run: () => tab && startSplit(tab) },
        { label: "Save selected pages as a new PDF…", disabled: !tab, run: () => tab && void extractPages(tab) },
        { sep: true },
        { label: "Crop and resize pages…", disabled: !tab?.info.value?.isPdf, run: () => tab && startPageSize(tab) },
        { sep: true },
        { label: "Page numbers, headers and footers…", disabled: !tab?.info.value?.isPdf, run: () => tab && startHeaders(tab) },
        { label: "Watermark…", disabled: !tab?.info.value?.isPdf, run: () => tab && startWatermark(tab) },
        { label: "Page labels…", disabled: !tab?.info.value?.isPdf, run: () => tab && startLabels(tab) },
        { label: "Document properties…", hint: "Ctrl+D", disabled: !tab, run: () => tab && startProperties(tab) },
        { sep: true },

        { label: "Protect with a password…", disabled: !tab?.info.value?.isPdf, run: () => tab && startSecure(tab, "protect") },
        { label: "Reduce file size…", disabled: !tab?.info.value?.isPdf, run: () => tab && startSecure(tab, "compress") },
        { label: "Repair…", disabled: !tab?.info.value?.isPdf, run: () => tab && startSecure(tab, "repair") },
        { sep: true },
        { head: "Get content out" },
        { label: "Save all pictures…", disabled: !tab, run: () => tab && (picturesDialog.value = tab) },
        { label: "Select an area to copy or save…", hint: "or right-click a page", disabled: !tab, run: () => tab && startArea(tab) },
        { label: `Attached files${tab?.attachments.value?.length ? ` (${tab.attachments.value.length})` : ""}`, disabled: !tab?.attachments.value?.length, run: showAttachments },
        { label: "Table on this page to Excel / CSV…", disabled: !tab, run: () => tab && void openPageTable(tab) },
        { sep: true },
        { label: "Edit text", check: !!tab && textEdit.value === tab, disabled: !tab?.info.value?.isPdf, run: () => tab && toggleTextEdit(tab) },
        soon("Redact", 6.5), soon("Compare", 7.5),
      ], "left", 280)} />
      <TB icon={I(RefreshCw, 17)} label="Convert" menu onClick={(e) => openMenu(e, [
        { head: "From PDF" }, soon("Word", 7), soon("Excel", 7), soon("PowerPoint", 7), soon("Images (JPG, PNG, TIFF)", 7), soon("Text", 7),
        { head: "To PDF" }, soon("Word, Excel, PowerPoint", 7), soon("Images", 7), soon("Web page (HTML)", 7),
      ], "left", 250)} />
    </>
  );
}

function HelpTools() {
  return (
    <>
      <TB icon={I(Keyboard, 17)} label="Keyboard shortcuts" title="Keyboard shortcuts (F1)" onClick={() => (helpPanel.value = "shortcuts")} />
      <TB icon={I(Info, 17)} label="About Paginaut" onClick={() => (helpPanel.value = "about")} />
      <TB icon={I(RefreshCw, 17)} label="Check for updates" title="Coming in a later version (checks the GitHub releases)" disabled onClick={() => {}} />
    </>
  );
}

/** Middle of the status bar, in every tab: page, zoom (text size in Read/Book), view modes. */
export function ViewControls({ tab }: { tab: Tab | null }) {
  const vm = tab ? viewModeOf(tab) : "single";
  const text = vm === "read" || vm === "book";
  const size = settings.value.reflow.size;
  const zoomMenu = (e: MouseEvent) =>
    openMenu(
      e,
      text
        ? [14, 16, 17, 19, 21, 24, 28, 32].map((z) => ({ label: `${z}px`, check: z === size, hint: z === 19 ? "Ctrl+0" : undefined, run: () => A.textSize(z) }))
        : [
            { label: "Fit width", hint: "Ctrl+2", run: A.fitWidth },
            { label: "Fit page", hint: "Ctrl+1", run: A.fitPage },
            { sep: true },
            ...[50, 75, 100, 125, 150, 200, 300, 400].map((z) => ({ label: `${z}%`, hint: z === 100 ? "Ctrl+0" : undefined, run: () => A.zoomTo(z / 100) })),
          ],
      "up",
      180,
    );
  return (
    <div class="viewctl">
      {tab && <PageBox tab={tab} />}
      {tab && <div class="sep" />}
      <button class="ib sm" disabled={!tab} onClick={A.zoomOut} aria-label={text ? "Smaller text" : "Zoom out"} title={text ? "Smaller text (Ctrl+−)" : "Zoom out (Ctrl+−)"}>
        <Minus size={15} />
      </button>
      <button class="btn ghost zoomval" disabled={!tab} onClick={zoomMenu} title={text ? "Text size" : "Zoom"}>
        {!tab ? "—" : text ? `${size}px` : `${Math.round(tab.zoom.value * 100)}%`}
        <ChevronUp size={12} />
      </button>
      <button class="ib sm" disabled={!tab} onClick={A.zoomIn} aria-label={text ? "Larger text" : "Zoom in"} title={text ? "Larger text (Ctrl+=)" : "Zoom in (Ctrl+=)"}>
        <Plus size={15} />
      </button>
      <div class="sep" />
      <div class="modeseg" role="group" aria-label="View mode">
        {MODES.map(([mode, label, icon, title]) => (
          <IB key={mode} icon={I(icon, 15)} label={label} title={title} on={vm === mode} disabled={!tab} onClick={() => tab && void setViewMode(tab, mode)} />
        ))}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- page box

/** Label of a displayed page: the file's own label if it has them, else its number. */
export function labelOf(tab: Tab, view: number) {
  const labels = tab.labels.value;
  const src = tab.edits.value?.order[view];
  return labels && src !== undefined ? labels[src] || String(view + 1) : String(view + 1);
}

function PageBox({ tab }: { tab: Tab }) {
  const page = tab.page.value;
  const hasLabels = !!tab.labels.value;
  const [text, setText] = useState(labelOf(tab, page));
  const [editing, setEditing] = useState(false);
  useEffect(() => {
    if (!editing) setText(labelOf(tab, page));
  }, [page, editing, hasLabels]);
  const go = (t: string) => {
    // A label ("iv", "A-3") wins; otherwise a plain page number.
    const order = tab.edits.value?.order ?? [];
    const labels = tab.labels.value;
    if (labels) {
      const v = order.findIndex((src) => labels[src]?.toLowerCase() === t.trim().toLowerCase());
      if (v >= 0) return A.goToPage(v);
    }
    const n = parseInt(t, 10);
    if (!isNaN(n)) A.goToPage(n - 1);
  };
  return (
    <div class="pagebox">
      <input
        id="page-input"
        class="inp"
        value={text}
        aria-label="Page number"
        onFocus={(e) => {
          setEditing(true);
          (e.currentTarget as HTMLInputElement).select();
        }}
        onBlur={() => setEditing(false)}
        onInput={(e) => setText((e.currentTarget as HTMLInputElement).value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            go((e.currentTarget as HTMLInputElement).value);
            (e.currentTarget as HTMLInputElement).blur();
            tab.viewer?.focus();
          } else if (e.key === "Escape") {
            (e.currentTarget as HTMLInputElement).blur();
            tab.viewer?.focus();
          }
        }}
      />
      <span>{hasLabels ? `(${page + 1} of ${tab.pageCount.value})` : `/ ${tab.pageCount.value}`}</span>
    </div>
  );
}
