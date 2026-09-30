// The Organize pane (right side): what used to be the long Organize menu, in groups. Pages (grid, insert, merge,
// split, crop), marks on every page, the document (properties, protection, size, repair), and getting content out.
import type { ComponentChild } from "preact";
import {
  Combine, Crop, FileArchive, FileInput, FileLock2, FilePlus2, FileText, Hash, Image, ImagePlus, Info, LayoutGrid, Paperclip, Scissors, SquareDashed,
  Stamp, Table, Tags, Wrench, X,
} from "lucide-preact";
import { attachFiles, openPageTable, picturesDialog, showAttachments, startArea } from "../extract";
import { bookScreen } from "../modes";
import { startPageSize } from "../pagesize";
import { closePane, pane } from "../panes";
import { activeTab, type Tab } from "../store";
import { addPicture } from "../textedit";
import { startLabels, startProperties } from "./MetaUI";
import { toggleGrid } from "./PageGrid";
import { extractPages, insertBlankPage, startInsert, startMerge, startSplit } from "./PagesUI";
import { startSecure } from "./SecureUI";
import { startHeaders, startWatermark } from "./StampUI";

function Row(p: { icon: ComponentChild; label: string; hint?: string; on?: boolean; disabled?: boolean; run: () => void }) {
  return (
    <button class={`pane-row ${p.on ? "on" : ""}`} disabled={p.disabled} onClick={p.run}>
      {p.icon}
      <span>{p.label}</span>
      {p.hint && <small>{p.hint}</small>}
    </button>
  );
}

const ic = (C: typeof X) => <C size={16} strokeWidth={1.7} />;

export function OrganizePane() {
  if (pane.value !== "organize" || bookScreen.value) return null;
  const tab = activeTab.value;
  const ready: Tab | null = tab?.status.value === "ready" ? tab : null;
  const pdf = !!ready?.info.value?.isPdf;
  const files = ready?.attachments.value?.length ?? 0;
  const run = (f: (t: Tab) => unknown) => () => ready && void f(ready);
  return (
    <aside class="signpane organize" aria-label="Organize">
      <div class="signpane-head">
        <strong>Organize</strong>
        <button class="ib sm" aria-label="Close the Organize pane" title="Close" onClick={() => closePane("organize")}>
          <X size={15} />
        </button>
      </div>
      <div class="signpane-body">
        {!ready && <p class="muted small pane-note">Open a document to work on its pages. Merging files works without one.</p>}
        <section class="pane-sec">
          <h3>Pages</h3>
          <Row icon={ic(LayoutGrid)} label="Page grid" hint="Ctrl+Shift+G" on={!!ready?.grid.value} disabled={!ready} run={run((t) => toggleGrid(t))} />
          <Row icon={ic(FileInput)} label="Insert pages from a file…" disabled={!pdf} run={run(startInsert)} />
          <Row icon={ic(FilePlus2)} label="Insert a blank page" disabled={!pdf} run={run(insertBlankPage)} />
          <Row icon={ic(ImagePlus)} label="Add a picture…" disabled={!pdf} run={run(addPicture)} />
          <Row icon={ic(Combine)} label="Merge files…" run={() => void startMerge(ready)} />
          <Row icon={ic(Scissors)} label="Split into files…" disabled={!ready} run={run(startSplit)} />
          <Row icon={ic(FileText)} label="Save selected pages as a new PDF…" disabled={!ready} run={run(extractPages)} />
          <Row icon={ic(Crop)} label="Crop and resize pages…" disabled={!pdf} run={run(startPageSize)} />
        </section>
        <section class="pane-sec">
          <h3>On every page</h3>
          <Row icon={ic(Hash)} label="Page numbers, headers and footers…" disabled={!pdf} run={run(startHeaders)} />
          <Row icon={ic(Stamp)} label="Watermark…" disabled={!pdf} run={run(startWatermark)} />
          <Row icon={ic(Tags)} label="Page labels…" disabled={!pdf} run={run(startLabels)} />
        </section>
        <section class="pane-sec">
          <h3>Document</h3>
          <Row icon={ic(Info)} label="Document properties…" hint="Ctrl+D" disabled={!ready} run={run(startProperties)} />
          <Row icon={ic(FileLock2)} label="Protect with a password…" disabled={!pdf} run={run((t) => startSecure(t, "protect"))} />
          <Row icon={ic(FileArchive)} label="Reduce file size…" disabled={!pdf} run={run((t) => startSecure(t, "compress"))} />
          <Row icon={ic(Wrench)} label="Repair…" disabled={!pdf} run={run((t) => startSecure(t, "repair"))} />
        </section>
        <section class="pane-sec">
          <h3>Get content out</h3>
          <Row icon={ic(Image)} label="Save all pictures…" disabled={!ready} run={run((t) => (picturesDialog.value = t))} />
          <Row icon={ic(SquareDashed)} label="Select an area to copy or save…" hint="or right-click a page" disabled={!ready} run={run(startArea)} />
          <Row icon={ic(Table)} label="Table on this page to Excel / CSV…" disabled={!ready} run={run(openPageTable)} />
          <Row icon={ic(Paperclip)} label={files ? `Attached files (${files})` : "Attached files"} disabled={!files} run={showAttachments} />
          <Row icon={ic(Paperclip)} label="Attach a file…" disabled={!pdf} run={run(attachFiles)} />
        </section>
      </div>
    </aside>
  );
}
