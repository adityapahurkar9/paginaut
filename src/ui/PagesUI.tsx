// Page tools that take pages from other files: the Insert pages dialog (into
// this document), blank pages, and the Merge dialog (files into a new PDF).
import { signal } from "@preact/signals";
import { open, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useRef, useState } from "preact/hooks";
import { ArrowDown, ArrowUp, FileText, GripVertical, Image, Lock, X } from "lucide-preact";
import { OPENABLE } from "../actions";
import { OFFICE_EXT } from "../office";
import { api, type InsertSource } from "../api";
import { docSigs } from "../dsc";
import { parsePages } from "../ocr";
import * as P from "../pages";
import { baseName, openFiles, samePath, showToast, tabs, type Tab } from "../store";
import { toolPane } from "../toolpanes";
import { ToolFrame } from "./ToolFrame";

const PICTURES = new Set(["png", "jpg", "jpeg", "jfif", "tif", "tiff", "bmp", "gif", "jp2", "jpx", "psd", "pnm", "pbm", "pgm", "ppm", "pam", "jxr"]);
const isPicture = (path: string) => PICTURES.has(path.split(".").pop()?.toLowerCase() ?? "");
const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;
const NOT_PDF = "Pages can only be added to a PDF. Save this file as a PDF first (Save as).";

interface FileEntry {
  /** Tells rows apart when the same file is added twice. */
  key: number;
  path: string;
  pages: number;
  isPdf: boolean;
  locked: boolean;
  password: string;
  /** The password given was wrong. */
  wrong: boolean;
  /** Pages to take, like "1-5, 8" (empty: all). */
  range: string;
}

let nextKey = 1;

declare const process: { env: { NODE_ENV: string } };
/**
 * Development builds: tools/cdp.mjs sets window.paginautTest.pick (files) and
 * .save (a path) to answer the file dialogs, which can't be driven from there.
 */
export function testHook(): { pick?: string[]; save?: string } | null {
  if (process.env.NODE_ENV === "production") return null;
  const t = (window as unknown as { paginautTest?: { pick?: string[]; save?: string } }).paginautTest;
  if (!t) return null;
  // Picks used up by a test answer nothing: the real dialog opens again.
  return { pick: t.pick?.length ? t.pick : undefined, save: t.save };
}

async function peekAll(paths: string[]): Promise<FileEntry[]> {
  const out: FileEntry[] = [];
  for (const path of paths) {
    try {
      const p = await api.peekFile(path);
      out.push({ key: nextKey++, path, pages: p.pages, isPdf: p.isPdf, locked: p.locked, password: "", wrong: false, range: "" });
    } catch (e) {
      showToast(String(e), 5000);
    }
  }
  return out;
}

async function pickFiles(title: string): Promise<FileEntry[]> {
  const res =
    testHook()?.pick ??
    (await open({
      multiple: true,
      title,
      filters: [
        { name: "Documents and pictures", extensions: OPENABLE },
        { name: "PDF", extensions: ["pdf"] },
        { name: "Office documents", extensions: OFFICE_EXT },
        { name: "Pictures", extensions: [...PICTURES] },
      ],
    }));
  return peekAll(Array.isArray(res) ? res : res ? [res] : []);
}

/** The pages a row takes (0-based), undefined for all; null if the range isn't valid. */
function rowPages(f: FileEntry): number[] | undefined | null {
  if (!f.range.trim()) return undefined;
  return parsePages(f.range, f.pages);
}

const rowCount = (f: FileEntry) => {
  const p = rowPages(f);
  return p === undefined ? f.pages : (p?.length ?? 0);
};

/** Checks the rows: a message for the first problem, or null. */
function problem(files: FileEntry[]): string | null {
  if (!files.length) return "Add a file to take pages from.";
  const locked = files.find((f) => f.locked);
  if (locked) return `${baseName(locked.path)} needs its password.`;
  const bad = files.find((f) => rowPages(f) === null);
  if (bad) return `Pages of ${baseName(bad.path)} must be numbers from 1 to ${bad.pages}, like 1-5, 8.`;
  return null;
}

