// The shelf on the Home page: the e-books (or comics) in the chosen folders as covers, in three
// parts: being read, the rest, and finished. Search, sort, and the folders looked in.
import { useEffect, useState } from "preact/hooks";
import { FolderPlus, RefreshCw, Search, X } from "lucide-preact";
import { coverUrl } from "../api";
import { storeItem } from "../store";
import { SortToggle, type SortState } from "../mobile/SortToggle";
import { addFolder, defaultFolders, metas, openBook, removeFolder, scanIfStale, scanning, scanShelf, sections, shelf, shelfFolders, type ShelfKind, type ShelfSort, type Shelved } from "../shelf";

const SORT_KEY = "paginaut.shelf.sort";
function loadSort(): SortState<ShelfSort> {
  try {
    return { key: "title", desc: false, ...JSON.parse(localStorage.getItem(SORT_KEY) ?? "{}") };
  } catch {
    return { key: "title", desc: false };
  }
}

export function ShelfPane({ kind }: { kind: ShelfKind }) {
  const [sort, setSortState] = useState(loadSort);
  const [find, setFind] = useState("");
  const setSort = (s: SortState<ShelfSort>) => {
    setSortState(s);
    storeItem(SORT_KEY, JSON.stringify(s));
  };
  useEffect(() => void scanIfStale(), []);
  // Read for the sections below to follow new covers and scans.
  void metas.value;
  void shelf.value;
  const { reading, rest, finished } = sections(kind, sort, find);
  const none = reading.length + rest.length + finished.length === 0;
  const word = kind === "ebook" ? "e-books" : "comics";
  return (
    <div class="shelf">
      <div class="shelf-bar">
        <label class="shelf-find">
          <Search size={15} />
          <input value={find} onInput={(e) => setFind(e.currentTarget.value)} placeholder={`Search ${word}`} aria-label={`Search ${word}`} />
          {find && (
            <button class="ib sm" aria-label="Clear search" onClick={() => setFind("")}>
              <X size={14} />
            </button>
          )}
        </label>
        <SortToggle<ShelfSort>
          value={sort}
          onChange={setSort}
          order={[
            ["title", "Title", false],
            ["author", "Author", false],
            ["added", "Added", true],
          ]}
        />
        <button class="btn ghost" onClick={() => void scanShelf(true)} disabled={scanning.value} title="Look through the folders again">
          <RefreshCw size={15} class={scanning.value ? "turning" : ""} /> Look again
        </button>
      </div>
      <Where />
      {none ? (
        <div class="shelf-empty">
          {scanning.value ? `Looking through your folders for ${word}…` : find ? `No ${word} match “${find}”.` : `No ${word} in these folders yet. Add the folder where you keep them.`}
        </div>
      ) : (
        <>
          {reading.length > 0 && <Grid title="Continue reading" books={reading} />}
          {rest.length > 0 && <Grid title={reading.length || finished.length ? `More ${word}` : `All ${word}`} books={rest} />}
          {finished.length > 0 && <Grid title="Finished" books={finished} />}
        </>
      )}
    </div>
  );
}

/** The folders looked in, each removable, and one more to add. */
function Where() {
  const [defaults, setDefaults] = useState<string[]>([]);
  const [open, setOpen] = useState(false);
  useEffect(() => void defaultFolders().then(setDefaults), []);
  const folders = shelfFolders.value ?? defaults;
  const short = (f: string) => f.split(/[\\/]/).filter(Boolean).pop() ?? f;
  return (
    <div class="shelf-where">
      <button class="shelf-where-sum" aria-expanded={open} onClick={() => setOpen(!open)}>
        Looking in {folders.length ? folders.map(short).join(", ") : "no folders"} · <span class="lnk">{open ? "Done" : "Change folders"}</span>
      </button>
      {open && (
        <div class="shelf-folders">
          {folders.map((f) => (
            <div class="shelf-folder" key={f}>
              <span title={f}>{f}</span>
              <button class="ib sm" aria-label={`Stop looking in ${short(f)}`} title="Stop looking here" onClick={() => void removeFolder(f)}>
                <X size={14} />
              </button>
            </div>
          ))}
          <button class="btn ghost" onClick={() => void addFolder()}>
            <FolderPlus size={15} /> Add folder…
          </button>
        </div>
      )}
    </div>
  );
}

function Grid({ title, books }: { title: string; books: Shelved[] }) {
  return (
    <section class="shelf-sec">
      <h3>
        {title} <span class="n">{books.length}</span>
      </h3>
      <div class="shelf-grid">
        {books.map((b) => (
          <Tile key={b.it.path} b={b} />
        ))}
      </div>
    </section>
  );
}

/** A plain cover's colour, the same for a book every time. */
function hue(s: string) {
  let h = 0;
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

function Tile({ b }: { b: Shelved }) {
  const m = b.it.fp ? metas.value[b.it.fp] : undefined;
  const ext = b.it.name.split(".").pop()?.toUpperCase() ?? "";
  return (
    <button class="book" onClick={() => openBook(b.it)} title={b.it.path}>
      <span class="book-cover" style={m?.cover ? undefined : { background: `hsl(${hue(b.it.fp || b.it.name)} 32% 38%)` }}>
        {m?.cover ? <img src={coverUrl(b.it.fp)} alt="" loading="lazy" draggable={false} /> : <span class="book-plain">{b.title}</span>}
        {b.progress !== null && (
          <span class="book-prog" aria-label={`${Math.round(b.progress * 100)}% read`}>
            <span style={{ width: `${Math.max(3, b.progress * 100)}%` }} />
          </span>
        )}
      </span>
      <span class="book-t">{b.title}</span>
      <span class="book-a">{b.author || ext}</span>
    </button>
  );
}
