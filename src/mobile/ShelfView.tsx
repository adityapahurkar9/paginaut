// The E-books and Comics tabs: covers of every book the shelf found, those being read first; and
// where the shelf looks (chosen folders, all-files access).
import { useRef, useState } from "preact/hooks";
import { BookOpen, Folder, FolderPlus, Images, RefreshCw, X } from "lucide-preact";
import { coverUrl } from "../api";
import { native } from "./native";
import { pushLayer } from "./nav";
import { progressOf, storage } from "./records";
import { addFolder, openShelf, removeFolder, scanning, scanShelf, sections, shelf, shelfOn, type ShelfItem, type Shelved } from "./shelf";
import { load, save, sheet } from "./state";
import { SortToggle, type SortState } from "./SortToggle";

type Sort = "title" | "author" | "added";
const NOUN = { ebook: "e-books", comic: "comics" } as const;

/** A plain cover for a book without one: its title on a colour picked from it. */
const PLAIN = [
  ["#e1f5ee", "#085041"],
  ["#faece7", "#712b13"],
  ["#eeedfe", "#3c3489"],
  ["#faeeda", "#633806"],
  ["#e6f1fb", "#0c447c"],
  ["#f1efe8", "#444441"],
];
function plainFor(t: string) {
  let h = 0;
  for (const c of t) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return PLAIN[h % PLAIN.length];
}

function ago(t: number) {
  const d = Math.floor((Date.now() - t) / 86_400_000);
  return d <= 0 ? "today" : d === 1 ? "yesterday" : d < 7 ? `${d} d` : d < 60 ? `${Math.round(d / 7)} w` : `${Math.round(d / 30)} mo`;
}

export function ShelfPane({ kind }: { kind: ShelfItem["kind"] }) {
  const [sort, setSortState] = useState<SortState<Sort>>(load<SortState<Sort>>("paginaut.m.shelfsort2", { key: "title", desc: false }));
  const setSort = (s: SortState<Sort>) => {
    setSortState(s);
    save("paginaut.m.shelfsort2", s);
  };
  const pull = usePullToScan();
  if (!storage.value) return null;
  if (!shelfOn.value) return <ShelfSetup kind={kind} />;
  const { all, reading, rest, finished } = sections(kind, sort);
  return (
    <div class="shelf" {...pull.handlers}>
      <div class={`shelf-pull ${pull.shown || scanning.value ? "on" : ""}`} style={pull.shown ? { height: `${pull.shown}px` } : undefined}>
        <RefreshCw size={18} class={scanning.value ? "spinning" : ""} /> {scanning.value ? "Looking for books…" : pull.ready ? "Let go to look again" : "Pull to look again"}
      </div>
      {all.length === 0 ? (
        <p class="hint shelf-none">{scanning.value ? "" : `No ${NOUN[kind]} found where the shelf looks. Add a folder, or look through the whole phone.`}</p>
      ) : (
        <>
          {reading.length > 0 && <Grid title="Reading now" list={reading} progress />}
          {rest.length > 0 && (
            <>
              <div class="shelf-head">
                <h2>{reading.length || finished.length ? "Not started" : `All ${NOUN[kind]}`}</h2>
                <SortToggle<Sort>
                  value={sort}
                  onChange={setSort}
                  order={[
                    ["title", "Title", false],
                    ["author", "Author", false],
                    ["added", "Added", true],
                  ]}
                />
              </div>
              <Grid list={rest} />
            </>
          )}
          {finished.length > 0 && <Grid title="Finished" list={finished} />}
        </>
      )}
      <Where kind={kind} />
    </div>
  );
}

function Grid({ title, list, progress }: { title?: string; list: Shelved[]; progress?: boolean }) {
  return (
    <section class="shelf-sec">
      {title && (
        <div class="shelf-head">
          <h2>{title}</h2>
        </div>
      )}
      <div class="shelf-grid">
        {list.map((s) => (
          <Tile key={s.it.id} s={s} progress={progress} />
        ))}
      </div>
    </section>
  );
}

function Tile({ s, progress }: { s: Shelved; progress?: boolean }) {
  const p = s.r ? progressOf(s.r) : 0;
  const [bg, fg] = plainFor(s.title);
  const ext = s.it.name.split(".").pop()?.toUpperCase() ?? "";
  return (
    <button class="tile" onClick={() => void openShelf(s.it)} title={s.it.name}>
      <span class="cover" style={s.m?.cover ? undefined : { background: bg, color: fg }}>
        {s.m?.cover ? <img src={coverUrl(s.it.fp)} alt="" loading="lazy" /> : <span class="cover-t">{s.title}</span>}
      </span>
      {progress && (
        <span class="tile-bar">
          <span style={{ width: `${Math.max(3, p * 100)}%` }} />
        </span>
      )}
      <span class="tile-t">{s.title}</span>
      <span class="tile-a">{progress && s.r ? `${Math.round(p * 100)}% · ${ago(s.r.at)}` : s.author || ext}</span>
    </button>
  );
}