const sourcesOf = (files: FileEntry[]): InsertSource[] =>
  files.map((f) => ({ path: f.path, pages: rowPages(f) ?? undefined, password: f.password || undefined }));

/** Marks the file a PASSWORD:/WRONG_PASSWORD: error names; returns the message to show. */
function passwordError(msg: string, files: FileEntry[], setFiles: (f: FileEntry[]) => void): string {
  const m = /^(WRONG_)?PASSWORD:(.*)$/.exec(msg);
  if (!m) return msg;
  setFiles(files.map((f) => (f.path === m[2] ? { ...f, locked: true, wrong: !!m[1] } : f)));
  return `${baseName(m[2])} needs its password.`;
}

function useEscape(close: () => void, deps: unknown[]) {
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        close();
      }
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, deps);
}

/**
 * The files a dialog takes pages from: remove, unlock with a password, and
 * (optionally) a page range each and reordering by dragging or arrows.
 */
function FileList({ files, setFiles, ranges, reorder, title }: { files: FileEntry[]; setFiles: (f: FileEntry[]) => void; ranges: boolean; reorder: boolean; title: string }) {
  const list = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<{ from: number; over: number } | null>(null);
  const update = (i: number, patch: Partial<FileEntry>) => setFiles(files.map((f, k) => (k === i ? { ...f, ...patch } : f)));
  const move = (from: number, to: number) => {
    if (to < 0 || to >= files.length || from === to) return;
    const next = files.slice();
    const [f] = next.splice(from, 1);
    next.splice(to, 0, f);
    setFiles(next);
  };
  /** Dragging a row by its grip (pointer events: the window's file drop takes HTML drag and drop). */
  const startDrag = (from: number, e: PointerEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const rowAt = (y: number) => {
      const rows = [...(list.current?.querySelectorAll<HTMLElement>(".ins-file") ?? [])];
      const i = rows.findIndex((r) => y < r.getBoundingClientRect().bottom);
      return i < 0 ? rows.length - 1 : i;
    };
    setDrag({ from, over: from });
    const moveTo = (ev: PointerEvent) => setDrag({ from, over: rowAt(ev.clientY) });
    const up = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", moveTo);
      window.removeEventListener("pointerup", up);
      setDrag(null);
      move(from, rowAt(ev.clientY));
    };
    window.addEventListener("pointermove", moveTo);
    window.addEventListener("pointerup", up);
  };
  /** Checks a typed password; the file's page count comes with it. */
  const unlock = async (i: number) => {
    const f = files[i];
    const p = await api.peekFile(f.path, f.password).catch(() => null);
    if (!p || p.locked) return update(i, { wrong: true });
    update(i, { locked: false, wrong: false, pages: p.pages });
  };
  const addMore = async () => {
    const more = await pickFiles(title);
    if (more.length) setFiles([...files, ...more]);
  };
  return (
    <div class="ins-files" ref={list}>
      {files.map((f, i) => (
        <div
          key={f.key}
          class={`ins-file ${drag && drag.over === i && drag.from !== i ? (drag.over > drag.from ? "drop-below" : "drop-over") : ""} ${drag?.from === i ? "dragging" : ""}`}
        >
          <div class="ins-file-row">
            {reorder && (
              <span class="ins-grip" title="Drag to move" onPointerDown={(e) => startDrag(i, e)}>
                <GripVertical size={14} />
              </span>
            )}
            {f.locked ? <Lock size={15} /> : f.isPdf ? <FileText size={15} /> : <Image size={15} />}
            <span class="ins-name" title={f.path}>
              {baseName(f.path)}
            </span>
            {ranges && !f.locked && f.pages > 1 ? (
              <input
                class="inp sm ins-range"
                placeholder={`All ${f.pages} pages`}
                title={`Pages to take, like 1-5, 8 (of ${f.pages}); empty for all`}
                value={f.range}
                onInput={(e) => update(i, { range: (e.target as HTMLInputElement).value })}
              />
            ) : (
              <span class="muted">{f.locked ? "locked" : isPicture(f.path) ? "picture" : plural(f.pages, "page")}</span>
            )}
            {reorder && (
              <>
                <button class="ib sm" aria-label="Move up" title="Move up" disabled={i === 0} onClick={() => move(i, i - 1)}>
                  <ArrowUp size={13} />
                </button>
                <button class="ib sm" aria-label="Move down" title="Move down" disabled={i === files.length - 1} onClick={() => move(i, i + 1)}>
                  <ArrowDown size={13} />
                </button>
              </>
            )}
            <button class="ib sm" aria-label={`Remove ${baseName(f.path)}`} title="Remove" onClick={() => setFiles(files.filter((_, k) => k !== i))}>
              <X size={13} />
            </button>
          </div>
          {ranges && f.range && rowPages(f) === null && <span class="error-text">Pages must be numbers from 1 to {f.pages}, like 1-5, 8.</span>}
          {f.locked && (
            <div class="ins-file-row">
              <input
                class="inp sm"
                type="password"
                placeholder="Password"
                value={f.password}
                onInput={(e) => update(i, { password: (e.target as HTMLInputElement).value, wrong: false })}
                onKeyDown={(e) => e.key === "Enter" && void unlock(i)}
              />
              <button class="btn ghost sm" onClick={() => void unlock(i)}>
                Unlock
              </button>
              {f.wrong && <span class="error-text">Wrong password</span>}
            </div>
          )}
        </div>
      ))}
      <button class="btn ghost sm ins-add" onClick={() => void addMore()}>
        Add files…
      </button>
    </div>
  );
}

