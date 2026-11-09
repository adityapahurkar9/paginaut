// Tools that work on the document open in the pane on the right (pane "tool"), not in dialogs over the page, so the
// page stays in view while they're set up: crop and resize, page numbers and watermarks, page labels, inserting,
// merging and splitting, text recognition, tables, and Read mode's layout fixes. One tool at a time; each keeps its
// own signal (null when closed), and opening it is setting that signal. A tool opened from another pane (Organize)
// goes back to it when it's done; its close button closes the pane.
import { effect, signal, type Signal } from "@preact/signals";
import { pane, type PaneId } from "./panes";
import { activeTab, type Tab } from "./store";

/** The tool in the pane, if any. */
export const currentTool = signal<string | null>(null);
/** The pane the tool was opened from, to go back to when it's done. */
export const toolBack = signal<PaneId | null>(null);

interface Entry {
  sig: Signal<unknown>;
  /** The document the tool works on: it closes when another tab comes forward. */
  tabOf?: (v: never) => Tab | null;
  /** Closes the tool (when it isn't simply its signal set to null). */
  close?: () => void;
}
const tools = new Map<string, Entry>();
/** Set while the pane's own close button closes the tool: no going back to the pane it came from. */
let closingAll = false;

const shut = (id: string) => {
  const t = tools.get(id);
  if (!t) return;
  if (t.close) t.close();
  else t.sig.value = null;
};

/** Makes the tool whose state is `sig` open in the pane whenever `sig` holds something. */
export function toolPane<T>(id: string, sig: Signal<T | null>, opts: { tabOf?: (v: T) => Tab | null; close?: () => void } = {}) {
  tools.set(id, { sig: sig as Signal<unknown>, tabOf: opts.tabOf as Entry["tabOf"], close: opts.close });
  effect(() => {
    const v = sig.value;
    if (v != null && v !== false) {
      if (currentTool.peek() === id && pane.peek() === "tool") return;
      const from = pane.peek();
      const cur = currentTool.peek();
      if (cur && cur !== id) {
        // Another tool gives way (and keeps the pane it came from).
        currentTool.value = id;
        shut(cur);
      } else {
        toolBack.value = from && from !== "tool" ? from : null;
        currentTool.value = id;
      }
      pane.value = "tool";
    } else if (currentTool.peek() === id) {
      currentTool.value = null;
      const back = closingAll ? null : toolBack.peek();
      toolBack.value = null;
      if (pane.peek() === "tool") pane.value = back;
    }
  });
}

/** The pane's close button: the tool closes, and so does the pane. */
export function closeToolPane() {
  const id = currentTool.peek();
  closingAll = true;
  try {
    if (id) shut(id);
    if (pane.peek() === "tool") pane.value = null;
  } finally {
    closingAll = false;
  }
}

/** Back to the pane the tool came from. */
export function toolGoBack() {
  const id = currentTool.peek();
  if (id) shut(id);
}

// Another pane taking the place closes the tool; so does another document coming forward.
effect(() => {
  const p = pane.value;
  const id = currentTool.peek();
  if (p !== "tool" && id) {
    currentTool.value = null;
    toolBack.value = null;
    shut(id);
  }
});
effect(() => {
  const t = activeTab.value;
  const id = currentTool.peek();
  const e = id ? tools.get(id) : undefined;
  const v = e?.sig.peek();
  if (e?.tabOf && v != null && e.tabOf(v as never) !== t) closeToolPane();
});
