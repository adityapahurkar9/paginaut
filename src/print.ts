// Printing (File › Print, Ctrl+P): Windows' own print dialog, then the pages as they're shown (in the order and
// turned as arranged, with comments and form entries) drawn for the printer by the app.
import { signal } from "@preact/signals";
import { api } from "./api";
import { baseName, showToast, type Tab } from "./store";

/** A print under way: pages sent so far, of how many (0 while the dialog is open). */
export const printing = signal<{ done: number; total: number } | null>(null);

export async function printTab(tab: Tab) {
  const info = tab.info.value;
  if (!info || tab.status.value !== "ready" || printing.value) return;
  const order = tab.edits.value?.order ?? Array.from({ length: info.pageCount }, (_, i) => i);
  const rot = tab.edits.value?.rot ?? [];
  const pages = order.map((page) => ({ page, rot: rot[page] ?? 0 }));
  printing.value = { done: 0, total: 0 };
  try {
    const n = await api.print(info.docId, baseName(tab.path), pages, Math.min(tab.page.value, pages.length - 1), (e) => {
      if (e.kind === "started") printing.value = { done: 0, total: e.total };
      else if (printing.value) printing.value = { ...printing.value, done: e.done };
    });
    if (n != null) showToast(n === 1 ? "Sent 1 page to the printer." : `Sent ${n} pages to the printer.`, 3500);
  } catch (e) {
    // "cancelled": a printer that writes files asked where, and wasn't told.
    if (String(e) === "stopped") showToast("Printing stopped.");
    else if (String(e) !== "cancelled") showToast(`Couldn't print: ${e}`, 6000);
  } finally {
    printing.value = null;
  }
}

export const stopPrinting = () => void api.printStop();