const PAPER: Record<string, [number, number]> = { A4: [595, 842], Letter: [612, 792] };

function paperName([w, h]: [number, number]) {
  const [a, b] = w < h ? [w, h] : [h, w];
  for (const [name, [pw, ph]] of Object.entries(PAPER)) if (Math.abs(a - pw) < 3 && Math.abs(b - ph) < 3) return name;
  return `${Math.round((a / 72) * 25.4)} × ${Math.round((b / 72) * 25.4)} mm`;
}

/** "doc": a given size (this document's page); a paper name; or the picture's own size. */
type Fit = "doc" | "A4" | "Letter" | "own";

function FitChoice({ fit, setFit, docSize }: { fit: Fit; setFit: (f: Fit) => void; docSize: [number, number] | null }) {
  const docName = docSize ? paperName(docSize) : null;
  return (
    <div class="tool-field">
      <label>Page size for pictures</label>
      <div class="seg nowrap">
        {docSize && (
          <button class={`segb ${fit === "doc" ? "on" : ""}`} onClick={() => setFit("doc")} title="The size of the page you're on">
            Like this document ({docName})
          </button>
        )}
        {Object.keys(PAPER)
          .filter((k) => k !== docName)
          .map((k) => (
            <button key={k} class={`segb ${fit === k ? "on" : ""}`} onClick={() => setFit(k as Fit)}>
              {k}
            </button>
          ))}
        <button class={`segb ${fit === "own" ? "on" : ""}`} onClick={() => setFit("own")}>
          Picture's own size
        </button>
      </div>
      <span class="hint">{fit === "own" ? "Each page is the size of its picture." : "Each picture is fitted to the page, turned to match its shape."}</span>
    </div>
  );
}

const fitSize = (fit: Fit, docSize: [number, number] | null): [number, number] | null =>
  fit === "own" ? null : fit === "doc" ? docSize : PAPER[fit];

// ---------------------------------------------------------------- insert

export const insertDialog = signal<{ tab: Tab; files: FileEntry[] } | null>(null);
toolPane("insert", insertDialog, { tabOf: (v) => v.tab });

/** Insert pages from files: pick them, then choose pages and where they go. */
export async function startInsert(tab: Tab) {
  if (!tab.info.value?.isPdf) return showToast(NOT_PDF, 4000);
  const files = await pickFiles("Insert pages from");
  if (files.length) insertDialog.value = { tab, files };
}

