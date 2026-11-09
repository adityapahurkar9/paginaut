// Page numbers / headers / footers / Bates numbers, and watermarks: tools in the pane on the right (toolpanes.ts).
import { signal } from "@preact/signals";
import { open } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "preact/hooks";
import {
  applyHeaders, applyWatermark, fill, headerSettings, pagesOf, removeStamps, setHeaders, setWatermark, watermarkSettings, type Font, type HeaderSettings,
} from "../stamps";
import { baseName, showToast, type Tab } from "../store";
import { toolPane } from "../toolpanes";
import { ToolFrame } from "./ToolFrame";

export const headerDialog = signal<Tab | null>(null);
export const watermarkDialog = signal<Tab | null>(null);
toolPane("headers", headerDialog, { tabOf: (v) => v });
toolPane("watermark", watermarkDialog, { tabOf: (v) => v });

const NOT_PDF = "Only PDFs can have page numbers or watermarks added. Save this file as a PDF first (Save as PDF).";

export function startHeaders(tab: Tab) {
  if (!tab.info.value?.isPdf) return showToast(NOT_PDF, 4500);
  headerDialog.value = tab;
}

export function startWatermark(tab: Tab) {
  if (!tab.info.value?.isPdf) return showToast(NOT_PDF, 4500);
  watermarkDialog.value = tab;
}

function useEscape(close: () => void) {
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        close();
      }
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, []);
}

function FontRow({ font, size, color, bold, set }: { font: Font; size: number; color: string; bold: boolean; set: (p: { font?: Font; size?: number; color?: string; bold?: boolean }) => void }) {
  return (
    <div class="ps-line">
      <select class="inp sm" value={font} onChange={(e) => set({ font: (e.target as HTMLSelectElement).value as Font })} aria-label="Font">
        <option value="sans-serif">Sans</option>
        <option value="serif">Serif</option>
        <option value="monospace">Monospace</option>
      </select>
      <input class="inp sm num" type="number" min="4" max="400" value={size} aria-label="Size (pt)" title="Size (pt)" onChange={(e) => set({ size: Math.max(4, +(e.target as HTMLInputElement).value || 9) })} />
      <span>pt</span>
      <input type="color" value={color} aria-label="Colour" onInput={(e) => set({ color: (e.target as HTMLInputElement).value })} />
      <label class="check-row">
        <input type="checkbox" checked={bold} onChange={(e) => set({ bold: (e.target as HTMLInputElement).checked })} /> Bold
      </label>
    </div>
  );
}

