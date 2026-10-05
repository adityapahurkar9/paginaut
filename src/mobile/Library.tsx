// The first screen: open a file, or carry on with one read before (Recent); and the shelf of
// every e-book and comic (E-books, Comics).
import { signal } from "@preact/signals";
import { useEffect, useRef, useState } from "preact/hooks";
import { ArrowLeft, FolderOpen, Search, Settings, X } from "lucide-preact";
import { Logo } from "../ui/Logo";
import { openRecent, pickAndOpen } from "./open";
import { back, pushLayer } from "./nav";
import { openSheet } from "./Sheets";
import { progressOf, recent, records, unlist, type ReadRecord } from "./records";
import { metas, openShelf, recordFor, shelf, shelfOn, shelved, type Shelved } from "./shelf";
import { ShelfPane } from "./ShelfView";
import { load, save } from "./state";

type Tab = "recent" | "ebook" | "comic";
const tab = signal<Tab>(load<Tab>("paginaut.m.tab", "recent"));
function setTab(t: Tab) {
  tab.value = t;
  save("paginaut.m.tab", t);
}

const FORMAT_COLORS: Record<string, string> = {
  PDF: "#2f5bd3",
  EPUB: "#1f8a70",
  MOBI: "#a0662a",
  FB2: "#a0662a",
  CBZ: "#b0408a",
  CBR: "#b0408a",
  XPS: "#6a55c8",
  OXPS: "#6a55c8",
  MD: "#5b6475",
};

function ago(t: number) {
  const s = (Date.now() - t) / 1000;
  if (s < 90) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  const d = Math.round(s / 86400);
  if (d === 1) return "yesterday";
  if (d < 7) return `${d} days ago`;
  return new Date(t).toLocaleDateString(undefined, { day: "numeric", month: "short", year: d > 300 ? "numeric" : undefined });
}

/** Searching the library (the header turns into a search field). */
const finding = signal(false);

function startFinding() {
  pushLayer(() => (finding.value = false));
  finding.value = true;
}

export function Library() {
  const t = tab.value;
  if (finding.value)
    return (
      <div class="lib">
        <LibSearch />
      </div>
    );
  const count = (k: "ebook" | "comic") => (shelfOn.value ? shelf.value.filter((it) => it.kind === k).length : 0);
  const tabs: [Tab, string, number][] = [
    ["recent", "Recent", 0],
    ["ebook", "E-books", count("ebook")],
    ["comic", "Comics", count("comic")],
  ];
  return (
    <div class="lib">
      <header class="lib-head">
        <Logo size={30} />
        <h1>Paginaut</h1>
        <button class="ib" aria-label="Search your files" onClick={startFinding}>
          <Search size={22} />
        </button>
        <button class="ib" aria-label="Settings" onClick={() => openSheet("settings")}>
          <Settings size={22} />
        </button>
      </header>
      <div class="lib-tabs" role="tablist">
        {tabs.map(([id, label, n]) => (
          <button key={id} role="tab" aria-selected={t === id} class={t === id ? "on" : ""} onClick={() => setTab(id)}>
            {label}
            {n > 0 && <span class="n">{n}</span>}
          </button>
        ))}
      </div>
      {t === "recent" ? <RecentPane /> : <ShelfPane kind={t} key={t} />}
    </div>
  );
}

function RecentPane() {
  const list = recent.value;
  const [removing, setRemoving] = useState<string | null>(null);
  return (
    <>

      <button class="open-card" onClick={() => void pickAndOpen()}>
        <span class="open-ic">
          <FolderOpen size={26} />
        </span>
        <span class="open-tx">
          <strong>Open a file</strong>
          <small>PDF, e-books, comics, XPS, Markdown</small>
        </span>
      </button>

      {list.length > 0 ? (
        <section class="recent">
          <h2>Recent</h2>
          <ul>
            {list.map((r) => (
              <RecentRow key={r.id} r={r} removing={removing === r.id} onHold={() => setRemoving(r.id)} onDone={() => setRemoving(null)} />
            ))}
          </ul>
        </section>
      ) : (
        <div class="lib-empty">
          <Logo size={88} />
          <p>Files you open show up here, and open again where you left off.</p>
        </div>
      )}
    </>
  );
}

