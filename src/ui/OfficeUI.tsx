// Office files in the UI: the bar over a converted document (which converter made it, convert with another,
// save as PDF), the Office files dialog (converters found, preference, Paginaut's LibreOffice), and Convert
// to PDF for many files.
import { useEffect, useState } from "preact/hooks";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { FileText, X } from "lucide-preact";
import type { OfficeEngine } from "../api";
import {
  available, convertDialog, convertible, engineName, engines, install, installError, installLibreOffice, isOffice, likelyEngine, loadEngines, officeDialog, OFFICE_EXT, WEB_EXT,
  reconvert, reconverting, removeLibreOffice, runConvert, setPreference, startConvert, stopInstall, type ConvertItem,
} from "../office";
import * as P from "../pages";
import { baseName, openFiles, settings, type Tab } from "../store";
import { startMergePictures, testHook } from "./PagesUI";

const mb = (bytes: number) => `${Math.round(bytes / (1 << 20)).toLocaleString()} MB`;

/** "Converting report.docx with Microsoft Word…" while an Office file opens. */
export function openingText(tab: Tab) {
  if (!isOffice(tab.path)) return `Opening ${tab.name}…`;
  if (!engines.value) void loadEngines();
  const e = likelyEngine(tab.path);
  if (!e) return `Opening ${tab.name}…`;
  if (e === "builtin") return `Reading ${tab.name}…`;
  return `Converting ${tab.name} with ${engineName(e, tab.path)}…`;
}

// ---------------------------------------------------------------- bar

const dismissed = new Set<string>();

export function OfficeBar({ tab }: { tab: Tab }) {
  const info = tab.info.value;
  const [, redraw] = useState(0);
  const busy = reconverting.value.get(tab.key);
  const from = info?.converted?.from ?? (info && isOffice(tab.path) ? tab.path : null);
  useEffect(() => {
    if (from && !engines.value) void loadEngines();
  }, [from]);
  if (!info || !from) return null;
  if (busy) {
    return (
      <div class="formbar officebar">
        <span class="spin" />
        <span>Converting with {busy}…</span>
      </div>
    );
  }
  if (dismissed.has(tab.key)) return null;
  const close = () => {
    dismissed.add(tab.key);
    redraw((n) => n + 1);
  };
  const current: OfficeEngine = info.converted?.engine ?? "builtin";
  const others = available(from).filter((e) => e !== current);
  const convertWith = others.length > 0 && (
    <select
      class="inp sm office-other"
      value=""
      aria-label="Convert with another program"
      onChange={(e) => {
        const v = (e.target as HTMLSelectElement).value as OfficeEngine;
        (e.target as HTMLSelectElement).value = "";
        if (v) void reconvert(tab, v);
      }}
    >
      <option value="">Convert with…</option>
      {others.map((e) => (
        <option key={e} value={e}>
          {e === "builtin" ? "Text preview" : engineName(e, from)}
        </option>
      ))}
    </select>
  );
  if (current === "builtin") {
    const better = others.filter((e) => e !== "builtin");
    return (
      <div class="formbar officebar warn">
        <FileText size={16} />
        <span title="Paginaut's own reader shows the words of Word, Excel and PowerPoint files, without their fonts, pictures or page layout.">
          Text preview: {baseName(from)}'s formatting, pictures and page layout aren't shown.
        </span>
        {better.length ? (
          <button class="btn ghost" onClick={() => void reconvert(tab, better[0])}>
            Show with {engineName(better[0], from)}
          </button>
        ) : (
          <button class="btn ghost" onClick={() => (officeDialog.value = true)}>
            Get LibreOffice (free)…
          </button>
        )}
        <button class="ib sm" aria-label="Dismiss" onClick={close}>
          <X size={14} />
        </button>
      </div>
    );
  }
  return (
    <div class="formbar officebar">
      <FileText size={16} />
      <span title={`Paginaut shows ${baseName(from)} as a PDF made by ${info.converted!.engineName}. Save as PDF keeps it.`}>
        PDF made from {baseName(from)} by {info.converted!.engineName}
      </span>
      <button class="btn ghost" onClick={() => void P.save(tab)}>
        Save as PDF…
      </button>
      {convertWith}
      <button class="ib sm" aria-label="Dismiss" onClick={close}>
        <X size={14} />
      </button>
    </div>
  );
}

