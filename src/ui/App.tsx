import { useEffect, useRef, useState } from "preact/hooks";
import { signal } from "@preact/signals";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { BookOpen, FileDiff, FileImage, FileSpreadsheet, FileText, House, Plus, Presentation, X } from "lucide-preact";
import { Logo } from "./Logo";
import * as A from "../actions";
import { api, type SystemInfo } from "../api";
import { Viewer } from "../render/Viewer";
import * as P from "../pages";
import { specs } from "../edits";
import { renderer } from "../render/RenderManager";
import { bookScreen } from "../modes";
import { activeKey, activeTab, closeHome, closeTab, findOpen, homes, minimized, openHome, recent, settings, tabs, type Tab } from "../store";
import { Sidebar } from "./Sidebar";
import { AskDialog, FindBar, Palette, PasswordDialog, Toast } from "./Overlays";
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
import { OfficeBar, openingText, RepairBar } from "./OfficeUI";
import { ConvertPane } from "./ConvertPane";
import { MarkdownPane } from "./MarkdownPane";
import { OrganizePane } from "./OrganizePane";
import { Rail } from "./Rail";
import { ReadingPane } from "./ReadingPane";
import { SettingsDialog } from "./SettingsDialog";
import { CompareDialog, CompareView } from "./CompareUI";
import { closeCompare, compare, COMPARE } from "../compare";
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
import { SpeechBar } from "./SpeechBar";
import { PrintBar } from "./PrintBar";

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
            {compare.value && <CompareView active={activeKey.value === COMPARE} />}
            {!tab && activeKey.value !== COMPARE && <Home />}
            {tab && findOpen.value && <FindBar tab={tab} />}
            {tab && <LayoutPanel tab={tab} />}
            {tab && <PageSizePanel tab={tab} />}
            {tab && <RedactPanel tab={tab} />}
            {tab && tab.mode.value === "pages" && selectedAnnot.value?.tab !== tab && (
              <div class="topbars">
                <SigBar tab={tab} />
                <OfficeBar tab={tab} />
                <RepairBar tab={tab} />
                <OcrBar tab={tab} />
                <FormBar tab={tab} />
              </div>
            )}
            {tab && tab.mode.value === "read" && (
              <div class="topbars">
                <RepairBar tab={tab} />
              </div>
            )}
            {tab && tab.mode.value === "pages" && <AnnotBar tab={tab} />}
            <SpeechBar />
            <PrintBar />
          </div>
        </div>
        <OrganizePane />
        <SignPane />
        <ConvertPane />
        <MarkdownPane />
        <ReadingPane />
        <Rail />
      </div>
      <StatusBar />
      <Palette />
      <AskDialog />
      <OcrDialog />
      <CompareDialog />
      <SettingsDialog />
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

/** Windows 11 Snap Layouts on our maximize button: a native window sits over it (snap.rs), told where it is here;
 * it takes the mouse there, so it says when the mouse is over it and the button lights up as on hover. */
function useSnapButton() {
  const ref = useRef<HTMLButtonElement>(null);
  const [hot, setHot] = useState(false);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    let raf = 0;
    const report = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        const r = el.getBoundingClientRect(), k = devicePixelRatio;
        const shown = r.width > 0 && r.height > 0 && getComputedStyle(el).visibility !== "hidden";
        invoke("snap_button", { x: Math.round(r.left * k), y: Math.round(r.top * k), w: Math.round(r.width * k), h: Math.round(r.height * k), shown }).catch(() => {});
      });
    };
    const ro = new ResizeObserver(report);
    ro.observe(el);
    ro.observe(document.documentElement);
    window.addEventListener("resize", report);
    const unlisten = listen<boolean>("snap-hover", (e) => setHot(e.payload));
    report();
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      window.removeEventListener("resize", report);
      void unlisten.then((f) => f());
    };
  }, []);
  return { ref, hot };
}

function TitleBar() {
  const win = getCurrentWindow();
  const snap = useSnapButton();
  return (
    <div class="titlebar">
      <div class="brand" data-tauri-drag-region>
        <Logo size={20} tile />
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
        {compare.value && (
          <button
            class={`tab ${activeKey.value === COMPARE ? "on" : ""}`}
            title={`${compare.value.a.path}\n${compare.value.b.path}`}
            onMouseDown={(e) => {
              if (e.button === 1) {
                e.preventDefault();
                closeCompare();
              } else if (e.button === 0) activeKey.value = COMPARE;
            }}
          >
            {compare.value.state === "opening" || compare.value.state === "comparing" ? <span class="spin" /> : <FileDiff size={15} />}
            <span class="name">
              Compare: {compare.value.a.name} · {compare.value.b.name}
            </span>
            <span
              class="x"
              role="button"
              aria-label="Close the comparison"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={(e) => {
                e.stopPropagation();
                closeCompare();
              }}
            >
              <X size={13} strokeWidth={2.2} />
            </span>
          </button>
        )}
        {homes.value.map((k) => (
          <button
            key={k}
            class={`tab ${activeKey.value === k ? "on" : ""}`}
            onMouseDown={(e) => {
              if (e.button === 1) {
                e.preventDefault();
                closeHome(k);
              } else if (e.button === 0) activeKey.value = k;
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
                closeHome(k);
              }}
            >
              <X size={13} strokeWidth={2.2} />
            </span>
          </button>
        ))}
        <button class="ib sm" style={{ marginBottom: 3, marginLeft: 2 }} aria-label="New tab" title="New tab (Ctrl+T)" onClick={openHome}>
          <Plus size={16} />
        </button>
      </div>
      <div class="drag" data-tauri-drag-region />
      <div class="winbtns">
        <button class="winbtn" aria-label="Minimize" onClick={() => win.minimize()}>
          <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 5h10" stroke="currentColor" /></svg>
        </button>
        <button ref={snap.ref} class={`winbtn ${snap.hot ? "hot" : ""}`} aria-label="Maximize" onClick={() => win.toggleMaximize()}>
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
  const hidden = minimized.value;
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let alive = true;
    const tick = async () => {
      setBusy(renderer.pending > 0);
      const s = await api.systemInfo().catch(() => null);
      if (alive && s) setSys(s);
    };
    // Every 3 s while the window can be seen; nothing while it's minimized.
    if (hidden) return;
    tick();
    const id = setInterval(tick, 3000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, [hidden]);
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
