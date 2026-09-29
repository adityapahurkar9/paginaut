// The table dialog: a table over its picture, with column lines to move, add and remove.
import { useEffect, useRef, useState } from "preact/hooks";
import { X } from "lucide-preact";
import { api, type Table } from "../api";
import { copyTable, gridOf, joinHeading, saveTable, tableDialog } from "../extract";
import type { Tab } from "../store";

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

  // Column lines moved: read again with exactly those columns.
  useEffect(() => {
    if (!edited.current || cols.length < 2) return;
    const id = setTimeout(() => {
      setBusy(true);
      api.tableIn(docId, src, area, cols).then(setTable).finally(() => setBusy(false));
    }, 180);
    return () => clearTimeout(id);
  }, [cols]);

  const change = (next: number[]) => {
    edited.current = true;
    setCols([...next].sort((a, b) => a - b));
  };
  const toPage = (clientX: number) => {
    const r = pic.current!.getBoundingClientRect();
    return Math.max(area[0], Math.min(area[2], area[0] + (clientX - r.left) / scale));
  };

  const drag = (i: number, e: PointerEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const move = (ev: PointerEvent) => {
      const x = toPage(ev.clientX);
      edited.current = true;
      setCols((c) => c.map((v, k) => (k === i ? x : v)));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      setCols((c) => {
        edited.current = true;
        return [...c].sort((a, b) => a - b);
      });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  const out = table && join ? joinHeading(table) : table;
  const grid = out ? gridOf(out) : [];
  const h = (area[3] - area[1]) * scale;
  return (
    <div class="overlay" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div class="dialog tool-dialog table-dialog" role="dialog" aria-label="Table">
        <button class="ib sm close" aria-label="Close" onClick={close}>
          <X size={16} />
        </button>
        <h2>Table</h2>
        <span class="hint">
          Drag the blue lines to where the columns split. Click the picture to add a line; double-click a line to remove it.
        </span>
        <div class="table-pic-wrap">
          <div ref={pic} class="table-pic" style={{ width: WIDTH, height: h }} onClick={(e) => (e.target as HTMLElement).closest(".col-line") || change([...cols, toPage(e.clientX)])}>
            {img && <img src={img} width={WIDTH} height={h} alt="" draggable={false} />}
            {table?.ys.map((y, i) => <div key={`r${i}`} class="row-line" style={{ top: (y - area[1]) * scale }} />)}
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
                    {row.map((s, c) => (
                      <td key={c} title={s}>
                        {s}
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
          <button class="btn ghost" disabled={!initial && !edited.current} onClick={() => { edited.current = false; setTable(initial); setCols(initial?.xs ?? []); if (!initial) { setBusy(true); api.tableIn(docId, src, area, []).then((t) => { setTable(t); setCols(t?.xs ?? [area[0], area[2]]); }).finally(() => setBusy(false)); } }}>
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
      </div>
    </div>
  );
}