function PagesRow({ all, setAll, range, setRange, count }: { all: boolean; setAll: (b: boolean) => void; range: string; setRange: (s: string) => void; count: number }) {
  return (
    <div class="tool-field">
      <label>Pages</label>
      <div class="ps-line">
        <div class="seg nowrap">
          <button class={`segb ${all ? "on" : ""}`} onClick={() => setAll(true)}>
            All ({count})
          </button>
          <button class={`segb ${!all ? "on" : ""}`} onClick={() => setAll(false)}>
            Pages…
          </button>
        </div>
        {!all && <input class="inp sm" placeholder="e.g. 2- or 1-5, 8" value={range} autoFocus onInput={(e) => setRange((e.target as HTMLInputElement).value)} />}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- headers and footers

export function HeaderDialog() {
  const tab = headerDialog.value;
  return tab ? <HeaderDialogInner key={tab.key} tab={tab} /> : null;
}

const BOX_LABELS = ["Header left", "Header centre", "Header right", "Footer left", "Footer centre", "Footer right"];
const PRESETS: [string, number, string][] = [
  ["1", 4, "{page}"],
  ["Page 1 of N", 4, "Page {page} of {pages}"],
  ["1 / N", 5, "{page} / {pages}"],
  ["Bates number", 5, "{bates}"],
];

function HeaderDialogInner({ tab }: { tab: Tab }) {
  const s = headerSettings();
  const [all, setAll] = useState(true);
  const [range, setRange] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const count = tab.edits.value?.order.length ?? 0;
  const close = () => !busy && (headerDialog.value = null);
  useEscape(close);
  const setBox = (i: number, v: string) => setHeaders({ boxes: s.boxes.map((b, k) => (k === i ? v : b)) as HeaderSettings["boxes"] });
  const views = pagesOf(tab, all ? null : range);
  const usesBates = s.boxes.some((b) => b.includes("{bates}"));
  const example = views?.length ? s.boxes.map((b) => (b.trim() ? fill(b, tab, views[0], 0, views.length, s) : "")) : null;

  const apply = async () => {
    if (!views?.length) return setError(`Pages must be numbers from 1 to ${count}, like 2- or 1-5, 8.`);
    if (!s.boxes.some((b) => b.trim())) return setError("Type something in at least one box, or pick a page number style.");
    setError("");
    setBusy(true);
    try {
      await applyHeaders(tab, views);
      headerDialog.value = null;
      showToast(`Added to ${views.length === 1 ? "1 page" : `${views.length} pages`} · Ctrl+Z to undo`);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <ToolFrame id="headers" title="Page numbers, headers and footers" class="stamp-dialog">
      <div class="tool-field">
        <label>Page numbers</label>
        <div class="ps-line">
          {PRESETS.map(([name, box, tpl]) => (
            <button key={name} class="btn ghost sm" onClick={() => setHeaders({ boxes: s.boxes.map((b, k) => (k === box ? tpl : k >= 3 && b.includes("{") ? "" : b)) as HeaderSettings["boxes"] })}>
              {name}
            </button>
          ))}
        </div>
      </div>

      <div class="stamp-boxes">
        {BOX_LABELS.map((label, i) => (
          <label key={label} class="stamp-box">
            <span>{label}</span>
            <input class="inp sm" value={s.boxes[i]} onInput={(e) => setBox(i, (e.target as HTMLInputElement).value)} />
          </label>
        ))}
      </div>
      <span class="hint">
        Fill-ins: {"{page}"} number, {"{pages}"} last number, {"{label}"} the page's own label, {"{date}"}, {"{file}"} file name, {"{bates}"} Bates number.
        {example && ` First page: ${example.filter(Boolean).join(" · ")}`}
      </span>

      <div class="tool-field">
        <label>Text</label>
        <FontRow font={s.font} size={s.size} color={s.color} bold={s.bold} set={setHeaders} />
      </div>

      <div class="tool-field">
        <label>Margins (mm)</label>
        <div class="ps-line">
          {(["Top", "Bottom", "Sides"] as const).map((name, i) => (
            <label key={name} class="check-row">
              {name}
              <input
                class="inp sm num"
                type="number"
                min="0"
                step="1"
                value={s.margins[i]}
                onChange={(e) => setHeaders({ margins: s.margins.map((m, k) => (k === i ? Math.max(0, +(e.target as HTMLInputElement).value || 0) : m)) as [number, number, number] })}
              />
            </label>
          ))}
        </div>
      </div>

      <PagesRow all={all} setAll={setAll} range={range} setRange={setRange} count={count} />
      <div class="ps-line">
        <label class="check-row">
          Start numbering at
          <input class="inp sm num" type="number" min="0" value={s.start} onChange={(e) => setHeaders({ start: Math.max(0, Math.round(+(e.target as HTMLInputElement).value || 1)) })} />
        </label>
      </div>

      {usesBates && (
        <div class="tool-field">
          <label>Bates number</label>
          <div class="ps-line">
            <input class="inp sm" placeholder="Prefix" value={s.bates.prefix} onInput={(e) => setHeaders({ bates: { ...s.bates, prefix: (e.target as HTMLInputElement).value } })} />
            <label class="check-row">
              Digits
              <input class="inp sm num" type="number" min="1" max="12" value={s.bates.digits} onChange={(e) => setHeaders({ bates: { ...s.bates, digits: Math.max(1, Math.min(12, +(e.target as HTMLInputElement).value || 6)) } })} />
            </label>
            <label class="check-row">
              From
              <input class="inp sm num" type="number" min="0" value={s.bates.start} onChange={(e) => setHeaders({ bates: { ...s.bates, start: Math.max(0, Math.round(+(e.target as HTMLInputElement).value || 1)) } })} />
            </label>
            <input class="inp sm" placeholder="Suffix" value={s.bates.suffix} onInput={(e) => setHeaders({ bates: { ...s.bates, suffix: (e.target as HTMLInputElement).value } })} />
          </div>
        </div>
      )}

      <span class="hint">Headers and footers added earlier by Paginaut are replaced. They can be taken off again with Remove.</span>
      {error && <span class="error-text">{error}</span>}
      <div class="row">
        <button class="btn ghost" onClick={() => { headerDialog.value = null; void removeStamps(tab, "Header"); }}>
          Remove all
        </button>
        <span style={{ flex: 1 }} />
        <button class="btn" onClick={close}>
          Cancel
        </button>
        <button class="btn pri" disabled={busy} onClick={() => void apply()}>
          {busy ? "Adding…" : "Add"}
        </button>
      </div>
    </ToolFrame>
  );
}

// ---------------------------------------------------------------- watermark

export function WatermarkDialog() {
  const tab = watermarkDialog.value;
  return tab ? <WatermarkDialogInner key={tab.key} tab={tab} /> : null;
}

function WatermarkDialogInner({ tab }: { tab: Tab }) {
  const s = watermarkSettings();
  const [all, setAll] = useState(true);
  const [range, setRange] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const count = tab.edits.value?.order.length ?? 0;
  const close = () => !busy && (watermarkDialog.value = null);
  useEscape(close);

  const pick = async () => {
    const f = await open({ title: "Watermark picture", filters: [{ name: "Pictures", extensions: ["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff"] }] });
    if (typeof f === "string") setWatermark({ picture: f, kind: "picture" });
  };

  const apply = async () => {
    const views = pagesOf(tab, all ? null : range);
    if (!views?.length) return setError(`Pages must be numbers from 1 to ${count}, like 2- or 1-5, 8.`);
    if (s.kind === "text" && !s.text.trim()) return setError("Type the watermark's text.");
    setError("");
    setBusy(true);
    try {
      await applyWatermark(tab, views);
      watermarkDialog.value = null;
      showToast(`Watermark added to ${views.length === 1 ? "1 page" : `${views.length} pages`} · Ctrl+Z to undo`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <ToolFrame id="watermark" title="Watermark" class="stamp-dialog">
      <div class="seg nowrap">
        <button class={`segb ${s.kind === "text" ? "on" : ""}`} onClick={() => setWatermark({ kind: "text" })}>
          Text
        </button>
        <button class={`segb ${s.kind === "picture" ? "on" : ""}`} onClick={() => setWatermark({ kind: "picture" })}>
          Picture
        </button>
      </div>

      {s.kind === "text" ? (
        <>
          <input class="inp" value={s.text} placeholder="CONFIDENTIAL, DRAFT, COPY…" onInput={(e) => setWatermark({ text: (e.target as HTMLInputElement).value })} />
          <FontRow font={s.font} size={s.size} color={s.color} bold={s.bold} set={setWatermark} />
        </>
      ) : (
        <div class="ps-line">
          <span class="ins-name" title={s.picture}>
            {s.picture ? baseName(s.picture) : "No picture chosen"}
          </span>
          <button class="btn ghost sm" onClick={() => void pick()}>
            Choose…
          </button>
          <label class="check-row">
            Width
            <input class="inp sm num" type="number" min="5" max="100" value={s.scale} onChange={(e) => setWatermark({ scale: Math.max(5, Math.min(100, +(e.target as HTMLInputElement).value || 50)) })} />
            % of the page
          </label>
        </div>
      )}

      <div class="tool-field">
        <label>Angle</label>
        <div class="seg nowrap">
          {[0, 45, -45, 90].map((a) => (
            <button key={a} class={`segb ${s.angle === a ? "on" : ""}`} onClick={() => setWatermark({ angle: a })}>
              {a}°
            </button>
          ))}
        </div>
      </div>

      <div class="tool-field">
        <label>Place</label>
        <div class="seg nowrap">
          {(["top", "center", "bottom"] as const).map((p) => (
            <button key={p} class={`segb ${s.position === p ? "on" : ""}`} onClick={() => setWatermark({ position: p })}>
              {p === "center" ? "Middle" : p === "top" ? "Top" : "Bottom"}
            </button>
          ))}
        </div>
        <div class="seg nowrap">
          <button class={`segb ${!s.behind ? "on" : ""}`} onClick={() => setWatermark({ behind: false })}>
            In front of the content
          </button>
          <button class={`segb ${s.behind ? "on" : ""}`} onClick={() => setWatermark({ behind: true })}>
            Behind it
          </button>
        </div>
        <span class="hint">{s.behind ? "Hidden where the page has pictures or a background." : "Faint over everything; text under it can still be read and selected."}</span>
      </div>

      <div class="tool-field">
        <label>Opacity</label>
        <div class="ps-line">
          <input type="range" min="5" max="100" value={s.opacity} onInput={(e) => setWatermark({ opacity: +(e.target as HTMLInputElement).value })} aria-label="Opacity" />
          <span>{s.opacity}%</span>
        </div>
      </div>

      <PagesRow all={all} setAll={setAll} range={range} setRange={setRange} count={count} />
      <span class="hint">A watermark added earlier by Paginaut is replaced. It sits in its own "Watermark" layer, and Remove takes it off again.</span>
      {error && <span class="error-text">{error}</span>}
      <div class="row">
        <button class="btn ghost" onClick={() => { watermarkDialog.value = null; void removeStamps(tab, "Watermark"); }}>
          Remove all
        </button>
        <span style={{ flex: 1 }} />
        <button class="btn" onClick={close}>
          Cancel
        </button>
        <button class="btn pri" disabled={busy} onClick={() => void apply()}>
          {busy ? "Adding…" : "Add"}
        </button>
      </div>
    </ToolFrame>
  );
}