/** Size of the displayed page `view` (turned as it's shown), in points. */
function viewSize(tab: Tab, view: number): [number, number] {
  const e = tab.edits.value;
  const src = e?.order[view] ?? 0;
  const [w, h] = tab.sizes.value?.[src] ?? tab.info.value?.firstSize ?? [612, 792];
  return (e?.rot[src] ?? 0) % 180 ? [h, w] : [w, h];
}

/** A blank page after the current one, the same size as it. */
export async function insertBlankPage(tab: Tab) {
  if (!tab.info.value?.isPdf) return showToast(NOT_PDF, 4000);
  const at = tab.page.value + 1;
  try {
    await P.insertPages(tab, at, { blank: viewSize(tab, tab.page.value) });
    showToast(`Blank page added as page ${at + 1} · Ctrl+Z to undo`);
  } catch (e) {
    showToast(`Couldn't add a page: ${e}`, 5000);
  }
}

export function InsertDialog() {
  const st = insertDialog.value;
  return st ? <InsertDialogInner key={st.tab.key} tab={st.tab} initial={st.files} /> : null;
}

type Where = "after" | "before" | "start" | "end";

function InsertDialogInner({ tab, initial }: { tab: Tab; initial: FileEntry[] }) {
  const [files, setFiles] = useState(initial);
  const [where, setWhere] = useState<Where>("after");
  const [fit, setFit] = useState<Fit>("doc");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const count = tab.edits.value?.order.length ?? 0;
  const current = Math.min(tab.page.value, Math.max(0, count - 1));
  const docSize = viewSize(tab, current);
  const signed = !!docSigs(tab).value?.some((s) => s.signed);
  const close = () => !busy && (insertDialog.value = null);
  useEscape(close, [busy]);

  const run = async () => {
    const p = problem(files);
    if (p) return setError(p);
    setError("");
    const at = where === "start" ? 0 : where === "end" ? count : where === "before" ? current : current + 1;
    setBusy(true);
    try {
      const r = await P.insertPages(tab, at, { sources: sourcesOf(files), fit: files.some((f) => !f.isPdf) ? fitSize(fit, docSize) : null });
      insertDialog.value = null;
      showToast(`${plural(r.count, "page")} inserted${r.count ? ` as page ${at + 1}${r.count > 1 ? `–${at + r.count}` : ""}` : ""} · Ctrl+Z to undo`, 3500);
    } catch (e) {
      setError(passwordError(String(e), files, setFiles));
    } finally {
      setBusy(false);
    }
  };

  const total = files.reduce((n, f) => n + rowCount(f), 0);

  return (
    <ToolFrame id="insert" title="Insert pages">
      <div class="tool-field">
        <label>From</label>
        <FileList files={files} setFiles={setFiles} ranges reorder={files.length > 1} title="Insert pages from" />
      </div>

      <div class="tool-field">
        <label>Where</label>
        <div class="seg nowrap">
          <button class={`segb ${where === "after" ? "on" : ""}`} onClick={() => setWhere("after")}>
            After page {current + 1}
          </button>
          <button class={`segb ${where === "before" ? "on" : ""}`} onClick={() => setWhere("before")}>
            Before page {current + 1}
          </button>
          <button class={`segb ${where === "start" ? "on" : ""}`} onClick={() => setWhere("start")}>
            At the start
          </button>
          <button class={`segb ${where === "end" ? "on" : ""}`} onClick={() => setWhere("end")}>
            At the end
          </button>
        </div>
      </div>

      {files.some((f) => !f.isPdf) && <FitChoice fit={fit} setFit={setFit} docSize={docSize} />}

      {signed && <div class="tool-note">This document is digitally signed. After saving, its signatures will show that the document changed since it was signed.</div>}
      {error && <span class="error-text">{error}</span>}

      <div class="row">
        <button class="btn" onClick={close}>
          Cancel
        </button>
        <button class="btn pri" disabled={busy || !files.length} onClick={() => void run()}>
          {busy ? "Inserting…" : total ? `Insert ${plural(total, "page")}` : "Insert"}
        </button>
      </div>
    </ToolFrame>
  );
}

