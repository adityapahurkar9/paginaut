import { useEffect, useRef, useState } from "preact/hooks";
import { signal } from "@preact/signals";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { BookOpen, FileImage, FileSpreadsheet, FileText, House, Plus, Presentation, X } from "lucide-preact";
import * as A from "../actions";
import { api, type SystemInfo } from "../api";
import { Viewer } from "../render/Viewer";
import * as P from "../pages";
import { specs } from "../edits";
import { renderer } from "../render/RenderManager";
import { bookScreen } from "../modes";
import { activeKey, activeTab, closeHome, closeTab, findOpen, HOME, homeOpen, openHome, recent, settings, tabs, type Tab } from "../store";
import { Sidebar } from "./Sidebar";
import { AskDialog, FindBar, Palette, PasswordDialog, Toast } from "./Overlays";
import { HelpDialogs } from "./HelpDialogs";
import { Ribbon, Toolbar, ViewControls } from "./Toolbar";
import { ReflowHost } from "./Reflow";
import { LayoutPanel } from "./LayoutPanel";
import { PageSizePanel } from "./PageSizePanel";
import { RedactPanel } from "./RedactPanel";
import { markFromSelection, redactTab } from "../redact";
import { FormBar } from "./FormBar";
import { fieldHighlightClass, initForms } from "../forms";
import { annotTool, attachAnnotLayer, markupFromSelection } from "../annots";
import { AnnotBar } from "./AnnotBar";
import { Home } from "./Home";
import { SignPane } from "./SignPane";
import { SigBar } from "./SigPanel";
import { OcrBar, OcrDialog } from "./OcrUI";
import { OfficeBar, OfficeDialog, openingText } from "./OfficeUI";
import { ConvertPane } from "./ConvertPane";
import { officeKind } from "../office";
import { InsertDialog, MergeDialog, SplitDialog } from "./PagesUI";
import { HeaderDialog, WatermarkDialog } from "./StampUI";
import { LabelsDialog, PropertiesDialog } from "./MetaUI";
import { SecureDialog } from "./SecureUI";
import { PicturesDialog } from "./ExtractUI";
import { TableDialog } from "./TableUI";
import { PageDragGhost } from "./Sidebar";
import { PageGrid } from "./PageGrid";
import { selectedAnnot } from "../annots";

export const tool = signal<"select" | "hand">("select");
export const dropping = signal(false);

export function App() {
  const tab = activeTab.value;
  return (
    <div class={`app ${bookScreen.value ? "reading" : ""}`}>
      <TitleBar />
      <Ribbon />
      <Toolbar />
      <div class="body">
        <Sidebar />
        <div class="center">
          <div class="host">
            {tabs.value.map((t) => (
              <TabView key={t.key} tab={t} active={t.key === activeKey.value} />
            ))}
            {!tab && <Home />}
            {tab && findOpen.value && <FindBar tab={tab} />}
            {tab && <LayoutPanel tab={tab} />}
            {tab && <PageSizePanel tab={tab} />}
            {tab && <RedactPanel tab={tab} />}
            {tab && tab.mode.value === "pages" && selectedAnnot.value?.tab !== tab && (
              <div class="topbars">
                <SigBar tab={tab} />
                <OfficeBar tab={tab} />
                <OcrBar tab={tab} />
                <FormBar tab={tab} />
              </div>
            )}
            {tab && tab.mode.value === "pages" && <AnnotBar tab={tab} />}
          </div>
        </div>
        <SignPane />
        <ConvertPane />
      </div>
      <StatusBar />
      <Palette />
      <AskDialog />
      <HelpDialogs />
      <OcrDialog />
      <OfficeDialog />
      <InsertDialog />
      <MergeDialog />
      <SplitDialog />
      <HeaderDialog />
      <WatermarkDialog />
      <PropertiesDialog />
      <LabelsDialog />
      <SecureDialog />
      <PicturesDialog />
      <TableDialog />
      <PageDragGhost />
      <Toast />
      {dropping.value && <div class="drop-veil" />}
    </div>
  );
}

// ---------------------------------------------------------------- title bar

