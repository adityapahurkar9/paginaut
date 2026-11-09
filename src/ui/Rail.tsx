// The side bar at the right edge of the window: a button per pane (Organize, Sign, Redact, Convert, Markdown, Reading). Which
// buttons it shows, and in what order, is chosen in View › Side bar, by right-clicking it, or in Settings.
import { BookOpenText, Combine, EyeOff, RefreshCw, Signature, SlidersHorizontal, TextCursorInput } from "lucide-preact";
import { isMarkdown, openMdPane } from "../markdown";
import { bookScreen } from "../modes";
import { onRail, openPane, pane, PANE_IDS, railItems, railShown, setOnRail, setRailShown, togglePane, type PaneId } from "../panes";
import { startRedact } from "../redact";
import { openSignPane } from "../sign";
import { activeTab, openSettings } from "../store";
import type { MenuItem } from "./Overlays";
import { openMenuAt } from "./Toolbar";

export const PANES: Record<PaneId, { label: string; title: string; icon: typeof Combine; available: () => boolean; open: () => void }> = {
  organize: {
    label: "Organize",
    title: "Organize: pages (grid, insert, merge, split, crop), page numbers and watermarks, protection, pictures and tables out",
    icon: Combine,
    available: () => true,
    open: () => openPane("organize"),
  },
  sign: {
    label: "Sign",
    title: "Sign: your signatures and initials, Fill & Sign, signing with a certificate, the document's signatures",
    icon: Signature,
    available: () => true,
    open: () => openSignPane(),
  },
  redact: {
    label: "Redact",
    title: "Redact: mark text and areas (by hand or by search), remove or cover them; sanitize the document",
    icon: EyeOff,
    available: () => !!activeTab.value?.info.value?.isPdf,
    open: () => {
      const t = activeTab.value;
      if (t) startRedact(t);
    },
  },
  convert: {
    label: "Convert",
    title: "Convert: this document to Word, Excel, PowerPoint, Markdown, pictures or text; files to PDF",
    icon: RefreshCw,
    available: () => true,
    open: () => openPane("convert"),
  },
  markdown: {
    label: "Markdown",
    title: "Edit the Markdown source (for .md files): the pages follow as you type",
    icon: TextCursorInput,
    available: () => !!activeTab.value && isMarkdown(activeTab.value.path),
    open: () => {
      const t = activeTab.value;
      if (t) void openMdPane(t);
    },
  },
  reading: {
    label: "Reading",
    title: "Reading: how Read and Book modes show the text (size, font, spacing, margins, paper)",
    icon: BookOpenText,
    available: () => !!activeTab.value,
    open: () => openPane("reading"),
  },
};

/** Ticks for the side bar's buttons (View › Side bar, right-click on the side bar). */
export function railMenuItems(): MenuItem[] {
  return [
    { head: "On the side bar" },
    ...PANE_IDS.map((id) => ({
      label: PANES[id].label,
      check: onRail(id),
      hint: id === "markdown" ? ".md files" : undefined,
      run: () => setOnRail(id, !onRail(id)),
    })),
    { sep: true },
    { label: "Show the side bar", check: railShown(), run: () => setRailShown(!railShown()) },
    { label: "Arrange…", run: () => openSettings("sidebar") },
  ];
}

export function Rail() {
  if (!railShown() || bookScreen.value) return null;
  const items = railItems().filter((id) => PANES[id].available());
  const cur = pane.value;
  return (
    <nav
      class="rail"
      aria-label="Panes"
      onContextMenu={(e) => {
        e.preventDefault();
        openMenuAt(e.clientX - 230, e.clientY, railMenuItems(), 230);
      }}
    >
      {items.map((id) => {
        const p = PANES[id];
        const Icon = p.icon;
        const on = cur === id;
        return (
          <button key={id} class={`rail-btn ${on ? "on" : ""}`} title={p.title} aria-pressed={on} onClick={() => (on ? togglePane(id) : p.open())}>
            <Icon size={16} strokeWidth={1.8} />
            <span>{p.label}</span>
          </button>
        );
      })}
      <div class="grow" />
      <button class="rail-btn edit" title="Choose what's on the side bar" aria-label="Choose what's on the side bar" onClick={(e) => {
        const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
        openMenuAt(r.left - 234, r.top, railMenuItems(), 230);
      }}>
        <SlidersHorizontal size={15} strokeWidth={1.7} />
      </button>
    </nav>
  );
}