// ---------------------------------------------------------------- merge

/** Merge: files (starting with this document, if one is open) into a new PDF. */
export const mergeDialog = signal<{ from: Tab | null; files: FileEntry[] } | null>(null);
toolPane("merge", mergeDialog);

/** Pictures into one PDF (Convert › To PDF › Images): the Merge dialog, starting with pictures picked. */
export async function startMergePictures() {
  const res = testHook()?.pick ?? (await open({ multiple: true, title: "Pictures to put in one PDF", filters: [{ name: "Pictures", extensions: [...PICTURES] }] }));
  const files = await peekAll(Array.isArray(res) ? res : res ? [res] : []);
  if (files.length) mergeDialog.value = { from: null, files };
}

export async function startMerge(from: Tab | null) {
  const files = from?.info.value ? await peekAll([from.path]) : await pickFiles("Merge files");
  if (files.length) mergeDialog.value = { from, files };
}

export function MergeDialog() {
  const st = mergeDialog.value;
  return st ? <MergeDialogInner key={st.files.map((f) => f.key).join(",")} from={st.from} initial={st.files} /> : null;
}

function MergeDialogInner({ from, initial }: { from: Tab | null; initial: FileEntry[] }) {
  const [files, setFiles] = useState(initial);
  const [bookmarks, setBookmarks] = useState(true);
  const [fit, setFit] = useState<Fit>("A4");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const close = () => !busy && (mergeDialog.value = null);
  useEscape(close, [busy]);
  // This document's unsaved changes aren't in its file.
  const unsaved = from && from.dirty.value && files.some((f) => samePath(f.path, from.path)) ? from : null;
  const total = files.reduce((n, f) => n + rowCount(f), 0);

  const run = async () => {
    const p = problem(files);
    if (p) return setError(p);
    setError("");
    const first = files[0].path;
    const stem = baseName(first).replace(/\.[^.]+$/, "");
    const target =
      testHook()?.save ??
      (await saveDialog({
        title: "Save the merged PDF as",
        defaultPath: first.replace(/[^\\/]+$/, `${stem}${files.length > 1 ? " (merged)" : " (copy)"}.pdf`),
        filters: [{ name: "PDF", extensions: ["pdf"] }],
      }));
    if (!target) return;
    if (tabs.value.some((t) => samePath(t.path, target))) {
      return setError(`${baseName(target)} is open in Paginaut. Close it first, or choose another name.`);
    }
    setBusy(true);
    const t0 = performance.now();
    try {
      const n = await api.mergeFiles(sourcesOf(files), files.some((f) => !f.isPdf) ? fitSize(fit, null) : null, bookmarks, target);
      mergeDialog.value = null;
      await openFiles([target]);
      showToast(`Merged ${plural(files.length, "file")} into ${baseName(target)} · ${plural(n, "page")} · ${Math.round(performance.now() - t0)} ms`, 4000);
    } catch (e) {
      setError(passwordError(String(e), files, setFiles));
    } finally {
      setBusy(false);
    }
  };

  return (
    <ToolFrame id="merge" title="Merge files">
      <span class="muted">Combines files into one new PDF, in this order. Drag or use the arrows to change it; type pages to take only some.</span>

      <div class="tool-field">
        <label>Files</label>
        <FileList files={files} setFiles={setFiles} ranges reorder title="Merge files" />
      </div>

      <label class="check-row">
        <input type="checkbox" checked={bookmarks} onChange={(e) => setBookmarks((e.target as HTMLInputElement).checked)} />
        A bookmark for each file, with its own bookmarks inside
      </label>

      {files.some((f) => !f.isPdf) && <FitChoice fit={fit} setFit={setFit} docSize={null} />}

      {unsaved && (
        <div class="tool-note warn">
          {baseName(unsaved.path)} has unsaved changes, which won't be included.{" "}
          <button class="btn ghost sm" onClick={() => void P.save(unsaved)}>
            Save it now
          </button>
        </div>
      )}
      {error && <span class="error-text">{error}</span>}

      <div class="row">
        <button class="btn" onClick={close}>
          Cancel
        </button>
        <button class="btn pri" disabled={busy || files.length === 0} onClick={() => void run()}>
          {busy ? "Merging…" : total ? `Merge ${plural(total, "page")}…` : "Merge…"}
        </button>
      </div>
    </ToolFrame>
  );
}

