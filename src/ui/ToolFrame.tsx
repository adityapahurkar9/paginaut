// The frame of a tool in the pane on the right (toolpanes.ts): its name, a way back to the pane it was opened from,
// a close button, and its contents (the action buttons stay in view at the bottom).
import type { ComponentChildren } from "preact";
import { ChevronLeft, X } from "lucide-preact";
import { bookScreen } from "../modes";
import { pane } from "../panes";
import { closeToolPane, currentTool, toolBack, toolGoBack } from "../toolpanes";
import { PANES } from "./Rail";

export function ToolFrame({ id, title, class: cls = "", children }: { id: string; title: string; class?: string; children: ComponentChildren }) {
  if (pane.value !== "tool" || currentTool.value !== id || bookScreen.value) return null;
  const back = toolBack.value;
  return (
    <aside class="signpane toolpane" aria-label={title}>
      <div class="signpane-head">
        {back ? (
          <button class="toolpane-back" title={`Back to ${PANES[back].label}`} onClick={toolGoBack}>
            <ChevronLeft size={15} />
            <span>{PANES[back].label}</span>
          </button>
        ) : null}
        <strong>{title}</strong>
        <button class="ib sm" aria-label={`Close ${title}`} title="Close" onClick={closeToolPane}>
          <X size={15} />
        </button>
      </div>
      <div class={`signpane-body toolpane-body ${cls}`}>{children}</div>
    </aside>
  );
}
