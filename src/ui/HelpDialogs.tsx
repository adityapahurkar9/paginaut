import { useEffect, useState } from "preact/hooks";
import { signal } from "@preact/signals";
import { getVersion } from "@tauri-apps/api/app";
import { X } from "lucide-preact";

/** Which help panel is open. */
export const helpPanel = signal<"shortcuts" | "about" | null>(null);

const SHORTCUTS: [string, [string, string][]][] = [
  ["Files", [
    ["Open", "Ctrl+O"], ["Save", "Ctrl+S"], ["Save as", "Ctrl+Shift+S"], ["New tab (Home)", "Ctrl+T"], ["Close tab", "Ctrl+W"],
    ["Next / previous tab", "Ctrl+Tab / Ctrl+Shift+Tab"], ["File menu", "Alt+F"],
  ]],
  ["Viewing", [
    ["Zoom in / out", "Ctrl+= / Ctrl+−  ·  Ctrl+wheel  ·  pinch"], ["Actual size", "Ctrl+0"], ["Fit page / fit width", "Ctrl+1 / Ctrl+2"],
    ["Go to page", "Ctrl+G"], ["Next / previous page", "→ / ←"], ["Read mode (reflowed text)", "Ctrl+H"], ["Book mode (full screen)", "F5 · Esc to leave"],
    ["Turn the page (Book)", "→ ← · Space · Page Up/Down · drag an edge"], ["Text size (Read, Book)", "Ctrl+= / Ctrl+− · pinch"],
    ["Full screen", "F11"], ["Sidebar", "F4"], ["Dark pages", "Ctrl+Shift+N"], ["Light / dark theme", "Ctrl+Shift+D"],
  ]],
  ["Pages", [
    ["Rotate right / left", "R / Shift+R"], ["Delete page", "Delete"], ["Undo / redo", "Ctrl+Z / Ctrl+Y"],
    ["Select several (thumbnails)", "Ctrl+click, Shift+click"],
  ]],
  ["Search and text", [
    ["Find", "Ctrl+F"], ["Next / previous match", "Enter / Shift+Enter, F3"], ["Commands and search", "Ctrl+K"],
    ["Copy selected text", "Ctrl+C"], ["Select page text", "Ctrl+A"], ["Hand / select tool", "H / V"],
  ]],
];

export function HelpDialogs() {
  const p = helpPanel.value;
  useEffect(() => {
    if (!p) return;
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        helpPanel.value = null;
      }
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [p]);
  if (!p) return null;
  return (
    <div class="overlay" onPointerDown={(e) => e.target === e.currentTarget && (helpPanel.value = null)}>
      <div class={`dialog help ${p}`} role="dialog" aria-label={p === "about" ? "About Paginaut" : "Keyboard shortcuts"}>
        <button class="ib sm close" aria-label="Close" onClick={() => (helpPanel.value = null)}>
          <X size={16} />
        </button>
        {p === "shortcuts" ? <Shortcuts /> : <About />}
      </div>
    </div>
  );
}

function Shortcuts() {
  return (
    <>
      <h2>Keyboard shortcuts</h2>
      <div class="keys">
        {SHORTCUTS.map(([group, rows]) => (
          <section key={group}>
            <h3>{group}</h3>
            {rows.map(([what, keys]) => (
              <div class="krow" key={what}>
                <span>{what}</span>
                <kbd>{keys}</kbd>
              </div>
            ))}
          </section>
        ))}
      </div>
    </>
  );
}

function About() {
  const [version, setVersion] = useState("");
  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);
  return (
    <>
      <h2>Paginaut {version}</h2>
      <span class="muted">A fast reader and editor for PDFs, e-books, comics and images.</span>
      <div class="about-rows">
        <div><span>Engine</span><span>MuPDF 1.28.5 (native, multi-threaded)</span></div>
        <div><span>Text recognition</span><span>Tesseract 5 (Apache-2.0), models from tessdata 4.1.0</span></div>
        <div><span>App</span><span>Tauri 2 · WebView2</span></div>
        <div><span>Threads</span><span>{navigator.hardwareConcurrency} logical processors</span></div>
        <div><span>Licence</span><span>AGPL-3.0 (MuPDF's licence)</span></div>
        <div><span>Updates</span><span>Update checks from GitHub are coming in a later version</span></div>
      </div>
    </>
  );
}
