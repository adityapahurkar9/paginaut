// The Convert pane (right side, sharing the place of the Sign pane): this document to Word, Excel, PowerPoint,
// pictures or text; and files (Office, web pages, e-books) to PDF.
import { useEffect, useRef, useState } from "preact/hooks";
import { save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { X } from "lucide-preact";
import { api, newId, type ExportDone, type ExportFormat, type OfficeEngine, type PdfaReport, type PdfaSaved } from "../api";
import { bookScreen } from "../modes";
import { parsePages } from "../ocr";
import { convertTarget, openConvertPane, type ExportTarget } from "../office";
import { closePane, pane } from "../panes";
import { activeTab, baseName, openSettings, type Tab } from "../store";
import { FilesToPdf } from "./OfficeUI";
import { testHook } from "./PagesUI";

/** Opens the pane at a conversion of this document (Convert menu, command palette). */
export function startExport(_tab: Tab | null, target: ExportTarget) {
  openConvertPane(target);
}

const TARGETS: [ExportTarget, string][] = [["docx", "Word"], ["xlsx", "Excel"], ["pptx", "PowerPoint"], ["md", "Markdown"], ["images", "Images"], ["txt", "Text"], ["pdfa", "PDF/A"]];
const WHAT: Record<ExportTarget, string> = {
  docx: "An editable Word document.",
  xlsx: "Every table found on the pages, a sheet each, numbers as numbers. (Scanned pages need Tools › OCR first.)",
  pptx: "A slide per page.",
  md: "Headings, lists, tables and pictures as Markdown text (pictures go in a folder next to it).",
  images: "Each page as a picture.",
  txt: "The pages' text, in reading order.",
  pdfa: "A PDF for keeping: it opens and looks the same for years, as archives, courts and many portals ask. A new file; this one stays as it is.",
};

/** "1-3, 7" from page numbers. */
function pageList(pages: number[]) {
  const out: string[] = [];
  for (let i = 0; i < pages.length; i++) {
    let j = i;
    while (j + 1 < pages.length && pages[j + 1] === pages[j] + 1) j++;
    out.push(j > i ? `${pages[i]}–${pages[j]}` : `${pages[i]}`);
    i = j;
  }
  return out.length > 8 ? `${out.slice(0, 8).join(", ")}…` : out.join(", ");
}

/** What a PDF/A check found, as a list. */
function PdfaIssues({ report }: { report: PdfaReport }) {
  const claim = report.claim ? `Marked as PDF/A-${report.claim.part}${report.claim.conformance.toLowerCase()}` : "Not marked as PDF/A";
  if (!report.issues.length) return <span class="hint">{claim}. Meets PDF/A-{report.part}b as far as Paginaut checks.</span>;
  return (
    <ul class="pdfa-issues">
      {report.issues.map((i) => (
        <li key={i.id} class={i.fix ? "" : "pdfa-stays"}>
          {i.text}
          {i.pages.length > 0 && <small> · {i.pages.length === 1 ? "page" : "pages"} {pageList(i.pages)}</small>}
          {!i.fix && <small> · can't be put right here</small>}
        </li>
      ))}
    </ul>
  );
}
const ENGINE_NOTE: Record<string, Record<string, [string, string]>> = {
  docx: {
    msoffice: ["Microsoft Word", "Word reads the PDF itself: paragraphs, tables and pictures stay editable, close to the page layout."],
    builtin: ["Paginaut", "The text, tables and pictures in reading order; the page layout isn't kept."],
  },
  pptx: {
    libreoffice: ["LibreOffice", "Editable slides: text boxes, shapes and pictures."],
    msoffice: ["Microsoft PowerPoint", "Each page as a picture on its slide (not editable)."],
  },
};

export function ConvertPane() {
  const tab = activeTab.value;
  const ready = tab?.status.value === "ready" ? tab : null;
  if (pane.value !== "convert" || bookScreen.value) return null;
  return (
    <aside class="signpane convertpane" aria-label="Convert">
      <div class="signpane-head">
        <strong>Convert</strong>
        <button class="ib sm" aria-label="Close the Convert pane" title="Close" onClick={() => closePane("convert")}>
          <X size={15} />
        </button>
      </div>
      <div class="signpane-body">
        <section class="pane-sec">
          <h3>This document to</h3>
          {ready ? <ThisDocument key={ready.key} tab={ready} /> : <p class="muted small">Open a document to convert it to Word, Excel, PowerPoint, Markdown, pictures or text.</p>}
        </section>
        <section class="pane-sec">
          <h3>Files to PDF</h3>
          <FilesToPdf />
        </section>
        <section class="pane-sec">
          <button class="linkish pane-link" onClick={() => openSettings("office")}>
            Office suite choices…
          </button>
        </section>
      </div>
    </aside>
  );
}

function ThisDocument({ tab }: { tab: Tab }) {
  const target = convertTarget.value;
  const setTarget = (t: ExportTarget) => (convertTarget.value = t);
  const [opts, setOpts] = useState<{ docx: OfficeEngine[]; pptx: OfficeEngine[] } | null>(null);
  /** The running conversion's id, for Stop. */
  const job = useRef(0);
  const [engine, setEngine] = useState<Record<string, OfficeEngine>>({});
  const [pages, setPages] = useState<"all" | "selected" | "range">("all");
  const [range, setRange] = useState("");
  const [image, setImage] = useState<"png" | "jpg" | "tiff">("png");
  const [dpi, setDpi] = useState(150);
  const [busy, setBusy] = useState<{ done: number; total: number; stopping?: boolean } | null>(null);
  const [error, setError] = useState("");
  const [done, setDone] = useState<ExportDone | null>(null);
  const [part, setPart] = useState<2 | 3>(2);
  const [report, setReport] = useState<PdfaReport | null>(null);
  const [checking, setChecking] = useState(false);
  const [pdfaDone, setPdfaDone] = useState<PdfaSaved | null>(null);
  const edits = tab.edits.value;
  const pdfa = target === "pdfa";
  const count = edits?.order.length ?? 0;
  const selected = [...tab.pageSel.value].sort((a, b) => a - b);

  useEffect(() => {
    api.convertOptions().then(setOpts, (e) => setError(String(e)));
  }, []);
  // A new choice clears the last result.
  useEffect(() => {
    setDone(null);
    setPdfaDone(null);
    setError("");
  }, [target, part]);
  // PDF/A: checked as soon as it's chosen (and again for the other part). Only the latest check's answer counts.
  const checkId = useRef(0);
  const check = async () => {
    const id = ++checkId.current;
    setChecking(true);
    setError("");
    try {
      const r = await api.pdfaCheck(tab.info.value!.docId, part);
      if (id === checkId.current) setReport(r);
    } catch (e) {
      if (id !== checkId.current) return;
      setReport(null);
      setError(String(e));
    } finally {
      if (id === checkId.current) setChecking(false);
    }
  };
  useEffect(() => {
    if (pdfa && tab.info.value?.isPdf) void check();
  }, [pdfa, part]);

  const list = target === "docx" || target === "pptx" ? (opts?.[target] ?? []) : [];
  const chosen = target === "docx" || target === "pptx" ? (engine[target] ?? list[0]) : undefined;
  const format: ExportFormat | null = target === "images" ? image : target === "pdfa" ? null : target;
  const unavailable = target === "pptx" && opts && list.length === 0;
  const usePages = pages === "selected" && selected.length < 2 ? "all" : pages;

  const run = async () => {
    if (!edits) return;
    // Pages as shown → the document's pages.
    const shown = usePages === "all" ? [...Array(count).keys()] : usePages === "selected" ? selected : parsePages(range, count);
    if (!shown?.length) return setError(`Type pages like "1-3, 7" (1 to ${count}).`);
    const order = shown.map((i) => edits.order[i]);
    const stem = baseName(tab.path).replace(/\.[^.]+$/, "");
    if (pdfa || !format) return void savePdfa(order, stem);
    const ext = format;
    const names: Record<string, string> = { docx: "Word document", xlsx: "Excel workbook", pptx: "PowerPoint presentation", md: "Markdown", png: "PNG picture", jpg: "JPEG picture", tiff: "TIFF picture", txt: "Text" };
    const out =
      testHook()?.save ??
      (await save({ title: "Save as", defaultPath: tab.path.replace(/[^\\/]+$/, `${stem}.${ext}`), filters: [{ name: names[format], extensions: [ext] }] }));
    if (!out) return;
    setError("");
    setDone(null);
    setBusy({ done: 0, total: order.length });
    try {
      job.current = newId();
      const r = await api.convertFromPdf(
        { doc: tab.info.value!.docId, order, rot: edits.rot, format, out, engine: chosen ?? null, dpi: target === "images" ? dpi : null },
        job.current,
        (e) => setBusy((b) => ({ ...b, done: e.done, total: e.total })),
      );
      setDone(r);
    } catch (e) {
      if (String(e) !== "stopped") setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const savePdfa = async (order: number[], stem: string) => {
    const defaultPath = tab.path.replace(/[^\\/]+$/, `${stem} (PDF-A).pdf`);
    const out = testHook()?.save ?? (await save({ title: "Save as PDF/A", defaultPath, filters: [{ name: "PDF", extensions: ["pdf"] }] }));
    if (!out) return;
    if (out.toLowerCase() === tab.path.toLowerCase()) return setError("Choose another name: the PDF/A copy is a new file, and this one stays as it is.");
    setError("");
    setPdfaDone(null);
    setBusy({ done: 0, total: 1 });
    try {
      setPdfaDone(await api.pdfaSave(tab.info.value!.docId, order, edits!.rot, out, part));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const seg = <T extends string>(value: T, items: [T, string][], set: (v: T) => void) => (
    <div class="seg wrap">
      {items.map(([v, label]) => (
        <button key={v} class={`segb ${value === v ? "on" : ""}`} disabled={!!busy} onClick={() => set(v)}>
          {label}
        </button>
      ))}
    </div>
  );

  return (
    <>
      {seg(target, TARGETS, setTarget)}
      <span class="hint">{WHAT[target]}</span>
      {list.length > 0 && (
        <div class="tool-field">
          <label>Made by</label>
          {list.map((e) => (
            <label key={e} class="check-row office-choice">
              <input type="radio" name="export-engine" checked={chosen === e} disabled={!!busy} onChange={() => setEngine({ ...engine, [target]: e })} />
              <span>
                {ENGINE_NOTE[target][e]?.[0] ?? e}
                <small>{ENGINE_NOTE[target][e]?.[1]}</small>
              </span>
            </label>
          ))}
        </div>
      )}
      {unavailable && (
        <span class="secure-warn">
          PowerPoint files need LibreOffice (free) or Microsoft PowerPoint.{" "}
          <button class="linkish" onClick={() => openSettings("office")}>
            Get LibreOffice…
          </button>
        </span>
      )}
      {target === "images" && (
        <div class="tool-field">
          <label>Pictures</label>
          {seg(image, [["png", "PNG"], ["jpg", "JPEG"], ["tiff", "TIFF"]], setImage)}
          {seg(String(dpi), [["72", "72 dpi"], ["150", "150"], ["300", "300"], ["600", "600"]], (v) => setDpi(Number(v)))}
          <span class="hint">{image === "tiff" ? "All the pages in one TIFF file." : "A file per page, numbered after the name you choose (report-1.png…)."}</span>
        </div>
      )}
      {pdfa && (
        <div class="tool-field">
          <label>Kind</label>
          {seg(String(part), [["2", "PDF/A-2b"], ["3", "PDF/A-3b"]], (v) => setPart(Number(v) as 2 | 3))}
          <span class="hint">
            {part === 2 ? "What most portals and archives ask for." : "Like PDF/A-2b, and keeps attached files of any kind (e-invoices need it)."}
          </span>
          {!tab.info.value?.isPdf ? (
            <span class="hint">This file is drawn into a new PDF first, then made PDF/A.</span>
          ) : checking ? (
            <span class="secure-busy">
              <span class="spin" /> Checking…
            </span>
          ) : (
            report && (
              <>
                <PdfaIssues report={report} />
                {report.signed > 0 && (
                  <span class="secure-warn">
                    This file is signed. The PDF/A copy is a new file, so {report.signed === 1 ? "its signature isn't" : "its signatures aren't"} valid in it: sign the copy
                    instead.
                  </span>
                )}
                {part === 2 && report.issues.some((i) => i.id === "attachments") && <span class="hint">PDF/A-2b takes those attached files out; PDF/A-3b keeps them.</span>}
                <button class="linkish pane-link" onClick={() => void check()}>
                  Check again
                </button>
              </>
            )
          )}
        </div>
      )}
      <div class="tool-field">
        <label>Pages</label>
        {seg(usePages, [["all", `All ${count}`], ...(selected.length > 1 ? [["selected", `Selected (${selected.length})`] as ["selected", string]] : []), ["range", "Pages…"]], setPages)}
        {usePages === "range" && (
          <input class="inp sm" placeholder="1-3, 7" value={range} autoFocus onInput={(e) => setRange((e.target as HTMLInputElement).value)} onKeyDown={(e) => e.stopPropagation()} />
        )}
      </div>
      {done && (
        <div class="export-result">
          <span>
            Saved {done.files.length === 1 ? baseName(done.files[0]) : `${done.files.length} files`}
            {done.by !== "Paginaut" ? ` · made by ${done.by}` : ""}
          </span>
          {done.note && <small>{done.note}</small>}
          <div class="ps-line">
            {done.files.length === 1 && (
              <button class="btn sm" onClick={() => void api.openPath(done.files[0]).catch((e) => setError(String(e)))}>
                Open
              </button>
            )}
            <button class="btn ghost sm" onClick={() => void revealItemInDir(done.files[0])}>
              Show in folder
            </button>
          </div>
        </div>
      )}
      {pdfaDone && (
        <div class="export-result">
          <span>Saved {baseName(pdfaDone.path)}</span>
          <ul class="pdfa-done">
            {pdfaDone.done.map((d) => (
              <li key={d}>{d}</li>
            ))}
          </ul>
          {pdfaDone.substitutes.length > 0 && (
            <div class="secure-warn">
              Some fonts couldn't be put in the file as they are, so others were (the text may look a little different):
              <ul class="pdfa-done">
                {pdfaDone.substitutes.map((s) => (
                  <li key={s}>{s}</li>
                ))}
              </ul>
            </div>
          )}
          {pdfaDone.left.issues.length > 0 ? (
            <>
              <small>Still not PDF/A:</small>
              <PdfaIssues report={pdfaDone.left} />
            </>
          ) : (
            <small>Checked: it meets PDF/A-{pdfaDone.left.part}b as far as Paginaut checks.</small>
          )}
          <div class="ps-line">
            <button class="btn sm" onClick={() => void api.openPath(pdfaDone.path).catch((e) => setError(String(e)))}>
              Open
            </button>
            <button class="btn ghost sm" onClick={() => void revealItemInDir(pdfaDone.path)}>
              Show in folder
            </button>
          </div>
        </div>
      )}
      {error && <span class="error-text">{error}</span>}
      <div class="ps-line">
        {busy ? (
          <>
            <span class="secure-busy" style={{ flex: 1 }}>
              <span class="spin" /> {pdfa ? "Saving as PDF/A…" : busy.stopping ? "Stopping…" : busy.total > 1 && busy.done ? `Page ${busy.done + 1} of ${busy.total}…` : "Converting…"}
            </span>
            {!pdfa && (
              <button
                class="btn ghost sm"
                disabled={busy.stopping}
                onClick={() => {
                  setBusy({ ...busy, stopping: true });
                  void api.convertStop(job.current);
                }}
              >
                Stop
              </button>
            )}
          </>
        ) : (
          <button class="btn pri sm export-go" disabled={!opts || !!unavailable || (pdfa && checking)} onClick={() => void run()}>
            {pdfa ? "Save as PDF/A…" : "Convert…"}
          </button>
        )}
      </div>
    </>
  );
}
