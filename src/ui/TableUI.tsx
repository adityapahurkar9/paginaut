// The table tool (in the pane on the right): a table over its picture, with column and row lines to move, add and remove.
import { useEffect, useRef, useState } from "preact/hooks";
import { api, type Table } from "../api";
import { copyTable, joinHeading, saveTable, spansOf, tableDialog } from "../extract";
import type { Tab } from "../store";
import { toolPane } from "../toolpanes";
import { ToolFrame } from "./ToolFrame";

toolPane("table", tableDialog, { tabOf: (v) => v.tab });

type Area = [number, number, number, number];

export function TableDialog() {
  const d = tableDialog.value;
  return d ? <TableInner key={`${d.tab.key}-${d.view}-${d.area.join(",")}`} tab={d.tab} view={d.view} area={d.area} initial={d.table} /> : null;
}

const WIDTH = 640;

function TableInner({ tab, view, area, initial }: { tab: Tab; view: number; area: Area; initial: Table | null }) {
  const docId = tab.info.value!.docId;
  const src = tab.edits.value?.order[view] ?? view;
  const [table, setTable] = useState<Table | null>(initial);
  const [cols, setCols] = useState<number[]>(initial?.xs ?? []);
  // Row lines you set (top and bottom included); null while they're the ones found.
  const [rows, setRows] = useState<number[] | null>(null);
  const [img, setImg] = useState<string | null>(null);
  const [busy, setBusy] = useState(!initial);
  const [join, setJoin] = useState(true);
  const edited = useRef(false);
  const pic = useRef<HTMLDivElement>(null);
  const scale = WIDTH / (area[2] - area[0]);
  const close = () => (tableDialog.value = null);

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

  // The picture of the area, sharp on this screen.
  useEffect(() => {
    let url = "";
    const dpi = Math.max(36, Math.min(400, 72 * scale * devicePixelRatio));
    api.areaBytes(docId, src, area, 0, "png", dpi).then((b) => setImg((url = URL.createObjectURL(new Blob([b], { type: "image/png" })))), () => {});
    return () => url && URL.revokeObjectURL(url);
  }, []);

  // A drawn area: read it as a table.
  useEffect(() => {
    if (initial) return;
    api
      .tableIn(docId, src, area, [])
      .then((t) => {
        setTable(t);
        setCols(t?.xs ?? [area[0], area[2]]);
      })
      .finally(() => setBusy(false));
  }, []);

  // Lines moved: read again with exactly those columns (and rows, once you've set them).
  useEffect(() => {
    if (!edited.current || cols.length < 2) return;
    // Only the answer for the lines as they are now is shown: an earlier one that arrives late is dropped.
    let latest = true;
    const id = setTimeout(() => {
      setBusy(true);
      api
        .tableIn(docId, src, area, cols, rows && rows.length >= 2 ? rows : [])
        .then((t) => latest && setTable(t))
        .finally(() => latest && setBusy(false));
    }, 180);
    return () => {
      latest = false;
      clearTimeout(id);
    };
  }, [cols, rows]);

  const sorted = (v: number[]) => [...v].sort((a, b) => a - b);
  const change = (next: number[]) => {
    edited.current = true;
    setCols(sorted(next));
  };
  const shownRows = rows ?? table?.ys ?? [];
  const changeRows = (next: number[]) => {
    edited.current = true;
    setRows(sorted(next));
  };
  const toPage = (clientX: number) => {
    const r = pic.current!.getBoundingClientRect();
    return Math.max(area[0], Math.min(area[2], area[0] + (clientX - r.left) / scale));
  };
  const toPageY = (clientY: number) => {
    const r = pic.current!.getBoundingClientRect();
    return Math.max(area[1], Math.min(area[3], area[1] + (clientY - r.top) / scale));
  };

  /** Drags line i: a column (x) or a row (y). */
  const drag = (i: number, e: PointerEvent, row = false) => {
    e.preventDefault();
    e.stopPropagation();
    // Only a drag counts: a click (or the first of a double-click) leaves the lines as found.
    let moved = false;
    const move = (ev: PointerEvent) => {
      if (!moved && Math.hypot(ev.clientX - e.clientX, ev.clientY - e.clientY) < 3) return;
      moved = true;
      edited.current = true;
      if (row) {
        const y = toPageY(ev.clientY);
        setRows((r) => (r ?? shownRows).map((v, k) => (k === i ? y : v)));
      } else {
        const x = toPage(ev.clientX);
        setCols((c) => c.map((v, k) => (k === i ? x : v)));
      }
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      if (!moved) return;
      // A found row line moved: from now on the rows are yours.
      if (row) setRows((r) => r && sorted(r));
      else setCols((c) => sorted(c));
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const out = table && join ? joinHeading(table) : table;
  const grid = out ? spansOf(out) : [];
  const h = (area[3] - area[1]) * scale;
  return (
    <ToolFrame id="table" title="Table" class="table-dialog">
      <span class="hint">
        Drag the blue lines to where the columns split, and the orange ones to where the rows do. Click the picture to add a
        column line, Shift+click to add a row line; double-click a line to remove it.
      </span>
      <div class="table-pic-wrap">
        <div
          ref={pic}
          class="table-pic"
          style={{ width: WIDTH, height: h }}
          onClick={(e) => {
            if ((e.target as HTMLElement).closest(".col-line, .row-line")) return;
            if (e.shiftKey) changeRows([...shownRows, toPageY(e.clientY)]);
            else change([...cols, toPage(e.clientX)]);
          }}
        >
          {img && <img src={img} width={WIDTH} height={h} alt="" draggable={false} />}
          {shownRows.map((y, i) => {
            const edge = i === 0 || i === shownRows.length - 1;
            return (
              <div
                key={`r${i}`}
                class={`row-line ${edge ? "edge" : ""}`}
                style={{ top: (y - area[1]) * scale }}
                onPointerDown={(e) => drag(i, e, true)}
                onDblClick={(e) => {
                  e.stopPropagation();
                  if (!edge && shownRows.length > 2) changeRows(shownRows.filter((_, k) => k !== i));
                }}
                title={edge ? "Top or bottom of the table" : "Drag to move · double-click to remove (joins the rows)"}
              />
            );
          })}
          {cols.map((x, i) => (
            <div
              key={i}
              class={`col-line ${i === 0 || i === cols.length - 1 ? "edge" : ""}`}
              style={{ left: (x - area[0]) * scale }}
              onPointerDown={(e) => drag(i, e)}
              onDblClick={(e) => {
                e.stopPropagation();
                if (cols.length > 2 && i > 0 && i < cols.length - 1) change(cols.filter((_, k) => k !== i));
              }}
              title={i === 0 || i === cols.length - 1 ? "Edge of the table" : "Drag to move · double-click to remove"}
            />
          ))}
        </div>
      </div>
      <div class="table-preview">
        {busy && !table ? (
          <span class="hint">Reading the table…</span>
        ) : !grid.length ? (
          <span class="hint">No table text found here. Scans need text recognition first (OCR).</span>
        ) : (
          <table>
            <tbody>
              {grid.slice(0, 60).map((row, r) => (
                <tr key={r}>
                  {row.map((cell, c) => (
                    <td key={c} title={cell.s} colSpan={cell.cs > 1 ? cell.cs : undefined} class={cell.cs > 1 ? "span" : undefined}>
                      {cell.s}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
      <span class="hint">
        {grid.length ? `${grid.length} rows × ${(table?.xs.length ?? 1) - 1} columns${grid.length > 60 ? " (first 60 shown)" : ""}` : ""}
      </span>
      <label class="check-row">
        <input type="checkbox" checked={join} onChange={(e) => setJoin((e.target as HTMLInputElement).checked)} /> Join heading lines into one row
      </label>
      <div class="row">
        <button class="btn ghost" disabled={!initial && !edited.current} onClick={() => { edited.current = false; setRows(null); setTable(initial); setCols(initial?.xs ?? []); if (!initial) { setBusy(true); api.tableIn(docId, src, area, []).then((t) => { setTable(t); setCols(t?.xs ?? [area[0], area[2]]); }).finally(() => setBusy(false)); } }}>
          Reset
        </button>
        <span style={{ flex: 1 }} />
        <button class="btn" disabled={!grid.length} onClick={() => out && copyTable(out)}>
          Copy table
        </button>
        <button class="btn" disabled={!grid.length} onClick={() => out && void saveTable(tab, view, out, "csv")}>
          Save as CSV…
        </button>
        <button class="btn pri" disabled={!grid.length} onClick={() => out && void saveTable(tab, view, out, "xlsx")}>
          Save as Excel…
        </button>
      </div>
    </ToolFrame>
  );
}