function tabIcon(t: Tab) {
  const ext = t.path.split(".").pop()?.toLowerCase() ?? "";
  if (["epub", "mobi", "fb2", "fbz"].includes(ext)) return <BookOpen size={15} />;
  if (["png", "jpg", "jpeg", "tif", "tiff", "bmp", "gif", "jp2", "jpx", "psd", "jfif"].includes(ext)) return <FileImage size={15} />;
  const office = officeKind(t.path);
  if (office === "excel") return <FileSpreadsheet size={15} />;
  if (office === "powerpoint") return <Presentation size={15} />;
  return <FileText size={15} />;
}

function TitleBar() {
  const win = getCurrentWindow();
  return (
    <div class="titlebar">
      <div class="brand" data-tauri-drag-region>
        <svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true">
          <path d="M5 2h9l6 6v14H5z" fill="var(--acc)" />
          <path d="M14 2v6h6" fill="none" stroke="var(--chrome)" stroke-width="1.6" stroke-linejoin="round" />
          <path d="M8.5 13h7M8.5 16.5h5" stroke="var(--on-acc)" stroke-width="1.6" stroke-linecap="round" />
        </svg>
        <span data-tauri-drag-region>Paginaut</span>
      </div>
      <div class="tabs">
        {tabs.value.map((t) => (
          <button
            key={t.key}
            class={`tab ${t.key === activeKey.value ? "on" : ""}`}
            data-tab={t.key}
            title={t.path}
            onMouseDown={(e) => {
              if (e.button === 1) {
                e.preventDefault();
                closeTab(t);
              } else if (e.button === 0) activeKey.value = t.key;
            }}
          >
            {t.status.value === "loading" || t.saving.value ? <span class="spin" /> : tabIcon(t)}
            <span class="name">{t.name}</span>
            {t.dirty.value && <span class="dot" title="Unsaved changes" />}
            <span
              class="x"
              role="button"
              aria-label={`Close ${t.name}`}
              onMouseDown={(e) => e.stopPropagation()}
              onClick={(e) => {
                e.stopPropagation();
                closeTab(t);
              }}
            >
              <X size={13} strokeWidth={2.2} />
            </span>
          </button>
        ))}
        {homeOpen.value && (
          <button
            class={`tab ${activeKey.value === HOME ? "on" : ""}`}
            onMouseDown={(e) => {
              if (e.button === 1) {
                e.preventDefault();
                closeHome();
              } else if (e.button === 0) activeKey.value = HOME;
            }}
          >
            <House size={15} />
            <span class="name">Home</span>
            <span
              class="x"
              role="button"
              aria-label="Close Home"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={(e) => {
                e.stopPropagation();
                closeHome();
              }}
            >
              <X size={13} strokeWidth={2.2} />
            </span>
          </button>
        )}
        <button class="ib sm" style={{ marginBottom: 3, marginLeft: 2 }} aria-label="New tab" title="New tab (Ctrl+T)" onClick={openHome}>
          <Plus size={16} />
        </button>
      </div>
      <div class="drag" data-tauri-drag-region />
      <div class="winbtns">
        <button class="winbtn" aria-label="Minimize" onClick={() => win.minimize()}>
          <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 5h10" stroke="currentColor" /></svg>
        </button>
        <button class="winbtn" aria-label="Maximize" onClick={() => win.toggleMaximize()}>
          <svg width="10" height="10" viewBox="0 0 10 10"><rect x=".5" y=".5" width="9" height="9" fill="none" stroke="currentColor" /></svg>
        </button>
        <button class="winbtn close" aria-label="Close" onClick={() => win.close()}>
          <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" /></svg>
        </button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- per-tab viewer host

function TabView({ tab, active }: { tab: Tab; active: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  const status = tab.status.value;

  useEffect(() => {
    if (status !== "ready" || tab.viewer || !host.current) return;
    const info = tab.info.value!;
    const v = new Viewer(host.current, info, {
      onPage: (p) => (tab.page.value = p),
      onZoom: (z, fit) => {
        tab.zoom.value = z;
        tab.fit.value = fit;
      },
      onSelection: (has) => (tab.hasSelection.value = has),
      onSelectionEnd: (src, quads, a, b) => void (redactTab.value === tab ? markFromSelection(tab, src, quads, a, b) : markupFromSelection(tab, src, quads, a, b)),
      onPageAction: (page, action) => {
        if (action === "delete") P.deletePages(tab, page);
        else P.rotatePages(tab, action === "rotate-left" ? -90 : 90, page);
      },
    });
    tab.viewer = v;
    v.setTool(tool.value);
    v.setLayoutMode(tab.layout.value);
    void initForms(tab);
    P.runDocReady(tab);
    if (tab.info.value?.isPdf) attachAnnotLayer(tab);
    const prev = recent.value.find((r) => r.path.toLowerCase() === tab.path.toLowerCase())?.view;
    v.restore(prev ?? { page: 0, frac: 0, zoom: 1, fit: "width" });
    if (tab.sizes.value && tab.edits.value) v.setPages(specs(tab.edits.value, tab.sizes.value));
    if (active) v.focus();
  }, [status]);

  // Real page sizes arrive shortly after opening.
  const sizes = tab.sizes.value;
  useEffect(() => {
    if (sizes && tab.viewer && tab.edits.value) tab.viewer.setPages(specs(tab.edits.value, sizes));
  }, [sizes]);

  const layout = tab.layout.value;
  useEffect(() => tab.viewer?.setLayoutMode(layout), [layout]);

  const night = settings.value.night;
  useEffect(() => tab.viewer?.setNight(night), [night, status]);

  const t = tool.value;
  useEffect(() => tab.viewer?.setTool(t), [t]);

  // Read and Book modes and the page grid replace the page viewer; it gives back its memory meanwhile.
  const grid = tab.grid.value && status === "ready";
  const reflowing = tab.mode.value !== "pages" || grid;
  const wasReflowing = useRef(false);
  useEffect(() => {
    const v = tab.viewer;
    if (!v) return;
    v.setActive(active && !reflowing);
    if (active && !reflowing) {
      // Coming back from Read/Book: open the page you were reading.
      if (wasReflowing.current) v.goToPage(tab.page.value);
      v.focus();
    }
    wasReflowing.current = reflowing;
  }, [active, reflowing]);

  return (
    <div class={`tabhost ${settings.value.night ? "night" : ""} ${reflowing ? "reflowing" : ""} ${fieldHighlightClass()} ${annotTool.value ? `tool-${annotTool.value.split(":")[0]}` : ""}`} style={{ display: active ? "block" : "none" }} ref={host}>
      {grid && active && <PageGrid key={tab.info.value?.docId} tab={tab} />}
      {!grid && reflowing && status === "ready" && active && <ReflowHost key={tab.info.value?.docId} tab={tab} />}
      {status === "loading" && <div class="state-msg">{openingText(tab)}</div>}
      {status === "error" && (
        <div class="state-msg">
          <strong>Couldn't open {tab.name}</strong>
          <span class="error-text">{tab.error.value}</span>
        </div>
      )}
      {status === "password" && active && <PasswordDialog tab={tab} />}
    </div>
  );
}

// ---------------------------------------------------------------- status bar

const fmtBytes = (n: number) =>
  n > 1 << 30 ? `${(n / (1 << 30)).toFixed(1)} GB` : n > 1 << 20 ? `${(n / (1 << 20)).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`;

function StatusBar() {
  const tab = activeTab.value;
  const ready = tab?.status.value === "ready" ? tab : null;
  const info = tab?.info.value;
  const [sys, setSys] = useState<SystemInfo | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let alive = true;
    const tick = async () => {
      setBusy(renderer.pending > 0);
      const s = await api.systemInfo().catch(() => null);
      if (alive && s) setSys(s);
    };
    tick();
    const id = setInterval(tick, 3000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);
  return (
    <div class="statusbar">
      <div class="sb-side">
        {ready && info ? (
          <span class="muted" title={[info.format || "Document", fmtBytes(info.fileSize), info.title].filter(Boolean).join(" · ")}>
            {[info.format || "Document", fmtBytes(info.fileSize), info.title].filter(Boolean).join(" · ")}
          </span>
        ) : (
          <span class="muted">Ready</span>
        )}
      </div>
      <ViewControls tab={ready} />
      <div class="sb-side end">
        {busy && <span class="busy-dot" title="Rendering" />}
        {sys && (
          <span class="muted" title={`Memory used by Paginaut (all its processes) · ${sys.cpus} logical processors, ${sys.workers} render threads · ${fmtBytes(renderer.cache.size)} page cache`}>
            {fmtBytes(sys.processMemory)} · {sys.workers} threads
          </span>
        )}
      </div>
    </div>
  );
}