// ---------------------------------------------------------------- Office files dialog

/** A suite's name and whether it's here. */
function Suite({ name, on, label }: { name: string; on: boolean; label?: string }) {
  return (
    <div class="office-suite">
      <b>{name}</b>
      <span class={`office-avail ${on ? "on" : ""}`}>{label ?? (on ? "Available" : "Not installed")}</span>
    </div>
  );
}

const STAGES: Record<string, string> = {
  finding: "Finding the latest version…",
  downloading: "Downloading",
  checking: "Checking the download…",
  unpacking: "Unpacking (about a minute)…",
  preparing: "Getting it ready…",
};

export function OfficeDialog() {
  const open_ = officeDialog.value;
  useEffect(() => {
    if (open_) void loadEngines(true);
  }, [open_]);
  useEffect(() => {
    if (!open_) return;
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        officeDialog.value = false;
      }
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [open_]);
  if (!open_) return null;
  const e = engines.value;
  const inst = install.value;
  const pref = settings.value.officeEngine;
  const ms = e?.msoffice;
  const apps = ms ? [ms.word && "Word", ms.excel && "Excel", ms.powerpoint && "PowerPoint"].filter(Boolean).join(", ") : "";
  const choice = (value: typeof pref, label: string, note: string, disabled = false) => (
    <label class={`check-row office-choice ${disabled ? "off" : ""}`}>
      <input type="radio" name="office-engine" checked={pref === value} disabled={disabled} onChange={() => setPreference(value)} />
      <span>
        {label}
        <small>{note}</small>
      </span>
    </label>
  );
  return (
    <div class="overlay" onPointerDown={(ev) => ev.target === ev.currentTarget && (officeDialog.value = false)}>
      <div class="dialog tool-dialog office-dialog" role="dialog" aria-label="Office files">
        <button class="ib sm close" aria-label="Close" onClick={() => (officeDialog.value = false)}>
          <X size={16} />
        </button>
        <h2>Office files</h2>
        <span class="hint">
          Word, Excel and PowerPoint files (and OpenDocument ones) open as PDFs made from them, so you can read, mark up, sign and save them like
          any PDF. How closely they match the original depends on the program that makes the PDF.
        </span>

        <div class="tool-field">
          <label>Office suite choices</label>
          {!e ? (
            <span class="hint">Looking…</span>
          ) : (
            <div class="office-engines">
              <div class="office-engine">
                <Suite name="Microsoft Office" on={!!ms} />
                {ms && <span>{`${ms.version} · ${apps}`}</span>}
                <small>{ms ? "Documents look exactly as their authors saw them." : "When it's installed, Paginaut uses it."}</small>
              </div>
              <div class="office-engine">
                <Suite name="LibreOffice" on={!!(e.pack || e.installed)} />
                {e.pack ? (
                  <span>
                    Paginaut's copy, version {e.pack.version} · {mb(e.pack.size)}{" "}
                    <button class="linkish" disabled={!!inst} onClick={() => void removeLibreOffice()}>
                      Remove
                    </button>
                  </span>
                ) : (
                  e.installed && <span>Installed{e.installed.version ? `, version ${e.installed.version}` : ""} · {e.installed.path}</span>
                )}
                <small>Free. Very close to Office in our tests; spreadsheets may break pages in other places.</small>
                {!e.pack && !inst && (
                  <div class="ps-line">
                    <button class="btn sm" onClick={() => void installLibreOffice()}>
                      {e.installed ? "Get Paginaut's own copy (~360 MB)" : "Download LibreOffice (~360 MB)"}
                    </button>
                    <span class="hint">From the Document Foundation; checked, then unpacked for Paginaut only (no installation, about 600 MB on disk).</span>
                  </div>
                )}
                {!inst && installError.value && <span class="error-text">Couldn't set up LibreOffice: {installError.value}</span>}
                {inst && (
                  <div class="ocr-dl office-dl">
                    <span>
                      {inst.stopping ? "Stopping…" : STAGES[inst.stage]}
                      {inst.stage === "downloading" && inst.total ? ` · ${mb(inst.done)} of ${mb(inst.total)}` : ""}
                    </span>
                    <div class="rf-bar">
                      <div style={{ width: inst.stage === "downloading" && inst.total ? `${Math.round((inst.done / inst.total) * 100)}%` : inst.stage === "finding" ? "0%" : "100%" }} />
                    </div>
                    {(inst.stage === "finding" || inst.stage === "downloading") && (
                      <button class="btn ghost sm" disabled={inst.stopping} onClick={stopInstall}>
                        Stop
                      </button>
                    )}
                  </div>
                )}
              </div>
              <div class="office-engine">
                <Suite name="Text preview" on label="Always available (built in)" />
                <small>Shows the words of .docx, .xlsx and .pptx files, without their formatting.</small>
              </div>
            </div>
          )}
        </div>

        <div class="tool-field">
          <label>Open Office files with</label>
          {choice("auto", "The best one here", "Microsoft Office, else LibreOffice, else the text preview")}
          {choice("msoffice", "Microsoft Office", "Word, Excel or PowerPoint", !!e && !ms)}
          {choice("libreoffice", "LibreOffice", "", !!e && !e.pack && !e.installed)}
          {choice("builtin", "Text preview", "Quickest, but only the words")}
          <span class="hint">Any file can be converted again with another program from the bar above it.</span>
        </div>

        <div class="row">
          <span style={{ flex: 1 }} />
          <button class="btn pri" onClick={() => (officeDialog.value = false)}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- Convert to PDF

async function pickFiles(web = false): Promise<string[]> {
  const office = { name: "Office documents", extensions: OFFICE_EXT };
  const pages = { name: "Web pages", extensions: WEB_EXT };
  const res =
    testHook()?.pick ??
    (await open({ multiple: true, title: web ? "Web pages to convert" : "Files to convert", filters: web ? [pages, office] : [office, pages, { name: "E-books, XPS, text", extensions: ["epub", "mobi", "fb2", "xps", "oxps", "txt"] }] }));
  return Array.isArray(res) ? res : res ? [res] : [];
}

/** Files to PDF (in the Convert pane), starting with files picked (Office files first, or web pages). */
export async function pickAndConvert(web = false) {
  officeDialog.value = false;
  const files = await pickFiles(web);
  if (files.length) startConvert(files);
}

const STATE: Record<ConvertItem["state"], string> = { waiting: "", converting: "Converting…", done: "Done", error: "Failed" };

/** "Files to PDF" in the Convert pane: Office files, web pages, e-books, each with its own program. */
export function FilesToPdf() {
  const d = convertDialog.value ?? { items: [], folder: null, running: false };
  const set = (patch: Partial<typeof d>) => (convertDialog.value = { ...d, ...convertDialog.value, ...patch });
  const waiting = d.items.filter((i) => i.state !== "done").length;
  const done = d.items.filter((i) => i.state === "done");
  const add = async () => {
    const files = await pickFiles();
    if (files.length) startConvert(files);
  };
  return (
    <>
      <span class="hint">Word, Excel and PowerPoint files, web pages and e-books.</span>
      {d.items.length > 0 && (
        <div class="convert-list">
          {d.items.map((it, i) => {
            const list = available(it.path);
            return (
              <div class={`convert-row ${it.state}`} key={it.path}>
                <div class="convert-top">
                  <span class="convert-name" title={it.path}>
                    {baseName(it.path)}
                  </span>
                  <span class="convert-state">{it.state === "converting" ? <span class="spin" /> : STATE[it.state]}</span>
                  {!d.running && it.state !== "done" && (
                    <button class="ib sm" aria-label={`Remove ${baseName(it.path)}`} onClick={() => set({ items: d.items.filter((_, j) => j !== i) })}>
                      <X size={14} />
                    </button>
                  )}
                </div>
                {it.state === "done" ? (
                  <span class="convert-note">
                    {it.note && `by ${it.note} · `}
                    <button class="linkish" onClick={() => void openFiles([it.out!])}>
                      Open
                    </button>
                  </span>
                ) : it.state === "error" ? (
                  <span class="convert-note error-text">{it.note}</span>
                ) : !isOffice(it.path) ? (
                  <span class="convert-note">Made by Paginaut</span>
                ) : (
                  <select
                    class="inp sm"
                    value={it.engine}
                    disabled={d.running}
                    aria-label={`Program for ${baseName(it.path)}`}
                    onChange={(e) => {
                      const items = d.items.slice();
                      items[i] = { ...it, engine: (e.target as HTMLSelectElement).value as ConvertItem["engine"], state: "waiting", note: undefined };
                      set({ items });
                    }}
                  >
                    <option value="auto">{list[0] && list[0] !== "builtin" ? `Auto (${engineName(likelyEngine(it.path) ?? list[0], it.path)})` : "Auto"}</option>
                    {list
                      .filter((e) => e !== "builtin")
                      .map((e) => (
                        <option key={e} value={e}>
                          {engineName(e, it.path)}
                        </option>
                      ))}
                  </select>
                )}
              </div>
            );
          })}
        </div>
      )}
      {engines.value && !engines.value.msoffice && !engines.value.pack && !engines.value.installed && d.items.some((i) => isOffice(i.path)) && (
        <span class="secure-warn">
          Office files need Microsoft Office or LibreOffice.{" "}
          <button class="linkish" onClick={() => (officeDialog.value = true)}>
            Get LibreOffice (free)…
          </button>
        </span>
      )}
      {d.items.length > 0 && (
        <>
          <div class="seg nowrap">
            <button class={`segb ${!d.folder ? "on" : ""}`} disabled={d.running} onClick={() => set({ folder: null })}>
              Next to each file
            </button>
            <button
              class={`segb ${d.folder ? "on" : ""}`}
              disabled={d.running}
              title={d.folder ?? undefined}
              onClick={async () => {
                const f = await open({ directory: true, title: "Folder for the PDFs" });
                if (typeof f === "string") set({ folder: f });
              }}
            >
              {d.folder ? baseName(d.folder) : "In a folder…"}
            </button>
          </div>
          <span class="hint">Existing files are never replaced: a new PDF gets a number, like "report (2).pdf".</span>
        </>
      )}
      <div class="ps-line">
        <button class="btn sm" disabled={d.running} onClick={() => void add()}>
          {d.items.length ? "Add files…" : "Choose files…"}
        </button>
        <button class="btn ghost sm" disabled={d.running} title="Pictures into one PDF (in the Merge dialog)" onClick={() => void startMergePictures()}>
          Pictures to one PDF…
        </button>
        {done.length > 0 && !d.running && (
          <button class="btn ghost sm" onClick={() => void revealItemInDir(done[0].out!)}>
            Show in folder
          </button>
        )}
        <span style={{ flex: 1 }} />
        {d.running ? (
          <span class="secure-busy">
            <span class="spin" /> Converting…
          </span>
        ) : waiting ? (
          <button class="btn pri sm" onClick={() => void runConvert()}>
            Convert {waiting}
          </button>
        ) : (
          d.items.length > 0 && (
            <button class="btn ghost sm" onClick={() => (convertDialog.value = null)}>
              Clear
            </button>
          )
        )}
      </div>
    </>
  );
}
