// The Page size panel: crop box (ratio, size, whole page / fit to content,
// hide or remove what's outside), an optional new page size (fit, fill or
// actual size, where the content sits, what fills the new space) and which
// pages it applies to.
import { useState } from "preact/hooks";
import { Pipette, RectangleHorizontal, RectangleVertical, ScanLine, Maximize2 } from "lucide-preact";
import {
  applyPageSize, cropRatio, endPageSize, fitRatio, fitToContent, pageGeom, pageSizeEdit, PRESETS, presetOf, psSettings, scopeViews, setMargins, setPs,
  targetSize, UNIT, type Scope, type Unit,
} from "../pagesize";
import { toolPane } from "../toolpanes";
import { ToolFrame } from "./ToolFrame";

toolPane("pagesize", pageSizeEdit, { tabOf: (v) => v.tab, close: endPageSize });

const GROUPS = ["Paper", "Photo", "Screen"];

function PresetOptions({ sizesOnly }: { sizesOnly?: boolean }) {
  return (
    <>
      {GROUPS.map((g) => {
        const list = PRESETS.filter((p) => p.group === g && !(sizesOnly && p.ratioOnly));
        return list.length ? (
          <optgroup key={g} label={g}>
            {list.map((p) => (
              <option key={p.key} value={p.key}>
                {p.label}
              </option>
            ))}
          </optgroup>
        ) : null;
      })}
    </>
  );
}

/** A number in the chosen unit; edits call back in points. */
function Num({ pt, unit, onPt, label }: { pt: number; unit: Unit; onPt: (v: number) => void; label: string }) {
  const shown = (pt / UNIT[unit]).toFixed(unit === "in" ? 2 : unit === "mm" ? 1 : 0);
  return (
    <input
      class="inp sm num"
      type="number"
      aria-label={label}
      value={shown}
      step={unit === "in" ? 0.1 : 1}
      min={0}
      onChange={(e) => {
        const v = parseFloat((e.target as HTMLInputElement).value);
        if (v > 0) onPt(v * UNIT[unit]);
      }}
    />
  );
}