/** Where the shelf looks, at its foot; opens the list of folders. */
function Where({ kind }: { kind: ShelfItem["kind"] }) {
  const st = storage.value!;
  const where = [...(st.allFiles ? ["The whole phone"] : []), ...st.folders.map((f) => f.name)].join(", ");
  const n = shelf.value.filter((it) => it.kind === kind).length;
  return (
    <button class="shelf-where" onClick={() => openShelfSheet()}>
      <Folder size={18} />
      <span>
        {where} · {n} {NOUN[kind]}
      </span>
      <strong>Folders</strong>
    </button>
  );
}

export function openShelfSheet() {
  if (!sheet.value) pushLayer(() => (sheet.value = null));
  sheet.value = "shelf";
}

function ShelfSetup({ kind }: { kind: ShelfItem["kind"] }) {
  const Icon = kind === "comic" ? Images : BookOpen;
  return (
    <div class="shelf-setup">
      <Icon size={40} />
      <h2>Find your {NOUN[kind]}</h2>
      <p>Choose the folders where you keep them, or let Paginaut look through the whole phone. They're read where they are: nothing is copied.</p>
      <button class="btn pri" onClick={() => void addFolder()}>
        <FolderPlus size={18} /> Choose a folder
      </button>
      <button class="btn" onClick={() => void native.allFilesAccess()}>
        Look through the whole phone
      </button>
      <p class="hint">
        Looking through the whole phone needs Android's "All files access" for Paginaut. It only reads e-books and comics, never changes a file, and you can turn it off any time (Settings ›
        Storage). Android doesn't let apps choose the Download folder itself; a folder inside it works.
      </p>
    </div>
  );
}

/** Where the shelf looks: the chosen folders, all-files access, and looking again. */
export function ShelfAccess() {
  const st = storage.value;
  if (!st) return null;
  return (
    <>
      <div class="store-head">
        <span>Where the shelf looks</span>
        <span class="store-num">{shelf.value.length} books</span>
      </div>
      {st.folders.length > 0 && (
        <ul class="store-list">
          {st.folders.map((f) => (
            <li key={f.uri}>
              <Folder size={20} class="store-ic" />
              <span class="store-tx">
                <span class="store-name">{f.name}</span>
              </span>
              <button class="ib sm" aria-label={`Stop looking in ${f.name}`} onClick={() => void removeFolder(f.uri)}>
                <X size={18} />
              </button>
            </li>
          ))}
        </ul>
      )}
      <button class="row link-row" onClick={() => void addFolder()}>
        <span class="row-label">Add a folder</span>
      </button>
      <div class="row">
        <span class="row-label row-two">
          All files access
          <small>{st.allFiles ? "On: the shelf looks through the whole phone" : "Off: only the folders above"}</small>
        </span>
        <button class="btn sm" onClick={() => void native.allFilesAccess()}>
          {st.allFiles ? "Turn off" : "Turn on"}
        </button>
      </div>
      {(st.allFiles || st.folders.length > 0) && (
        <button class="row link-row" onClick={() => void scanShelf(true)}>
          <span class="row-label">{scanning.value ? "Looking for books…" : "Look for books again"}</span>
        </button>
      )}
    </>
  );
}

export function ShelfSheet() {
  return (
    <>
      <h3>Shelf</h3>
      <div class="sheet-scroll">
        <ShelfAccess />
      </div>
    </>
  );
}

/** Pulling the shelf down from its top looks for books again. */
function usePullToScan() {
  const [shown, setShown] = useState(0);
  const y0 = useRef<number | null>(null);
  const READY = 70;
  const scroller = (e: TouchEvent) => (e.currentTarget as HTMLElement).closest(".lib");
  return {
    shown,
    ready: shown >= READY,
    handlers: {
      onTouchStart: (e: TouchEvent) => {
        y0.current = (scroller(e)?.scrollTop ?? 1) <= 0 ? e.touches[0].clientY : null;
      },
      onTouchMove: (e: TouchEvent) => {
        if (y0.current === null) return;
        const dy = e.touches[0].clientY - y0.current;
        setShown(dy > 8 ? Math.min(96, (dy - 8) * 0.6) : 0);
      },
      onTouchEnd: () => {
        if (shown >= READY) void scanShelf(true);
        y0.current = null;
        setShown(0);
      },
    },
  };
}
