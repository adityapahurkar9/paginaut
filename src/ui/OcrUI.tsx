// Text recognition in the UI: the bar over scanned documents (offer, progress,
// result) and the Recognize text dialog (pages, languages, accuracy).
import { useEffect, useMemo, useState } from "preact/hooks";
import { ScanText, X } from "lucide-preact";
import { api } from "../api";
import { docSigs } from "../dsc";
import * as P from "../pages";
import {
  dismissOcrState, dismissOffer, downloadModel, downloads, hasModel, looksScanned, ocrDialog, ocrLangs, ocrState,
  offerDismissed, parsePages, recognize, rememberChoice, startOcr, stopDownload, stopOcr, type OcrLang, type OcrQuality,
} from "../ocr";
import { openSettings, settings, type Tab } from "../store";

const mb = (bytes: number) => `${(bytes / 1e6).toFixed(bytes < 1e7 ? 1 : 0)} MB`;
const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;

// ---------------------------------------------------------------- bar

export function OcrBar({ tab }: { tab: Tab }) {
  const st = ocrState(tab).value;
  const scanned = looksScanned(tab).value;
  if (st?.phase === "running") {
    const frac = st.total ? st.done / st.total : 0;
    return (
      <div class="formbar ocrbar">
        <ScanText size={16} />
        <span>
          Recognizing text… {st.done.toLocaleString()} of {plural(st.total, "page")}
        </span>
        <div class="rf-bar ocr-progress">
          <div style={{ width: `${Math.round(frac * 100)}%` }} />
        </div>
        <button class="btn ghost" disabled={st.stopping} onClick={() => void stopOcr(tab)} title="Pages already recognized keep their text">
          {st.stopping ? "Stopping…" : "Stop"}
        </button>
      </div>
    );
  }
  if (st?.phase === "done") {
    const e = st.end;
    const added = e.pages > 0;
    const did = e.replaced ? `Recognized ${plural(e.pages, "page")} again` : `Added text to ${plural(e.pages, "page")}`;
    // Sideways or upside-down pages: read the right way, and offered turned upright (until they are).
    const rot = tab.edits.value?.rot ?? [];
    const sideways = e.turned.length ? ` ${plural(e.turned.length, "page was", "pages were")} sideways or upside down.` : "";
    const turnable = e.turned.some(([src, deg]) => (rot[src] ?? 0) !== deg);
    const text = e.error
      ? `Text recognition stopped with an error: ${e.error}${added ? ` ${did}.` : ""}`
      : added
        ? `${st.stopped ? "Stopped. " : ""}${did}: you can now search, select and copy it.${sideways}${tab.dirty.value ? " Save to keep it." : ""}`
        : st.stopped
          ? "Stopped before any text was found."
          : "No new text found: the pages already have text, or have nothing to read.";
    return (
      <div class={`formbar ocrbar ${e.error ? "warn" : ""}`}>
        <ScanText size={16} />
        <span title={text}>{text}</span>
        {turnable && (
          <button class="btn ghost" onClick={() => P.turnSources(tab, e.turned)} title="Turn those pages so they read the right way up (Ctrl+Z to undo)">
            Turn upright
          </button>
        )}
        {added && !e.error && (
          <button class="btn ghost" onClick={() => void recognize(tab)} title="Wrong language, or words misread? Read the pages again with other languages or Best accuracy">
            Recognize again…
          </button>
        )}
        {added && tab.dirty.value && (
          <button class="btn ghost" onClick={() => void P.save(tab)}>
            Save
          </button>
        )}
        <button class="ib sm" aria-label="Dismiss" onClick={() => dismissOcrState(tab)}>
          <X size={14} />
        </button>
      </div>
    );
  }
  if (!scanned || offerDismissed(tab)) return null;
  return (
    <div class="formbar ocrbar">
      <ScanText size={16} />
      <span>The text on these pages is part of the picture, so it can't be searched or copied yet.</span>
      <button class="btn ghost" onClick={() => void recognize(tab)}>
        Recognize text
      </button>
      <button class="ib sm" aria-label="Don't offer again for this file" title="Don't offer again for this file (Tools › OCR still works)" onClick={() => dismissOffer(tab)}>
        <X size={14} />
      </button>
    </div>
  );
}

