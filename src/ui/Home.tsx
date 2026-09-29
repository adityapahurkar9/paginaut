import { Combine, FileText, FolderOpen, X } from "lucide-preact";
import * as A from "../actions";
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

export function Home() {
  const list = recent.value;
  return (
    <div class="home">
      <div class="home-inner">
        <div class="drop">
          <svg width="44" height="44" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M5 2h9l6 6v14H5z" fill="var(--acc)" />
            <path d="M14 2v6h6" fill="none" stroke="var(--surface)" stroke-width="1.4" stroke-linejoin="round" />
            <path d="M8.5 13h7M8.5 16.5h5" stroke="var(--on-acc)" stroke-width="1.5" stroke-linecap="round" />
          </svg>
          <h1>Open a document</h1>
          <span>PDF, scans, e-books (EPUB, MOBI, FB2), comics (CBZ), XPS and images.</span>
          <span>Drop files anywhere in this window, or</span>
          <button class="btn pri" onClick={A.openDialog} style={{ height: 34, padding: "0 16px" }}>
            <FolderOpen size={16} /> Choose files <kbd style={{ background: "transparent", color: "inherit", borderColor: "currentColor", opacity: 0.7 }}>Ctrl+O</kbd>
          </button>
          <button class="btn ghost" onClick={() => void startMerge(null)}>
            <Combine size={15} /> Merge files into one PDF…
          </button>
        </div>
        {list.length > 0 && (
          <div class="recent">
            <h3>Recent</h3>
            {list.slice(0, 12).map((r) => (
              <button class="recent-row" key={r.path} onClick={() => openFiles([r.path])} title={r.path}>
                <FileText size={18} style={{ color: "var(--tx3)" }} />
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
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