function RecentRow({ r, removing, onHold, onDone }: { r: ReadRecord; removing: boolean; onHold: () => void; onDone: () => void }) {
  const progress = r.pages > 1 ? progressOf(r) : 1;
  const color = FORMAT_COLORS[r.format] ?? "#5b6475";
  const title = r.name.replace(/\.[^.]+$/, "");
  return (
    <li class={`rec ${removing ? "removing" : ""}`}>
      <button
        class="rec-main"
        onClick={() => (removing ? onDone() : void openRecent(r))}
        onContextMenu={(e) => {
          e.preventDefault();
          onHold();
        }}
      >
        <span class="rec-badge" style={{ background: color }}>
          {r.format.slice(0, 4)}
        </span>
        <span class="rec-tx">
          <span class="rec-name">{title}</span>
          <span class="rec-meta">
            {r.mode === "pages" || r.pages <= 1 ? `Page ${r.page + 1} of ${r.pages}` : `${Math.round(progress * 100)}% read`} · {ago(r.at)}
          </span>
          <span class="rec-bar">
            <span style={{ width: `${Math.max(2, progress * 100)}%`, background: color }} />
          </span>
        </span>
      </button>
      {removing && (
        <button
          class="rec-remove"
          onClick={() => {
            unlist(r.id);
            onDone();
          }}
        >
          <X size={16} /> Remove
        </button>
      )}
    </li>
  );
}

/** What a search matches: a recent file, or a book on the shelf. */
type Hit = { kind: "recent"; r: ReadRecord } | { kind: "shelf"; s: Shelved };

/**
 * Files in Recent and books on the shelf whose name (extension included: "pdf", ".cbr"), title or
 * author has every word typed.
 */
function search(q: string): Hit[] {
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  const has = (...fields: string[]) => {
    const hay = fields.join(" ").toLowerCase();
    return words.every((w) => hay.includes(w));
  };
  const list = records.value;
  const m = metas.value;
  const hits: Hit[] = [];
  const seen = new Set<string>();
  for (const r of recent.value) {
    if (!has(r.name, r.title, r.format)) continue;
    hits.push({ kind: "recent", r });
    seen.add(r.id);
  }
  if (shelfOn.value)
    for (const it of shelf.value) {
      const s = shelved(it, list, m);
      if (s.r && seen.has(s.r.id)) continue;
      if (has(it.name, s.title, s.author)) hits.push({ kind: "shelf", s });
    }
  return hits;
}

function LibSearch() {
  const [q, setQ] = useState("");
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);
  const hits = search(q);
  return (
    <>
      <header class="lib-head lib-find">
        <button class="ib" aria-label="Close the search" onClick={() => back()}>
          <ArrowLeft size={22} />
        </button>
        <input
          ref={input}
          type="search"
          enterKeyHint="search"
          placeholder="Name, title, author or pdf, epub…"
          value={q}
          onInput={(e) => setQ((e.target as HTMLInputElement).value)}
        />
        {q && (
          <button class="ib" aria-label="Clear the search" onClick={() => (setQ(""), input.current?.focus())}>
            <X size={20} />
          </button>
        )}
      </header>
      {q.trim() && hits.length === 0 && <p class="hint find-none">Nothing in Recent{shelfOn.value ? " or on the shelf" : ""} matches.</p>}
      {hits.length > 0 && (
        <section class="recent">
          <ul>
            {hits.map((h) => (h.kind === "recent" ? <RecentHit key={h.r.id} r={h.r} /> : <ShelfHit key={h.s.it.id} s={h.s} />))}
          </ul>
        </section>
      )}
    </>
  );
}

/** Opens a search hit: the search closes first, so back from the reader returns to the library. */
function openHit(open: () => void) {
  back();
  setTimeout(open, 0);
}

function RecentHit({ r }: { r: ReadRecord }) {
  const color = FORMAT_COLORS[r.format] ?? "#5b6475";
  return (
    <li class="rec">
      <button class="rec-main" onClick={() => openHit(() => void openRecent(r))}>
        <span class="rec-badge" style={{ background: color }}>
          {r.format.slice(0, 4)}
        </span>
        <span class="rec-tx">
          <span class="rec-name">{r.title.trim() || r.name.replace(/\.[^.]+$/, "")}</span>
          <span class="rec-meta">
            Recent · {r.name} · {ago(r.at)}
          </span>
        </span>
      </button>
    </li>
  );
}

function ShelfHit({ s }: { s: Shelved }) {
  const ext = s.it.name.split(".").pop()?.toUpperCase() ?? "";
  const color = FORMAT_COLORS[ext] ?? "#5b6475";
  return (
    <li class="rec">
      <button class="rec-main" onClick={() => openHit(() => void openShelf(s.it))}>
        <span class="rec-badge" style={{ background: color }}>
          {ext.slice(0, 4)}
        </span>
        <span class="rec-tx">
          <span class="rec-name">{s.title}</span>
          <span class="rec-meta">
            {s.it.kind === "comic" ? "Comics" : "E-books"}
            {s.author && <> · {s.author}</>} · {s.it.name}
          </span>
        </span>
      </button>
    </li>
  );
}
