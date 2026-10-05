// Settings › Storage: the copies kept of files other apps lent (each can be removed), and what the
// app remembers about the files you've read.
import { useEffect, useState } from "preact/hooks";
import { ArrowLeft, FileText, Image, BookOpen, Trash2 } from "lucide-preact";
import { back } from "./nav";
import { CACHE_LIMIT, copyState, dropCopy, forgetAll, records, refreshStorage, storage, tidyCache } from "./records";
import { say } from "./state";
import { native } from "./native";
import { ShelfAccess } from "./ShelfView";
import { SortToggle, type SortState } from "./SortToggle";

export function sizeText(n: number) {
  if (n < 1024 * 1024) return `${Math.max(0.1, n / 1024 / 1024).toFixed(1)} MB`;
  const mb = n / 1024 / 1024;
  return `${mb < 10 ? mb.toFixed(1) : Math.round(mb)} MB`;
}

function ago(t: number) {
  const d = Math.floor((Date.now() - t) / 86_400_000);
  return d <= 0 ? "today" : d === 1 ? "yesterday" : `${d} d ago`;
}

const iconFor = (name: string) => {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (["epub", "mobi", "fb2", "fbz"].includes(ext)) return BookOpen;
  if (["cbz", "cbr", "cbt", "png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff"].includes(ext)) return Image;
  return FileText;
};

type Sort = "size" | "date" | "name";

export function StorageSheet() {
  const [sort, setSort] = useState<SortState<Sort>>({ key: "size", desc: true });
  const [confirm, setConfirm] = useState<"copies" | "records" | null>(null);
  useEffect(() => void refreshStorage(), []);
  const st = storage.value;
  const recs = records.value;
  const byCopy = new Map(recs.filter((r) => r.copy).map((r) => [r.copy, r]));
  const copies = (st?.copies ?? []).map((c) => {
    const r = byCopy.get(c.path);
    return { ...c, r, at: r?.at ?? c.at, name: r?.name ?? c.name };
  });
  const sign = sort.desc ? -1 : 1;
  copies.sort((a, b) => sign * (sort.key === "size" ? a.size - b.size : sort.key === "date" ? a.at - b.at : a.name.localeCompare(b.name)));
  const total = copies.reduce((n, c) => n + c.size, 0);

  return (
    <>
      <div class="sheet-title">
        <button class="ib" aria-label="Back to settings" onClick={() => back()}>
          <ArrowLeft size={22} />
        </button>
        <h3>Storage</h3>
      </div>
      <div class="sheet-scroll">
        <div class="store-head">
          <span>Copies of files from other apps</span>
          <span class="store-num">
            {sizeText(total)} of {sizeText(CACHE_LIMIT)}
          </span>
        </div>
        <div class="store-bar">
          <span style={{ width: `${Math.min(100, (total / CACHE_LIMIT) * 100)}%` }} />
        </div>
        <p class="hint store-why">
          Apps like WhatsApp, mail and Files lend Paginaut a file only while it's open. So that Recent can open it again, a copy is kept in Paginaut's cache, here on this phone (nothing is
          sent anywhere). Copies of short files are removed after a week; when the cache is full, the biggest files you aren't reading go first.
        </p>
        {copies.length > 0 && (
          <SortToggle<Sort>
            value={sort}
            onChange={setSort}
            order={[
              ["name", "Name", false],
              ["date", "Date", true],
              ["size", "Size", true],
            ]}
          />
        )}
        {copies.length === 0 ? (
          <p class="hint">No copies kept. Files picked in Paginaut are read where they are.</p>
        ) : (
          <ul class="store-list">
            {copies.map((c) => {
              const Icon = iconFor(c.name);
              const s = copyState(c.r);
              return (
                <li key={c.path}>
                  <Icon size={20} class="store-ic" />
                  <span class="store-tx">
                    <span class="store-name">{c.name.replace(/\.[^.]+$/, "")}</span>
                    <span class="store-meta">
                      {sizeText(c.size)} · {ago(c.at)}
                      {s.label && <span class={`store-badge ${s.tone}`}>{s.label}</span>}
                    </span>
                  </span>
                  <button
                    class="ib sm"
                    aria-label={`Remove the copy of ${c.name}`}
                    onClick={() => void dropCopy(c.path).then(() => say(`Removed the copy of ${c.name}. Where you were in it is kept.`))}
                  >
                    <Trash2 size={18} />
                  </button>
                </li>
              );
            })}
          </ul>
        )}
        {copies.length > 0 && (
          <button
            class="row link-row danger"
            onClick={async () => {
              if (confirm !== "copies") return setConfirm("copies");
              setConfirm(null);
              await native.dropCopies(copies.map((c) => c.path));
              await tidyCache();
              say("Removed every copy. Where you were in each file is kept.");
            }}
          >
            <span class="row-label">{confirm === "copies" ? `Tap again to remove ${copies.length} copies (${sizeText(total)})` : "Remove every copy"}</span>
          </button>
        )}

        <div class="store-head store-sep">
          <span>Reading positions and bookmarks</span>
          <span class="store-num">{recs.length} files</span>
        </div>
        <p class="hint store-why">
          Where you were in each file and its bookmarks, kept in Paginaut (not in the files) and forgotten a year after a file was last opened. Forgetting them also empties Recent.
        </p>
        {recs.length > 0 && (
          <button
            class="row link-row danger"
            onClick={() => {
              if (confirm !== "records") return setConfirm("records");
              setConfirm(null);
              forgetAll();
              say("Forgot every reading position and bookmark.");
            }}
          >
            <span class="row-label">{confirm === "records" ? `Tap again to forget ${recs.length} files` : "Forget positions and bookmarks"}</span>
          </button>
        )}
        <div class="store-sep" />
        <ShelfAccess />
      </div>
    </>
  );
}
