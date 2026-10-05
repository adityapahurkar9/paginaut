// The first screen: open a file, or carry on with one read before.
import { useState } from "preact/hooks";
import { FolderOpen, Settings, X } from "lucide-preact";
import { Logo } from "../ui/Logo";
import { openRecent, pickAndOpen } from "./open";
import { openSheet } from "./Sheets";
import { forget, recent, type Recent } from "./state";

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

export function Library() {
  const list = recent.value;
  const [removing, setRemoving] = useState<string | null>(null);
  return (
    <div class="lib">
      <header class="lib-head">
        <Logo size={30} />
        <h1>Paginaut</h1>
        <button class="ib" aria-label="Settings" onClick={() => openSheet("settings")}>
          <Settings size={22} />
        </button>
      </header>

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
              <RecentRow key={r.uri} r={r} removing={removing === r.uri} onHold={() => setRemoving(r.uri)} onDone={() => setRemoving(null)} />
            ))}
          </ul>
        </section>
      ) : (
        <div class="lib-empty">
          <Logo size={88} />
          <p>Files you open show up here, and open again where you left off.</p>
        </div>
      )}
    </div>
  );
}

function RecentRow({ r, removing, onHold, onDone }: { r: Recent; removing: boolean; onHold: () => void; onDone: () => void }) {
  const progress = r.pages > 1 ? (r.page + 1) / r.pages : 1;
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
            forget(r.uri);
            onDone();
          }}
        >
          <X size={16} /> Remove
        </button>
      )}
    </li>
  );
}
