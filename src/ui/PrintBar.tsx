// While printing: how far it's got, and a way to stop.
import { Printer } from "lucide-preact";
import { printing, stopPrinting } from "../print";
import { speaking } from "../speech";

export function PrintBar() {
  const p = printing.value;
  // Nothing to show while Windows' dialog is open.
  if (!p || !p.total) return null;
  return (
    <div class={`speechbar printbar ${speaking.value ? "above" : ""}`} role="status">
      <Printer size={15} strokeWidth={1.7} />
      <span class="pb-text">
        Printing page {Math.min(p.done + 1, p.total)} of {p.total}
      </span>
      <span class="rf-bar pb-bar">
        <span style={{ width: `${Math.round((p.done / p.total) * 100)}%` }} />
      </span>
      <button class="linkish" onClick={stopPrinting}>
        Stop
      </button>
    </div>
  );
}
