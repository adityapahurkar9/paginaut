// Document properties and page labels dialogs.
import { signal } from "@preact/signals";
import { Fragment } from "preact";
import { useEffect, useState } from "preact/hooks";
import { Plus, Trash2, X } from "lucide-preact";
import { api, type LabelRange, type Properties } from "../api";
import { metaChanged } from "../edits";
import { baseName, showToast, type Tab } from "../store";

export const propertiesDialog = signal<Tab | null>(null);
export const labelsDialog = signal<Tab | null>(null);

export function startProperties(tab: Tab) {
  if (tab.info.value) propertiesDialog.value = tab;
}

export function startLabels(tab: Tab) {
  if (!tab.info.value?.isPdf) return showToast("Only PDFs have page labels. Save this file as a PDF first (Save as).", 4500);
  labelsDialog.value = tab;
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

/** "D:20240131093000+05'30'" → a local date and time. */
function pdfDate(s?: string) {
  const m = s?.match(/^(?:D:)?(\d{4})(\d{2})?(\d{2})?(\d{2})?(\d{2})?(\d{2})?(Z|[+-]\d{2}'?\d{2}'?)?/);
  if (!m) return s ?? "";
  const [, y, mo = "01", d = "01", h = "00", mi = "00", se = "00", z] = m;
  const zone = !z ? "" : z === "Z" ? "Z" : `${z.slice(0, 3)}:${z.replace(/'/g, "").slice(3, 5) || "00"}`;
  const date = new Date(`${y}-${mo}-${d}T${h}:${mi}:${se}${zone}`);
  return isNaN(+date) ? s! : date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

const size = (b: number) => (b >= 1 << 20 ? `${(b / (1 << 20)).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1024))} KB`);

// ---------------------------------------------------------------- properties

export function PropertiesDialog() {
  const tab = propertiesDialog.value;
  return tab ? <PropertiesInner key={tab.key} tab={tab} /> : null;
}

function PropertiesInner({ tab }: { tab: Tab }) {
  const info = tab.info.value!;
  const [props, setProps] = useState<Properties | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const close = () => !busy && (propertiesDialog.value = null);
  useEscape(close);
  useEffect(() => {
    api.properties(info.docId).then(setProps, (e) => setError(String(e)));
  }, []);

  const save = async () => {
    if (!props) return;
    setBusy(true);
    try {
      await api.setProperties(info.docId, props);
      tab.info.value = { ...tab.info.value!, title: props.title.trim(), author: props.author.trim() };
      metaChanged(tab);
      propertiesDialog.value = null;
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const field = (key: "title" | "author" | "subject" | "keywords", label: string) => (
    <label class="meta-field">
      <span>{label}</span>
      <input
        class="inp sm"
        value={props?.[key] ?? ""}
        disabled={!props || !info.isPdf}
        onInput={(e) => {
          const v = (e.target as HTMLInputElement).value;
          setProps((p) => p && { ...p, [key]: v });
        }}
        onKeyDown={(e) => e.key === "Enter" && info.isPdf && void save()}
      />
    </label>
  );
  const pt = (v: number) => `${Math.round((v / 72) * 25.4)} mm`;
  const facts: [string, string][] = [
    ["File", info.path],
    ["Kind", `${info.format}${info.isPdf ? "" : " (read only)"}`],
    ["Size", `${size(info.fileSize)} · ${info.pageCount} ${info.pageCount === 1 ? "page" : "pages"}`],
    ["First page", `${pt(info.firstSize[0])} × ${pt(info.firstSize[1])}`],
    ["Made with", [props?.creator, props?.producer].filter(Boolean).join(" · ")],
    ["Created", pdfDate(props?.created)],
    ["Changed", pdfDate(props?.modified)],
  ];

  return (
    <div class="overlay" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div class="dialog tool-dialog meta-dialog" role="dialog" aria-label="Document properties">
        <button class="ib sm close" aria-label="Close" onClick={close}>
          <X size={16} />
        </button>
        <h2>Document properties</h2>
        <div class="meta-grid">
          {field("title", "Title")}
          {field("author", "Author")}
          {field("subject", "Subject")}
          {field("keywords", "Keywords")}
        </div>
        <dl class="meta-facts">
          {facts.filter(([, v]) => v).map(([k, v]) => (
            <Fragment key={k}>
              <dt>{k}</dt>
              <dd title={v}>{v}</dd>
            </Fragment>
          ))}
        </dl>
        {!info.isPdf && <span class="hint">Only PDFs can be changed here. Save this file as a PDF first (Save as).</span>}
        {error && <span class="error-text">{error}</span>}
        <div class="row">
          <span style={{ flex: 1 }} />
          <button class="btn" onClick={close}>
            {info.isPdf ? "Cancel" : "Close"}
          </button>
          {info.isPdf && (
            <button class="btn pri" disabled={busy || !props} onClick={() => void save()}>
              Save
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- page labels

const STYLES: [LabelRange["st"], string][] = [
  ["D", "1, 2, 3"],
  ["r", "i, ii, iii"],
  ["R", "I, II, III"],
  ["a", "a, b, c"],
  ["A", "A, B, C"],
  ["", "No number"],
];

function roman(n: number) {
  const r: [number, string][] = [[1000, "m"], [900, "cm"], [500, "d"], [400, "cd"], [100, "c"], [90, "xc"], [50, "l"], [40, "xl"], [10, "x"], [9, "ix"], [5, "v"], [4, "iv"], [1, "i"]];
  let s = "";
  for (const [v, t] of r) while (n >= v) (s += t), (n -= v);
  return s;
}

/** The label of page `i` under these ranges (as the PDF spec numbers them). */
export function labelOf(ranges: LabelRange[], i: number) {
  let r: LabelRange | undefined;
  for (const x of ranges) if (x.s <= i && (!r || x.s >= r.s)) r = x;
  if (!r) return String(i + 1);
  const n = r.n + i - r.s;
  const letters = (k: number) => String.fromCharCode(97 + ((k - 1) % 26)).repeat(Math.floor((k - 1) / 26) + 1);
  const num = { D: String(n), r: roman(n), R: roman(n).toUpperCase(), a: letters(n), A: letters(n).toUpperCase(), "": "" }[r.st];
  return r.p + num;
}

export function LabelsDialog() {
  const tab = labelsDialog.value;
  return tab ? <LabelsInner key={tab.key} tab={tab} /> : null;
}

function LabelsInner({ tab }: { tab: Tab }) {
  const info = tab.info.value!;
  const count = info.pageCount;
  const order = tab.edits.value?.order;
  const reordered = !!order && (order.length !== count || order.some((p, i) => p !== i));
  const [ranges, setRanges] = useState<LabelRange[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const close = () => !busy && (labelsDialog.value = null);
  useEscape(close);
  useEffect(() => {
    api.pageLabelRanges(info.docId).then((r) => setRanges(r.length ? r : [{ s: 0, st: "D", p: "", n: 1 }]), (e) => setError(String(e)));
  }, []);

  const set = (i: number, p: Partial<LabelRange>) => setRanges((list) => list && list.map((r, k) => (k === i ? { ...r, ...p } : r)));
  const add = () =>
    setRanges((list) => {
      if (!list) return list;
      const here = Math.min(count - 1, Math.max(1, tab.page.value));
      const s = list.some((r) => r.s === here) ? Math.min(count - 1, Math.max(...list.map((r) => r.s)) + 1) : here;
      return [...list, { s, st: "D" as const, p: "", n: 1 }].sort((a, b) => a.s - b.s);
    });
  const sorted = ranges ? [...ranges].sort((a, b) => a.s - b.s) : [];
  const clash = sorted.some((r, i) => i > 0 && r.s === sorted[i - 1].s);
  const preview = ranges ? Array.from({ length: Math.min(count, 12) }, (_, i) => labelOf(sorted, i)) : [];

  const save = async (list: LabelRange[]) => {
    if (clash) return setError("Two ranges start on the same page.");
    setBusy(true);
    try {
      await api.setPageLabels(info.docId, list);
      tab.labels.value = await api.pageLabels(info.docId);
      metaChanged(tab);
      labelsDialog.value = null;
      showToast(list.length ? "Page labels changed · Ctrl+Z to undo" : "Page labels removed · Ctrl+Z to undo");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="overlay" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div class="dialog tool-dialog labels-dialog" role="dialog" aria-label="Page labels">
        <button class="ib sm close" aria-label="Close" onClick={close}>
          <X size={16} />
        </button>
        <h2>Page labels</h2>
        <span class="hint">
          Labels are the page numbers a reader shows, like i, ii for the front matter and 1, 2 from the first chapter. They don't print on the page (use Page numbers for that).
        </span>
        {reordered ? (
          <span class="hint">Pages have been moved, added or deleted since the last save. Save first, then set the labels.</span>
        ) : (
          <>
            <div class="labels-rows">
              <div class="labels-head">
                <span>From page</span>
                <span>Style</span>
                <span>Prefix</span>
                <span>Start at</span>
                <span />
              </div>
              {ranges?.map((r, i) => (
                <div class="labels-row" key={i}>
                  <input
                    class="inp sm num"
                    type="number"
                    min="1"
                    max={count}
                    value={r.s + 1}
                    disabled={r.s === 0 && i === 0}
                    aria-label="From page"
                    onChange={(e) => set(i, { s: Math.max(0, Math.min(count - 1, Math.round(+(e.target as HTMLInputElement).value || 1) - 1)) })}
                  />
                  <select class="inp sm" value={r.st} aria-label="Style" onChange={(e) => set(i, { st: (e.target as HTMLSelectElement).value as LabelRange["st"] })}>
                    {STYLES.map(([v, name]) => (
                      <option key={v} value={v}>
                        {name}
                      </option>
                    ))}
                  </select>
                  <input class="inp sm" value={r.p} placeholder="e.g. A-" aria-label="Prefix" onInput={(e) => set(i, { p: (e.target as HTMLInputElement).value })} />
                  <input
                    class="inp sm num"
                    type="number"
                    min="1"
                    value={r.n}
                    disabled={!r.st}
                    aria-label="Start at"
                    onChange={(e) => set(i, { n: Math.max(1, Math.round(+(e.target as HTMLInputElement).value || 1)) })}
                  />
                  <button class="ib sm" aria-label="Remove this range" disabled={ranges.length < 2} onClick={() => setRanges(ranges.filter((_, k) => k !== i))}>
                    <Trash2 size={14} />
                  </button>
                </div>
              ))}
              <button class="btn ghost sm" disabled={!ranges || count < 2} onClick={add} style={{ alignSelf: "flex-start" }}>
                <Plus size={14} /> Add a range
              </button>
            </div>
            {!!preview.length && (
              <span class="hint">
                {baseName(info.path)}: {preview.join(", ")}
                {count > preview.length ? ", …" : ""}
                {sorted.length > 1 && sorted[sorted.length - 1].s >= preview.length && ` · page ${sorted[sorted.length - 1].s + 1} is ${labelOf(sorted, sorted[sorted.length - 1].s)}`}
              </span>
            )}
          </>
        )}
        {clash && <span class="error-text">Two ranges start on the same page.</span>}
        {error && <span class="error-text">{error}</span>}
        <div class="row">
          {!reordered && tab.labels.value && (
            <button class="btn ghost" disabled={busy} onClick={() => void save([])}>
              Remove labels
            </button>
          )}
          <span style={{ flex: 1 }} />
          <button class="btn" onClick={close}>
            Cancel
          </button>
          {!reordered && (
            <button class="btn pri" disabled={busy || !ranges || clash} onClick={() => ranges && void save(sorted)}>
              Save
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