// ---------------------------------------------------------------- dialog

export function OcrDialog() {
  const tab = ocrDialog.value;
  return tab ? <OcrDialogInner key={tab.key} tab={tab} /> : null;
}

function OcrDialogInner({ tab }: { tab: Tab }) {
  const saved = settings.value.ocr;
  const [langs, setLangs] = useState<string[]>(saved.langs.length ? saved.langs : ["eng"]);
  const [quality, setQuality] = useState<OcrQuality>(saved.quality);
  const [scope, setScope] = useState<"all" | "page" | "range">("all");
  const [range, setRange] = useState("");
  const [error, setError] = useState("");
  const [preparing, setPreparing] = useState(false);
  // Pages Paginaut recognised before: read again (replacing their text) unless unticked.
  const [layers, setLayers] = useState(0);
  const [replace, setReplace] = useState(true);
  useEffect(() => {
    const doc = tab.info.value?.docId;
    if (doc !== undefined) api.ocrLayers(doc).then(setLayers, () => {});
  }, []);
  const all = ocrLangs.value;
  const byCode = useMemo(() => new Map(all.map((l) => [l.code, l])), [all]);
  const order = tab.edits.value?.order ?? [];
  const count = order.length;
  const current = Math.min(tab.page.value, Math.max(0, count - 1));
  const signed = !!docSigs(tab).value?.some((s) => s.signed);
  const close = () => (ocrDialog.value = null);

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

  // Saved languages that no longer exist in the list are dropped once it has loaded.
  const chosen = all.length ? langs.filter((c) => byCode.has(c)) : langs;
  const missing = chosen.map((c) => byCode.get(c)).filter((l): l is OcrLang => !!l && !hasModel(l, quality));
  const missingSize = missing.reduce((n, l) => n + (quality === "best" ? l.bestSize : l.fastSize), 0);
  const active = Object.entries(downloads.value);

  const start = async () => {
    setError("");
    if (!chosen.length) return setError("Choose the language the document is written in.");
    const views = scope === "all" ? order.map((_, i) => i) : scope === "page" ? [current] : parsePages(range, count);
    if (!views) return setError(`Pages must be numbers from 1 to ${count}, like 1-5, 8.`);
    const pages = views.map((v) => order[v]).filter((p) => p !== undefined);
    if (missing.length) {
      setPreparing(true);
      for (const l of missing) {
        if (!(await downloadModel(l.code, quality))) {
          setPreparing(false);
          return;
        }
      }
      setPreparing(false);
    }
    rememberChoice(chosen, quality);
    close();
    startOcr(tab, pages, chosen, quality, layers > 0 && replace);
  };

  const add = (code: string) => code && !chosen.includes(code) && setLangs([...chosen, code]);
  const ready = all.filter((l) => hasModel(l, quality) && !chosen.includes(l.code));
  const others = all.filter((l) => !hasModel(l, quality) && !chosen.includes(l.code));

  return (
    <div class="overlay" onPointerDown={(e) => e.target === e.currentTarget && !preparing && close()}>
      <div class="dialog ocr-dialog" role="dialog" aria-label="Recognize text">
        <button class="ib sm close" aria-label="Close" onClick={close}>
          <X size={16} />
        </button>
        <h2>Recognize text</h2>
        <span class="muted">
          Finds the text in scanned pages, so you can search, select and copy it and read them in Read mode. Text already on the pages stays as it is
          {layers > 0 && replace ? ", except text recognized before, which is read again" : ""}.
        </span>
        {layers > 0 && (
          <label class="chk">
            <input type="checkbox" checked={replace} onChange={(e) => setReplace(e.currentTarget.checked)} />
            <span>
              Read the text recognized before again, and replace it ({plural(layers, "page")}). Useful when words were misread: try other languages or Best accuracy.
            </span>
          </label>
        )}

        <div class="ocr-field">
          <label>Pages</label>
          <div class="seg">
            <button class={`segb ${scope === "all" ? "on" : ""}`} onClick={() => setScope("all")}>
              All ({count})
            </button>
            <button class={`segb ${scope === "page" ? "on" : ""}`} onClick={() => setScope("page")}>
              This page ({current + 1})
            </button>
            <button class={`segb ${scope === "range" ? "on" : ""}`} onClick={() => setScope("range")}>
              Pages…
            </button>
          </div>
          {scope === "range" && (
            <input
              class="inp sm"
              placeholder={`e.g. 1-5, 8 (of ${count})`}
              value={range}
              autoFocus
              onInput={(e) => setRange((e.target as HTMLInputElement).value)}
              onKeyDown={(e) => e.key === "Enter" && void start()}
            />
          )}
        </div>

        <div class="ocr-field">
          <label>Languages in the document</label>
          <div class="chips">
            {chosen.map((c) => {
              const l = byCode.get(c);
              return (
                <span key={c} class={`chip ${l && !hasModel(l, quality) ? "dl" : ""}`} title={l && !hasModel(l, quality) ? "Will be downloaded" : undefined}>
                  {l?.name ?? c}
                  <button class="chip-x" aria-label={`Remove ${l?.name ?? c}`} onClick={() => setLangs(chosen.filter((x) => x !== c))}>
                    <X size={12} />
                  </button>
                </span>
              );
            })}
            <select class="inp sm chip-add" value="" onChange={(e) => add((e.target as HTMLSelectElement).value)} aria-label="Add a language">
              <option value="">Add a language…</option>
              {ready.length > 0 && (
                <optgroup label="Ready to use">
                  {ready.map((l) => (
                    <option key={l.code} value={l.code}>{l.name}</option>
                  ))}
                </optgroup>
              )}
              <optgroup label="Download">
                {others.map((l) => (
                  <option key={l.code} value={l.code}>
                    {l.name} ({mb(quality === "best" ? l.bestSize : l.fastSize)})
                  </option>
                ))}
              </optgroup>
            </select>
          </div>
          {chosen.length > 1 && <span class="hint">Each extra language makes recognition slower; pick only the ones the document uses.</span>}
        </div>

        <div class="ocr-field">
          <label>Accuracy</label>
          <div class="seg">
            <button class={`segb ${quality === "fast" ? "on" : ""}`} onClick={() => setQuality("fast")}>
              Standard
            </button>
            <button class={`segb ${quality === "best" ? "on" : ""}`} onClick={() => setQuality("best")}>
              Best
            </button>
          </div>
          <span class="hint">
            {quality === "fast"
              ? "Quick and good for clear scans."
              : "Best for faint or low-quality scans; about 3× slower. Uses larger models, downloaded once."}
          </span>
        </div>

        {missing.length > 0 && (
          <div class="ocr-note">
            Downloads {missing.map((l) => l.name).join(", ")} ({mb(missingSize)}) from Tesseract's official models first.
          </div>
        )}
        {signed && (
          <div class="ocr-note warn">
            This document is digitally signed. Adding text changes its pages, so after saving the signatures will show that the document changed since it was signed.
          </div>
        )}
        {active.length > 0 && (
          <div class="ocr-downloads">
            {active.map(([key, frac]) => {
              const [code, q] = key.split("/") as [string, OcrQuality];
              return (
                <div key={key} class="ocr-dl">
                  <span>Downloading {byCode.get(code)?.name ?? code}{q === "best" ? " (best)" : ""}…</span>
                  <div class="rf-bar">
                    <div style={{ width: `${Math.round(frac * 100)}%` }} />
                  </div>
                  <button class="btn ghost sm" onClick={() => stopDownload(code, q)}>
                    Stop
                  </button>
                </div>
              );
            })}
          </div>
        )}
        {error && <span class="error-text">{error}</span>}

        <span class="hint">
          Downloaded languages are kept for next time; remove them or get others in{" "}
          <button
            class="linkish"
            onClick={() => {
              close();
              openSettings("ocr");
            }}
          >
            Settings › Text recognition
          </button>
          .
        </span>

        <div class="row">
          <button class="btn" onClick={close}>
            Cancel
          </button>
          <button class="btn pri" disabled={preparing || !chosen.length} onClick={() => void start()}>
            {preparing ? "Downloading…" : missing.length ? "Download and recognize" : "Recognize"}
          </button>
        </div>
      </div>
    </div>
  );
}
