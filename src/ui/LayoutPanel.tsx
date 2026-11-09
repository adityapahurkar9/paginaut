// The "Fix layout" panel: shown over the page view while correcting how Read
// and Book modes read the document.
import { effect } from "@preact/signals";
import { Check, MousePointer2, ListOrdered, Image, EyeOff, Type, RotateCcw } from "lucide-preact";
import { layoutEdit, LayoutLayer, type LayoutTool } from "../reflow/LayoutEditor";
import { overridesOf, resetPage, setDocColumns, updatePage } from "../reflow/overrides";
import { setViewMode } from "../modes";
import { activeTab, showToast, type Tab } from "../store";
import { analysisOf } from "./Reflow";
import { toolPane } from "../toolpanes";
import { ToolFrame } from "./ToolFrame";

let layer: LayoutLayer | null = null;

toolPane("layout", layoutEdit, { tabOf: (v) => v.tab, close: endLayoutEdit });

/** Opens the page view with the reading layout drawn over it. */
export async function startLayoutEdit(tab: Tab) {
  const back = tab.mode.value === "book" ? "book" : "read";
  let analysis;
  try {
    analysis = await analysisOf(tab).analysis;
  } catch (e) {
    showToast(`Couldn't read the layout: ${e}`);
    return;
  }
  await setViewMode(tab, tab.layout.value === "single" ? "single" : "double");
  layoutEdit.value = { tab, back, tool: "select" };
  // The page viewer becomes active on the next render.
  requestAnimationFrame(() => {
    if (!tab.viewer || layoutEdit.value?.tab !== tab) return;
    layer = new LayoutLayer(tab, tab.viewer, analysis);
    tab.viewer.addLayer(layer);
  });
}

function dropLayer() {
  const st = layoutEdit.value;
  if (layer && st) st.tab.viewer?.removeLayer(layer);
  layer = null;
}

/** Back to Read/Book mode, at the page you were fixing. */
export function endLayoutEdit() {
  const st = layoutEdit.value;
  if (!st) return;
  dropLayer();
  layoutEdit.value = null;
  st.tab.reflowBlock = -1;
  void setViewMode(st.tab, st.back);
}

// Leaving the page view any other way (switching mode or tab) ends editing too.
effect(() => {
  const st = layoutEdit.value;
  if (!st) return;
  if (st.tab.mode.value !== "pages" || activeTab.value !== st.tab) {
    dropLayer();
    layoutEdit.value = null;
  }
});

const HINTS: Record<LayoutTool, string> = {
  select: "Click a box to make it text, a picture, or leave it out.",
  order: "Click the boxes in the order they should be read. The rest follow in their current order.",
  picture: "Drag a rectangle: everything inside becomes one picture.",
  ignore: "Drag a rectangle: everything inside is left out (side notes, stamps…).",
  text: "Drag a rectangle: text inside is kept even if it looked like part of a picture.",
};

const TOOLS: [LayoutTool, string, typeof Check][] = [
  ["select", "Change", MousePointer2],
  ["order", "Set order", ListOrdered],
  ["picture", "Picture", Image],
  ["ignore", "Leave out", EyeOff],
  ["text", "Text", Type],
];

export function LayoutPanel() {
  const st = layoutEdit.value;
  if (!st) return null;
  const tab = st.tab;
  const ov = overridesOf(tab).value;
  const view = tab.page.value;
  const src = tab.edits.value?.order[view] ?? view;
  const pageOv = ov.pages?.[src];
  const setTool = (tool: LayoutTool) => {
    layoutEdit.value = { ...st, tool };
    if (tool === "order") layer?.startOrder(src);
    else layer?.endOrder();
  };
  const cols = (value: number | undefined, set: (n: number) => void) => (
    <div class="seg">
      {[0, 1, 2, 3].map((n) => (
        <button key={n} class={`segb ${(value ?? 0) === n ? "on" : ""}`} onClick={() => set(n)}>
          {n === 0 ? "Auto" : n}
        </button>
      ))}
    </div>
  );
  return (
    <ToolFrame id="layout" title="Fix reading layout" class="lp-pane">
      <span class="hint">How Read and Book modes read page {view + 1}: boxes on the page show what's text, pictures and left out.</span>
      <div class="lp-row">
        <label>Columns, whole document</label>
        {cols(ov.columns, (n) => {
          setDocColumns(tab, n);
          layer?.refresh();
        })}
      </div>
      <div class="lp-row">
        <label>Columns, this page</label>
        {cols(pageOv?.columns, (n) => {
          updatePage(tab, src, (p) => ({ ...p, columns: n || undefined }));
          layer?.refresh();
        })}
      </div>
      <div class="lp-tools">
        {TOOLS.map(([tool, label, Icon]) => (
          <button key={tool} class={`tb ${st.tool === tool ? "on" : ""}`} onClick={() => setTool(tool)} title={HINTS[tool]}>
            <Icon size={16} strokeWidth={1.8} />
            <span>{label}</span>
          </button>
        ))}
      </div>
      <p class="lp-hint">{HINTS[st.tool]}</p>
      <div class="lp-legend">
        <span class="sw-t" /> text <span class="sw-p" /> picture <span class="sw-x" /> left out
      </div>
      <div class="row">
        <button class="btn" disabled={!pageOv} onClick={() => { resetPage(tab, src); layer?.refresh(); }} title="Forget the corrections on this page">
          <RotateCcw size={14} /> Reset page
        </button>
        <button class="btn pri" onClick={endLayoutEdit}>
          <Check size={15} /> Done
        </button>
      </div>
    </ToolFrame>
  );
}