// ---------------------------------------------------------------- extract

/** "1-3, 7" for display indices (0-based in, 1-based out). */
function rangesText(views: number[]): string {
  const out: string[] = [];
  for (let i = 0; i < views.length; ) {
    let j = i;
    while (j + 1 < views.length && views[j + 1] === views[j] + 1) j++;
    out.push(i === j ? `${views[i] + 1}` : `${views[i] + 1}-${views[j] + 1}`);
    i = j + 1;
  }
  return out.join(", ");
}

const isOpen = (path: string) => tabs.value.some((t) => samePath(t.path, path));

/** Saves the selected pages (or the current page) as a new PDF and opens it. */
export async function extractPages(tab: Tab) {
  const e = tab.edits.value;
  const info = tab.info.value;
  if (!e || !info) return;
  const views = P.targets(tab);
  const stem = baseName(tab.path).replace(/\.[^.]+$/, "");
  const which = rangesText(views);
  const word = views.length === 1 ? "page" : "pages";
  const target =
    testHook()?.save ??
    (await saveDialog({
      title: `Save ${word} ${which} as`,
      defaultPath: tab.path.replace(/[^\\/]+$/, `${stem} (${word} ${which.replace(/, /g, ",")}).pdf`),
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    }));
  if (!target) return;
  if (isOpen(target)) return showToast(`${baseName(target)} is open in Paginaut. Close it first, or choose another name.`, 5000);
  try {
    await api.saveParts(info.docId, [{ pages: views.map((v) => e.order[v]), path: target }], e.rot);
    await openFiles([target]);
    showToast(`Saved ${plural(views.length, "page")} as ${baseName(target)}`);
  } catch (err) {
    showToast(`Couldn't save the pages: ${err}`, 5000);
  }
}

// ---------------------------------------------------------------- split

export const splitDialog = signal<Tab | null>(null);
toolPane("split", splitDialog, { tabOf: (v) => v });

export function startSplit(tab: Tab) {
  if (tab.edits.value && tab.edits.value.order.length < 2) return showToast("There's only one page.");
  splitDialog.value = tab;
}

export function SplitDialog() {
  const tab = splitDialog.value;
  return tab ? <SplitDialogInner key={tab.key} tab={tab} /> : null;
}

type SplitBy = "every" | "ranges" | "bookmarks" | "size";

