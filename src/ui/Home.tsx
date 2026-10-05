import { useState } from "preact/hooks";
import { BookImage, BookOpen, Combine, File, FileText, FolderOpen, X } from "lucide-preact";
import * as A from "../actions";
import { Logo } from "./Logo";
import { startMerge } from "./PagesUI";
import { forgetFile, openFiles, recent } from "../store";

function ago(ms: number) {
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  if (s < 86400 * 7) return `${Math.floor(s / 86400)} d ago`;
  return new Date(ms).toLocaleDateString();
}

/** What the Recent list can be narrowed to, by file extension. */
type Kind = "pdf" | "ebook" | "comic" | "other";
const KINDS: { id: Kind; label: string; Icon: typeof FileText }[] = [
  { id: "pdf", label: "PDFs", Icon: FileText },
  { id: "ebook", label: "E-books", Icon: BookOpen },
  { id: "comic", label: "Comics", Icon: BookImage },
  { id: "other", label: "Others", Icon: File },
];
function kindOf(path: string): Kind {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  if (ext === "pdf") return "pdf";
  if (["epub", "mobi", "fb2", "fbz"].includes(ext)) return "ebook";
  if (["cbz", "cbr"].includes(ext)) return "comic";
  return "other";
}

export function Home() {
  const all = recent.value;
  const [want, setWant] = useState<Kind | "all">("all");
  // Tabs only for the kinds there are, and none when everything is one kind.
  const counts = new Map<Kind, number>();
  for (const r of all) counts.set(kindOf(r.path), (counts.get(kindOf(r.path)) ?? 0) + 1);
  const kinds = KINDS.filter((k) => counts.has(k.id));
  const shown = want !== "all" && counts.has(want) ? want : "all";
  const list = shown === "all" ? all : all.filter((r) => kindOf(r.path) === shown);
  return (
    <div class="home">
      <div class="home-inner">
        <div class="drop">
          <Logo size={56} tile />
          <h1>Open a document</h1>
          <span>PDF, scans, e-books (EPUB, MOBI, FB2), comics (CBZ, CBR), XPS and images.</span>
          <span>Drop files anywhere in this window, or</span>
          <button class="btn pri" onClick={A.openDialog} style={{ height: 34, padding: "0 16px" }}>
            <FolderOpen size={16} /> Choose files <kbd style={{ background: "transparent", color: "inherit", borderColor: "currentColor", opacity: 0.7 }}>Ctrl+O</kbd>
          </button>
          <button class="btn ghost" onClick={() => void startMerge(null)}>
            <Combine size={15} /> Merge files into one PDF…
          </button>
        </div>
        {all.length > 0 && (
          <div class="recent">
            <div class="recent-head">
              <h3>Recent</h3>
              {kinds.length > 1 && (
                <div class="recent-kinds" role="tablist" aria-label="Show">
                  {[{ id: "all" as const, label: "All", n: all.length }, ...kinds.map((k) => ({ id: k.id, label: k.label, n: counts.get(k.id)! }))].map((k) => (
                    <button key={k.id} role="tab" aria-selected={shown === k.id} class={shown === k.id ? "on" : ""} onClick={() => setWant(k.id)}>
                      {k.label} <span class="n">{k.n}</span>
                    </button>
                  ))}
                </div>
              )}
            </div>
            {list.slice(0, 12).map((r) => {
              const Icon = KINDS.find((k) => k.id === kindOf(r.path))!.Icon;
              return (
              <button class="recent-row" key={r.path} onClick={() => openFiles([r.path])} title={r.path}>
                <Icon size={18} style={{ color: "var(--tx3)" }} />
                <span style={{ display: "flex", flexDirection: "column", minWidth: 0 }}>
                  <span class="n">{r.name}</span>
                  <span class="p">{r.path}</span>
                </span>
                <span class="meta">
                  {r.view && r.pages ? `p. ${r.view.page + 1} of ${r.pages} · ` : ""}
                  {ago(r.opened)}
                </span>
                <span
                  class="ib sm"
                  role="button"
                  aria-label={`Remove ${r.name} from recent`}
                  title="Remove from list"
                  onClick={(e) => {
                    e.stopPropagation();
                    forgetFile(r.path);
                  }}
                >
                  <X size={14} />
                </span>
              </button>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