export function PageSizePanel() {
  const st = pageSizeEdit.value;
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  if (!st) return null;
  const tab = st.tab;
  const s = psSettings();
  const view = tab.page.value;
  const g = pageGeom(tab, view);
  const [l, t, r, b] = st.margins;
  const cw = g.W - l - r, ch = g.H - t - b;
  const sel = tab.pageSel.value.size;
  const target = targetSize(st.margins, g.W, g.H);
  const scopeCount = (() => {
    const v = scopeViews(tab, st.scope, st.range);
    return typeof v === "string" ? 0 : v.length;
  })();

  const setRatio = (ratio: string, landscape = s.landscape) => {
    setPs({ ratio, landscape });
    if (ratio !== "free") setMargins(fitRatio(g.W, g.H, cropRatio(tab)));
  };
  /** A new crop size (shown, points), kept centred where it is. */
  const setCropSize = (w: number, h: number) => {
    const cx = l + cw / 2, cy = t + ch / 2;
    w = Math.min(w, g.W);
    h = Math.min(h, g.H);
    const x0 = Math.max(0, Math.min(g.W - w, cx - w / 2)), y0 = Math.max(0, Math.min(g.H - h, cy - h / 2));
    setPs({ ratio: "free" });
    setMargins([x0, y0, g.W - x0 - w, g.H - y0 - h]);
  };
  const pickColor = async () => {
    const Dropper = (window as unknown as { EyeDropper?: new () => { open(): Promise<{ sRGBHex: string }> } }).EyeDropper;
    if (!Dropper) return;
    try {
      const { sRGBHex } = await new Dropper().open();
      setPs({ color: sRGBHex });
    } catch {
      /* cancelled */
    }
  };
  const apply = async () => {
    setBusy(true);
    const err = await applyPageSize();
    setBusy(false);
    setError(err ?? "");
  };
  const scale = target ? (s.fit === 2 ? 1 : s.fit === 0 ? Math.min(target[0] / cw, target[1] / ch) : Math.max(target[0] / cw, target[1] / ch)) : 1;
  const u = s.unit;
  const fmt = (v: number) => (v / UNIT[u]).toFixed(u === "in" ? 2 : 0);

  return (
    <ToolFrame id="pagesize" title="Crop and resize pages" class="ps-panel">
      <span class="hint">Drag the box's edges on page {view + 1}, or set the sizes here.</span>

      <div class="lp-row">
        <label>Crop</label>
        <div class="ps-line">
          <select class="inp sm" value={s.ratio} onChange={(e) => setRatio((e.target as HTMLSelectElement).value)} aria-label="Shape of the crop box">
            <option value="free">Any shape</option>
            <option value="original">Same shape as the page</option>
            <PresetOptions />
          </select>
          <button
            class={`ib sm ${!s.landscape ? "on" : ""}`}
            title="Portrait"
            aria-label="Portrait"
            disabled={s.ratio === "free" || s.ratio === "original"}
            onClick={() => setRatio(s.ratio, false)}
          >
            <RectangleVertical size={15} />
          </button>
          <button
            class={`ib sm ${s.landscape ? "on" : ""}`}
            title="Landscape"
            aria-label="Landscape"
            disabled={s.ratio === "free" || s.ratio === "original"}
            onClick={() => setRatio(s.ratio, true)}
          >
            <RectangleHorizontal size={15} />
          </button>
        </div>
        <div class="ps-line">
          <Num pt={cw} unit={u} label="Crop width" onPt={(w) => setCropSize(w, ch)} />
          <span>×</span>
          <Num pt={ch} unit={u} label="Crop height" onPt={(h) => setCropSize(cw, h)} />
          <select class="inp sm" value={u} onChange={(e) => setPs({ unit: (e.target as HTMLSelectElement).value as Unit })} aria-label="Unit">
            <option value="mm">mm</option>
            <option value="in">in</option>
            <option value="pt">pt</option>
          </select>
        </div>
        <div class="ps-line">
          <button class="btn ghost sm" onClick={() => { setPs({ ratio: "free" }); setMargins([0, 0, 0, 0]); }} title="The crop box covers the whole page">
            <Maximize2 size={13} /> Whole page
          </button>
          <button class="btn ghost sm" onClick={() => void fitToContent()} title="Trim the empty margins around what's on the page">
            <ScanLine size={13} /> Fit to content
          </button>
        </div>
        <div class="seg nowrap">
          <button class={`segb ${!s.remove ? "on" : ""}`} onClick={() => setPs({ remove: false })}>
            Hide edges
          </button>
          <button class={`segb ${s.remove ? "on" : ""}`} onClick={() => setPs({ remove: true })}>
            Remove outside
          </button>
        </div>
        <span class="hint">
          {s.remove
            ? "What's outside the box is deleted from the file (text, pictures, drawings), not just hidden."
            : "What's outside is hidden, and can be shown again later (the page keeps it)."}
        </span>
      </div>

      <div class="lp-row">
        <label class="check-row">
          <input type="checkbox" checked={s.resize} onChange={(e) => setPs({ resize: (e.target as HTMLInputElement).checked })} />
          Change the page size
        </label>
        {s.resize && (
          <>
            <div class="ps-line">
              <select class="inp sm" value={s.target} onChange={(e) => setPs({ target: (e.target as HTMLSelectElement).value })} aria-label="New page size">
                <PresetOptions sizesOnly />
                <option value="custom">Custom size</option>
              </select>
              {s.target === "custom" && (
                <>
                  <Num pt={s.custom[0]} unit={u} label="Page width" onPt={(w) => setPs({ custom: [w, s.custom[1]] })} />
                  <span>×</span>
                  <Num pt={s.custom[1]} unit={u} label="Page height" onPt={(h) => setPs({ custom: [s.custom[0], h] })} />
                </>
              )}
            </div>
            <div class="seg nowrap">
              {(["Fit", "Fill", "Actual size"] as const).map((name, i) => (
                <button key={name} class={`segb ${s.fit === i ? "on" : ""}`} onClick={() => setPs({ fit: i as 0 | 1 | 2 })}>
                  {name}
                </button>
              ))}
            </div>
            <div class="ps-line">
              <div class="ps-anchor" role="group" aria-label="Where the content sits">
                {[0, 0.5, 1].map((y) =>
                  [0, 0.5, 1].map((x) => (
                    <button
                      key={`${x},${y}`}
                      class={s.anchor[0] === x && s.anchor[1] === y ? "on" : ""}
                      aria-label={`${y === 0 ? "Top" : y === 1 ? "Bottom" : "Middle"} ${x === 0 ? "left" : x === 1 ? "right" : "centre"}`}
                      onClick={() => setPs({ anchor: [x, y] })}
                    />
                  )),
                )}
              </div>
              <span class="hint">
                {target
                  ? `${presetOf(s.target)?.label ?? "Custom"} ${fmt(target[0])} × ${fmt(target[1])} ${u}. ${
                      s.fit === 1 ? "Fills the page (edges beyond it are cut)" : s.fit === 2 ? "Content at its own size" : "All of it shows"
                    }${s.fit === 2 ? "" : `, at ${Math.round(scale * 100)}%`}.`
                  : ""}
              </span>
            </div>
            {s.fit !== 1 && (
              <>
                <div class="seg nowrap">
                  {(["Nothing", "Colour", "Extend edges", "Paper"] as const).map((name, i) => (
                    <button key={name} class={`segb ${s.fill === i ? "on" : ""}`} onClick={() => setPs({ fill: i as 0 | 1 | 2 | 3 })}>
                      {name}
                    </button>
                  ))}
                </div>
                {s.fill === 1 && (
                  <div class="ps-line">
                    <input type="color" value={s.color} onInput={(e) => setPs({ color: (e.target as HTMLInputElement).value })} aria-label="Colour of the new space" />
                    <button class="btn ghost sm" onClick={() => void pickColor()} title="Pick a colour from the screen">
                      <Pipette size={13} /> Pick from page
                    </button>
                  </div>
                )}
                <span class="hint">
                  {s.fill === 0
                    ? "New space is left empty (it shows as white; stays see-through if the page is laid over another)."
                    : s.fill === 1
                      ? "New space around the content is filled with this colour."
                      : s.fill === 2
                        ? "The outermost pixels of the content are stretched into the new space."
                        : "New space looks like the page's own paper: blank patches from near its edges, blended in (best for scans)."}
                </span>
              </>
            )}
          </>
        )}
      </div>

      <div class="lp-row">
        <label>Apply to</label>
        <div class="ps-line">
          <select
            class="inp sm"
            value={st.scope}
            onChange={(e) => (pageSizeEdit.value = { ...st, scope: (e.target as HTMLSelectElement).value as Scope })}
            aria-label="Pages to change"
          >
            <option value="page">This page ({view + 1})</option>
            {sel > 1 && <option value="selected">Selected pages ({sel})</option>}
            <option value="all">All pages</option>
            <option value="odd">Odd pages</option>
            <option value="even">Even pages</option>
            <option value="range">Pages…</option>
          </select>
          {st.scope === "range" && (
            <input class="inp sm" placeholder="e.g. 1-5, 8" value={st.range} onInput={(e) => (pageSizeEdit.value = { ...st, range: (e.target as HTMLInputElement).value })} />
          )}
        </div>
        {scopeCount > 1 && <span class="hint">The same margins are cut from each page.</span>}
      </div>

      {error && <span class="error-text">{error}</span>}
      <div class="row">
        <button class="btn" onClick={endPageSize}>
          Cancel
        </button>
        <button class="btn pri" disabled={busy} onClick={() => void apply()}>
          {busy ? "Working…" : scopeCount > 1 ? `Apply to ${scopeCount} pages` : "Apply"}
        </button>
      </div>
    </ToolFrame>
  );
}