/** A file name without characters Windows doesn't allow. */
const safeName = (s: string) => s.replace(/[\\/:*?"<>|\u0000-\u001f]+/g, " ").replace(/\s+/g, " ").trim().slice(0, 80) || "part";

function SplitDialogInner({ tab }: { tab: Tab }) {
  const e = tab.edits.value!;
  const count = e.order.length;
  const isPdf = !!tab.info.value?.isPdf;
  const stem = baseName(tab.path).replace(/\.[^.]+$/, "");
  const top = tab.outline.value ?? [];
  const [by, setBy] = useState<SplitBy>("every");
  const [every, setEvery] = useState("1");
  const [ranges, setRanges] = useState("");
  const [mb, setMb] = useState("10");
  const [folder, setFolder] = useState(tab.path.replace(/[\\/][^\\/]+$/, ""));
  const [sized, setSized] = useState<number[][] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [replace, setReplace] = useState<string[] | null>(null);
  const close = () => !busy && (splitDialog.value = null);
  useEscape(close, [busy]);
  const change = (f: () => void) => {
    f();
    setReplace(null);
    setError("");
  };

  // Bookmarks: the top-level ones, by where their page is shown now.
  const marks = (() => {
    const at = new Map<number, number>();
    e.order.forEach((src, v) => !at.has(src) && at.set(src, v));
    const list = top.map((o) => ({ title: o.t, view: at.get(o.p) })).filter((m): m is { title: string; view: number } => m.view !== undefined);
    list.sort((a, b) => a.view - b.view);
    return list.filter((m, i) => i === 0 || m.view !== list[i - 1].view);
  })();

  // By size: the engine groups pages, counting shared fonts and pictures once per file.
  useEffect(() => {
    if (by !== "size") return;
    setSized(null);
    const bytes = parseFloat(mb) * 1e6;
    if (!(bytes > 0)) return;
    const t = setTimeout(() => {
      api.splitBySize(tab.info.value!.docId, e.order, bytes).then(setSized).catch((err) => setError(String(err)));
    }, 250);
    return () => clearTimeout(t);
  }, [by, mb]);

  /** Files as display indices with a name each, or what's missing. */
  const plan = ((): { views: number[]; name: string }[] | string => {
    const pad = (i: number, n: number) => String(i + 1).padStart(String(n).length, "0");
    const numbered = (groups: number[][]) => groups.map((views, i) => ({ views, name: `${stem} - ${pad(i, groups.length)}` }));
    if (by === "every") {
      const n = parseInt(every, 10);
      if (!(n >= 1)) return "Type how many pages each file gets.";
      const groups: number[][] = [];
      for (let i = 0; i < count; i += n) groups.push(Array.from({ length: Math.min(n, count - i) }, (_, k) => i + k));
      return numbered(groups);
    }
    if (by === "ranges") {
      const parts = ranges.split(/[,;]+/).map((s) => s.trim()).filter(Boolean);
      if (!parts.length) return `Type a range for each file, like 1-3, 4-10, 11- (of ${count}).`;
      const groups: number[][] = [];
      for (const p of parts) {
        const views = parsePages(p, count);
        if (!views) return `"${p}" isn't a range of pages 1 to ${count}.`;
        groups.push(views);
      }
      return numbered(groups);
    }
    if (by === "bookmarks") {
      if (!marks.length) return "This document has no bookmarks.";
      const used = new Map<string, number>();
      return marks.map((m, i) => {
        // Pages before the first bookmark go with it.
        const from = i === 0 ? 0 : m.view;
        const to = i + 1 < marks.length ? marks[i + 1].view : count;
        const base = `${stem} - ${safeName(m.title)}`;
        const n = (used.get(base) ?? 0) + 1;
        used.set(base, n);
        return { views: Array.from({ length: to - from }, (_, k) => from + k), name: n > 1 ? `${base} (${n})` : base };
      });
    }
    if (!sized) return "Working out the files…";
    const viewOf = new Map<number, number>();
    e.order.forEach((src, v) => viewOf.set(src, v));
    return numbered(sized.map((g) => g.map((src) => viewOf.get(src)!)));
  })();

  const sep = folder.includes("/") && !folder.includes("\\") ? "/" : "\\";
  const pathOf = (name: string) => `${folder}${sep}${name}.pdf`;

  const chooseFolder = async () => {
    const f = await open({ directory: true, title: "Save the files in", defaultPath: folder });
    if (typeof f === "string") change(() => setFolder(f));
  };

  const run = async () => {
    if (typeof plan === "string") return setError(plan);
    setError("");
    const paths = plan.map((p) => pathOf(p.name));
    const clash = paths.find(isOpen);
    if (clash) return setError(`${baseName(clash)} is open in Paginaut. Close it first, or choose another folder.`);
    if (!replace) {
      const existing = await api.existingFiles(paths);
      if (existing.length) return setReplace(existing);
    }
    setBusy(true);
    const t0 = performance.now();
    try {
      await api.saveParts(tab.info.value!.docId, plan.map((p, i) => ({ pages: p.views.map((v) => e.order[v]), path: paths[i] })), e.rot);
      splitDialog.value = null;
      showToast(`Split into ${plural(plan.length, "file")} in ${baseName(folder)} · ${Math.round(performance.now() - t0)} ms`, 4000);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const shown = typeof plan === "string" ? [] : plan.slice(0, 6);
  return (
    <ToolFrame id="split" title="Split into files">
      <span class="muted">Saves parts of this document as separate PDFs. The document itself stays as it is.</span>

      <div class="tool-field">
        <label>Split</label>
        <div class="seg nowrap">
          <button class={`segb ${by === "every" ? "on" : ""}`} onClick={() => change(() => setBy("every"))}>
            Every few pages
          </button>
          <button class={`segb ${by === "ranges" ? "on" : ""}`} onClick={() => change(() => setBy("ranges"))}>
            By page ranges
          </button>
          <button
            class={`segb ${by === "bookmarks" ? "on" : ""}`}
            disabled={!marks.length}
            title={marks.length ? undefined : "This document has no bookmarks"}
            onClick={() => change(() => setBy("bookmarks"))}
          >
            By bookmarks
          </button>
          <button class={`segb ${by === "size" ? "on" : ""}`} disabled={!isPdf} onClick={() => change(() => setBy("size"))}>
            By file size
          </button>
        </div>
        {by === "every" && (
          <div class="ins-file-row">
            <span>A new file every</span>
            <input class="inp sm num" type="number" min="1" max={count} value={every} onInput={(ev) => change(() => setEvery((ev.target as HTMLInputElement).value))} />
            <span>{parseInt(every, 10) === 1 ? "page" : "pages"}</span>
          </div>
        )}
        {by === "ranges" && (
          <input
            class="inp sm"
            placeholder={`One range per file, like 1-3, 4-10, 11- (of ${count})`}
            value={ranges}
            autoFocus
            onInput={(ev) => change(() => setRanges((ev.target as HTMLInputElement).value))}
          />
        )}
        {by === "bookmarks" && <span class="hint">A file for each of the {marks.length} top-level bookmarks, named after it.</span>}
        {by === "size" && (
          <div class="ins-file-row">
            <span>Files of at most about</span>
            <input class="inp sm num" type="number" min="0.1" step="0.5" value={mb} onInput={(ev) => change(() => setMb((ev.target as HTMLInputElement).value))} />
            <span>MB</span>
          </div>
        )}
      </div>

      <div class="tool-field">
        <label>Save in</label>
        <div class="ins-file-row">
          <span class="ins-name" title={folder}>
            {folder}
          </span>
          <button class="btn ghost sm" onClick={() => void chooseFolder()}>
            Change…
          </button>
        </div>
      </div>

      {typeof plan === "string" ? (
        <span class="hint">{plan}</span>
      ) : (
        <div class="split-plan">
          {shown.map((p) => (
            <div key={p.name} class="split-row">
              <span class="ins-name">{p.name}.pdf</span>
              <span class="muted">{p.views.length === 1 ? `page ${p.views[0] + 1}` : `pages ${rangesText(p.views)}`}</span>
            </div>
          ))}
          {plan.length > shown.length && <span class="muted">… and {plural(plan.length - shown.length, "more file")}</span>}
        </div>
      )}
      {replace && (
        <div class="tool-note warn">
          {replace.length === 1 ? `${baseName(replace[0])} already exists.` : `${replace.length} of these files already exist.`} Split again to replace{" "}
          {replace.length === 1 ? "it" : "them"}.
        </div>
      )}
      {error && <span class="error-text">{error}</span>}

      <div class="row">
        <button class="btn" onClick={close}>
          Cancel
        </button>
        <button class="btn pri" disabled={busy || typeof plan === "string"} onClick={() => void run()}>
          {busy ? "Splitting…" : typeof plan === "string" ? "Split" : `${replace ? "Replace and split" : "Split"} into ${plural(plan.length, "file")}`}
        </button>
      </div>
    </ToolFrame>
  );
}
