// The Redact panel: mark (by hand or by search), then remove or cover; and Sanitize.
import { useEffect, useState } from "preact/hooks";
import { X } from "lucide-preact";
import { docSigs } from "../dsc";
import {
  applyRedactions, checkHits, clearMarks, findAll, markAll, PATTERNS, redactAreas, redactMarks, redactTab, refreshMarks, sanitize, stopRedact,
} from "../redact";
import { ask, showToast, type Tab } from "../store";

const rgbOf = (hex: string): [number, number, number] => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255) as [number, number, number];

export function RedactPanel({ tab }: { tab: Tab }) {
  const [query, setQuery] = useState("");
  const [regex, setRegex] = useState(false);
  const [found, setFound] = useState<{ hits: Map<number, number[][]>; count: number; pages: number; limited: boolean; partial: boolean } | null>(null);
  const [cover, setCover] = useState(false);
  const [color, setColor] = useState("#000000");
  const [labelOn, setLabelOn] = useState(false);
  const [label, setLabel] = useState("REDACTED");
  const [what, setWhat] = useState(1 | 2 | 4);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState("");
  const open = redactTab.value === tab;
  // Undo and redo change the marks too.
  const version = tab.historyVersion.value;
  useEffect(() => {
    if (open) void refreshMarks(tab);
  }, [open, version]);
  if (!open) return null;
  const marks = redactMarks.value;
  const signed = docSigs(tab).value?.some((s) => s.signed);

  const find = async (needle: string, re: boolean, matchCase: boolean, check?: (text: string) => boolean) => {
    if (!needle.trim()) return;
    setError("");
    setBusy("Finding…");
    try {
      const r = await findAll(tab, needle, re, matchCase);
      const hits = check ? await checkHits(tab, r.hits, check) : r.hits;
      const count = [...hits.values()].reduce((n, l) => n + l.length, 0);
      setFound({ hits, count, pages: hits.size, limited: r.limited, partial: r.partial });
    } catch (e) {
      setError(String(e));
      setFound(null);
    } finally {
      setBusy("");
    }
  };

  const markFound = async () => {
    if (!found?.count) return;
    setBusy("Marking…");
    try {
      const n = await markAll(tab, found.hits);
      showToast(`Marked ${n} ${n === 1 ? "match" : "matches"} · Ctrl+Z to undo`);
      setFound(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy("");
    }
  };

  const apply = async () => {
    const how = cover
      ? "The marked places get covered with boxes. The text stays in the file underneath: anyone can still copy or search it. Use this only to hide things from view."
      : "What's under the marks — text, picture pixels, drawings — is deleted from the pages, and checked afterwards. Save to make it permanent (Save as keeps the original).";
    const go = await ask(cover ? "Cover the marked places?" : "Remove what's under the marks?", how, [
      { label: cover ? "Cover" : "Remove", value: "go", primary: true },
      { label: "Cancel", value: "cancel" },
    ]);
    if (go !== "go") return;
    setError("");
    setBusy(cover ? "Covering…" : "Removing…");
    try {
      const r = await applyRedactions(tab, cover, rgbOf(color), labelOn ? label.trim() : "");
      if (cover) showToast(`Covered ${r.marks} ${r.marks === 1 ? "place" : "places"} on ${r.pages} ${r.pages === 1 ? "page" : "pages"}. The text is still underneath (${r.left} letters). Ctrl+Z to undo`, 9000);
      else if (r.left) showToast(`Removed under ${r.marks} marks, but ${r.left} letters can still be read there (drawn in an unusual way). Check those places before sharing.`, 12000);
      else showToast(`Removed what was under ${r.marks} ${r.marks === 1 ? "mark" : "marks"} on ${r.pages} ${r.pages === 1 ? "page" : "pages"} · checked: nothing readable left · save to keep it`, 9000);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy("");
    }
  };

  const clean = async () => {
    if (!what) return;
    setBusy("Sanitizing…");
    setError("");
    try {
      const r = await sanitize(tab, what);
      const parts = [
        what & 1 ? `${r.meta} metadata entries` : "",
        what & 2 ? `${r.scripts} scripts` : "",
        what & 4 ? `${r.files} attached files` : "",
        what & 8 ? `${r.comments} comments` : "",
        what & 16 ? `${r.hidden} hidden letters` : "",
      ].filter(Boolean);
      showToast(`Removed ${parts.join(", ")} · Ctrl+Z to undo`, 8000);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy("");
    }
  };

  const opt = (bit: number, name: string, note?: string) => (
    <label class="check-row rd-opt">
      <input type="checkbox" checked={(what & bit) !== 0} onChange={(e) => setWhat((e.target as HTMLInputElement).checked ? what | bit : what & ~bit)} />
      <span>
        {name}
        {note && <small>{note}</small>}
      </span>
    </label>
  );

  return (
    <div class="layout-panel ps-panel rd-panel">
      <div class="lp-title">
        <b>Redact</b>
        <span />
        <button class="ib sm" aria-label="Close" onClick={stopRedact}>
          <X size={16} />
        </button>
      </div>
      {signed && <span class="secure-warn">This file is digitally signed; redacting changes it, so its signatures will no longer be valid.</span>}

      <div class="lp-row">
        <label>Mark</label>
        <span class="hint">Drag over text to mark it. On scans and pictures, drag a box.</span>
        <label class="check-row">
          <input type="checkbox" checked={redactAreas.value} onChange={(e) => (redactAreas.value = (e.target as HTMLInputElement).checked)} /> Mark boxes even over text
        </label>
      </div>

      <div class="lp-row">
        <label>Find and mark</label>
        <div class="ps-line">
          <input
            class="inp sm"
            placeholder={regex ? "Pattern" : "Word or phrase"}
            value={query}
            onInput={(e) => setQuery((e.target as HTMLInputElement).value)}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") void find(query, regex, false);
            }}
          />
          <button class="btn sm" disabled={!query.trim() || !!busy} onClick={() => void find(query, regex, false)}>
            Find
          </button>
        </div>
        <label class="check-row">
          <input type="checkbox" checked={regex} onChange={(e) => setRegex((e.target as HTMLInputElement).checked)} /> Pattern (regular expression)
        </label>
        <div class="rd-chips">
          {PATTERNS.map((p) => (
            <button
              key={p.name}
              class="btn ghost sm"
              title={`Like ${p.hint}`}
              disabled={!!busy}
              onClick={() => {
                setQuery(p.re);
                setRegex(true);
                void find(p.re, true, true, p.check);
              }}
            >
              {p.name}
            </button>
          ))}
        </div>
        {found && (
          <div class="ps-line">
            <span class="hint" style={{ flex: 1 }}>
              {found.count ? `${found.count} found on ${found.pages} ${found.pages === 1 ? "page" : "pages"}${found.limited ? " (stopped at the limit)" : ""}${found.partial ? " — some pages couldn't be searched with this pattern" : ""}` : "Nothing found."}
            </span>
            {found.count > 0 && (
              <button class="btn pri sm" disabled={!!busy} onClick={() => void markFound()}>
                Mark all {found.count}
              </button>
            )}
          </div>
        )}
      </div>

      <div class="lp-row">
        <label>Apply</label>
        <span class="hint">
          {marks.marks ? `${marks.marks} ${marks.marks === 1 ? "mark" : "marks"} on ${marks.pages} ${marks.pages === 1 ? "page" : "pages"}.` : "No marks yet."}
          {marks.marks > 0 && (
            <button class="linkish" disabled={!!busy} onClick={() => void clearMarks(tab).then((n) => n && showToast(`Took off ${n} marks · Ctrl+Z to undo`))}>
              Clear marks
            </button>
          )}
        </span>
        <div class="seg nowrap">
          <button class={`segb ${!cover ? "on" : ""}`} onClick={() => setCover(false)}>
            Remove
          </button>
          <button class={`segb ${cover ? "on" : ""}`} onClick={() => setCover(true)}>
            Cover only
          </button>
        </div>
        <span class={cover ? "rd-warn" : "hint"}>
          {cover ? "Not secure: the text stays in the file under the boxes and can still be copied or searched." : "Secure: what's under the marks is deleted, then checked."}
        </span>
        <div class="ps-line">
          <span class="hint">Box</span>
          <input type="color" value={color} aria-label="Box colour" onInput={(e) => setColor((e.target as HTMLInputElement).value)} />
          <label class="check-row">
            <input type="checkbox" checked={labelOn} onChange={(e) => setLabelOn((e.target as HTMLInputElement).checked)} /> Text on boxes
          </label>
          {labelOn && <input class="inp sm" value={label} onInput={(e) => setLabel((e.target as HTMLInputElement).value)} onKeyDown={(e) => e.stopPropagation()} />}
        </div>
      </div>

      <div class="lp-row">
        <label>Sanitize the whole document</label>
        {opt(1, "Metadata", "title, author, dates, editing history")}
        {opt(2, "Scripts", "JavaScript; forms lose their calculations")}
        {opt(4, "Attached files")}
        {opt(8, "Comments", "notes, highlights, drawings")}
        {opt(16, "Hidden text", "invisible text; also takes out recognized text (OCR), so scans can't be searched")}
        <button class="btn sm" disabled={!what || !!busy} onClick={() => void clean()} style={{ alignSelf: "flex-start" }}>
          Sanitize
        </button>
      </div>

      {error && <span class="error-text">{error}</span>}
      <div class="lp-actions">
        {busy && (
          <span class="secure-busy" style={{ flex: 1 }}>
            <span class="spin" /> {busy}
          </span>
        )}
        {!busy && <span style={{ flex: 1 }} />}
        <button class="btn pri" disabled={!marks.marks || !!busy} onClick={() => void apply()}>
          {cover ? "Cover" : "Remove"} {marks.marks || ""}
        </button>
      </div>
    </div>
  );
}
