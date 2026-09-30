// "Save all pictures" and the Attachments panel.
import { open, save } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "preact/hooks";
import { FileDown, FolderDown, Paperclip, Plus, Trash2, X } from "lucide-preact";
import { api, type Attachment } from "../api";
import { attachFiles, picturesDialog, removeAttachment } from "../extract";
import { pagesOf } from "../stamps";
import { baseName, showToast, type Tab } from "../store";
import { testHook } from "./PagesUI";

const size = (b: number) => (b >= 1 << 20 ? `${(b / (1 << 20)).toFixed(1)} MB` : b >= 1024 ? `${Math.round(b / 1024)} KB` : `${b} bytes`);

async function pickFolder(title: string): Promise<string | null> {
  const hook = testHook();
  if (hook?.pick) return hook.pick[0] ?? null;
  const r = await open({ directory: true, title });
  return typeof r === "string" ? r : null;
}

// ---------------------------------------------------------------- save all pictures

export function PicturesDialog() {
  const tab = picturesDialog.value;
  return tab ? <PicturesInner key={tab.key} tab={tab} /> : null;
}

function PicturesInner({ tab }: { tab: Tab }) {
  const selected = [...tab.pageSel.value].sort((a, b) => a - b);
  const [which, setWhich] = useState<"all" | "sel" | "range">(selected.length > 1 ? "sel" : "all");
  const [range, setRange] = useState("");
  const [min, setMin] = useState(100);
  const [png, setPng] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const count = tab.edits.value?.order.length ?? 0;
  const close = () => !busy && (picturesDialog.value = null);
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        close();
      }
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [busy]);

  const go = async () => {
    const views = which === "all" ? pagesOf(tab, null) : which === "sel" ? selected : pagesOf(tab, range);
    if (!views?.length) return setError(`Pages must be numbers from 1 to ${count}, like 2- or 1-5, 8.`);
    const dir = await pickFolder("Save the pictures in");
    if (!dir) return;
    setError("");
    setBusy(true);
    try {
      const order = tab.edits.value!.order;
      const srcs = views.map((v) => order[v]);
      const names = views.map((v) => tab.labels.value?.[order[v]] || String(v + 1));
      const stem = baseName(tab.path).replace(/\.[^.]+$/, "");
      const r = await api.savePictures(tab.info.value!.docId, srcs, names, min, png ? 1 : 0, dir, stem);
      picturesDialog.value = null;
      const notes = [r.repeats ? `${r.repeats} repeats left out` : "", r.small ? `${r.small} small ones skipped` : ""].filter(Boolean).join(", ");
      showToast(
        r.saved ? `Saved ${r.saved} ${r.saved === 1 ? "picture" : "pictures"} in ${baseName(dir)}${notes ? ` (${notes})` : ""}` : `No pictures to save${notes ? ` (${notes})` : ""}.`,
        6000,
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div class="overlay" onPointerDown={(e) => e.target === e.currentTarget && close()}>
      <div class="dialog tool-dialog" role="dialog" aria-label="Save all pictures">
        <button class="ib sm close" aria-label="Close" onClick={close}>
          <X size={16} />
        </button>
        <h2>Save all pictures</h2>
        <div class="tool-field">
          <label>Pages</label>
          <div class="ps-line">
            <div class="seg nowrap">
              <button class={`segb ${which === "all" ? "on" : ""}`} onClick={() => setWhich("all")}>
                All ({count})
              </button>
              {selected.length > 0 && (
                <button class={`segb ${which === "sel" ? "on" : ""}`} onClick={() => setWhich("sel")}>
                  Selected ({selected.length})
                </button>
              )}
              <button class={`segb ${which === "range" ? "on" : ""}`} onClick={() => setWhich("range")}>
                Pages…
              </button>
            </div>
            {which === "range" && <input class="inp sm" placeholder="e.g. 2- or 1-5, 8" value={range} autoFocus onInput={(e) => setRange((e.target as HTMLInputElement).value)} />}
          </div>
        </div>
        <div class="tool-field">
          <label>Leave out</label>
          <div class="ps-line">
            <label class="check-row">
              Pictures smaller than
              <input class="inp sm num" type="number" min="0" step="10" value={min} onChange={(e) => setMin(Math.max(0, Math.round(+(e.target as HTMLInputElement).value || 0)))} />
              pixels (icons, lines, specks)
            </label>
          </div>
        </div>
        <div class="tool-field">
          <label>Format</label>
          <div class="seg nowrap">
            <button class={`segb ${!png ? "on" : ""}`} onClick={() => setPng(false)}>
              As stored
            </button>
            <button class={`segb ${png ? "on" : ""}`} onClick={() => setPng(true)}>
              All as PNG
            </button>
          </div>
          <span class="hint">
            {png ? "Every picture as PNG, with its transparency; photos make large files." : "Photos stay the JPEG files they are (full quality, nothing re-compressed); others become PNG."}
          </span>
        </div>
        <span class="hint">A picture used more than once is saved once.</span>
        {error && <span class="error-text">{error}</span>}
        <div class="row">
          <span style={{ flex: 1 }} />
          <button class="btn" onClick={close}>
            Cancel
          </button>
          <button class="btn pri" disabled={busy} onClick={() => void go()}>
            {busy ? "Saving…" : "Choose a folder and save"}
          </button>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- attachments panel

export function Attachments({ tab }: { tab: Tab }) {
  const list = tab.attachments.value;
  const docId = tab.info.value!.docId;
  if (!list) return <div class="empty-note">Loading…</div>;
  if (!list.length) return <div class="empty-note">This document has no attached files.</div>;

  const openOne = (a: Attachment) => api.attachmentOpen(docId, a.i).catch((e) => showToast(String(e), 7000));
  const saveOne = async (a: Attachment) => {
    const ext = a.name.includes(".") ? a.name.split(".").pop()! : "";
    const dir = tab.path.replace(/[^\\/]+$/, "");
    const path = testHook()?.save ?? (await save({ title: "Save the attachment", defaultPath: dir + a.name, filters: ext ? [{ name: ext.toUpperCase(), extensions: [ext] }] : [] }));
    if (!path) return;
    api.attachmentSave(docId, a.i, path, null).then((p) => showToast(`Saved ${baseName(p)}`), (e) => showToast(`Couldn't save: ${e}`, 5000));
  };
  const saveAll = async () => {
    const dir = await pickFolder("Save the attachments in");
    if (!dir) return;
    try {
      for (const a of list) await api.attachmentSave(docId, a.i, null, dir);
      showToast(`Saved ${list.length} ${list.length === 1 ? "file" : "files"} in ${baseName(dir)}`);
    } catch (e) {
      showToast(`Couldn't save: ${e}`, 5000);
    }
  };

  return (
    <div class="side-scroll attach-list">
      <div class="outline-bar">
        <span class="muted">
          {list.length} attached {list.length === 1 ? "file" : "files"}
        </span>
        <span style={{ flex: 1 }} />
        {tab.info.value?.isPdf && (
          <button class="ib sm" onClick={() => void attachFiles(tab)} title="Attach files to the document" aria-label="Attach files">
            <Plus size={15} />
          </button>
        )}
        <button class="btn ghost sm" onClick={() => void saveAll()} title="Save every attached file in a folder">
          <FolderDown size={14} /> Save all
        </button>
      </div>
      {list.map((a) => (
        <div key={a.i} class="attach-row" onDblClick={() => void openOne(a)} title={`${a.name}\nDouble-click to open`}>
          <Paperclip size={15} />
          <div class="attach-main">
            <span class="attach-name">{a.name || "Unnamed file"}</span>
            <span class="attach-meta">
              {[a.size > 0 ? size(a.size) : "", a.page >= 0 ? `page ${(tab.edits.value?.order.indexOf(a.page) ?? a.page) + 1}` : "", a.desc].filter(Boolean).join(" · ")}
            </span>
          </div>
          <button class="ib sm" title="Save…" aria-label={`Save ${a.name}`} onClick={() => void saveOne(a)}>
            <FileDown size={15} />
          </button>
          <button class="ib sm" title={a.page >= 0 ? "Remove (takes its pin off the page)" : "Remove from the document"} aria-label={`Remove ${a.name}`} onClick={() => void removeAttachment(tab, a.i, a.name, a.page >= 0)}>
            <Trash2 size={15} />
          </button>
        </div>
      ))}
      <span class="hint attach-hint">Double-click to open. Programs and scripts are never opened, only saved.</span>
    </div>
  );
}
