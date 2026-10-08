// Help content shown in Settings: keyboard shortcuts, and About (with updates).
import { useEffect, useState } from "preact/hooks";
import { getVersion } from "@tauri-apps/api/app";
import { checkForUpdates, installUpdate, setAutoUpdates, stopUpdate, updatesSetting, updateState } from "../update";

const SHORTCUTS: [string, [string, string][]][] = [
  ["Files", [
    ["Open", "Ctrl+O"], ["Save", "Ctrl+S"], ["Save as", "Ctrl+Shift+S"], ["Print", "Ctrl+P"], ["New tab (Home)", "Ctrl+T"], ["Close tab", "Ctrl+W"],
    ["Next / previous tab", "Ctrl+Tab / Ctrl+Shift+Tab"],
    ["Switch tab (last used first)", "Hold Alt, press ` · Alt+~ goes back · let go to switch"], ["New window", "Ctrl+N"],
    ["Move a tab to its own window", "Drag it off the tab strip · right-click the tab"], ["Hand the file to another app", "Drag it from the status bar or Home's recent files"], ["File menu", "Alt+F"], ["Settings", "Ctrl+,"],
  ]],
  ["Viewing", [
    ["Zoom in / out", "Ctrl+= / Ctrl+−  ·  Ctrl+wheel  ·  pinch"], ["Actual size", "Ctrl+0"], ["Fit page / fit width", "Ctrl+1 / Ctrl+2"],
    ["Go to page", "Ctrl+G"], ["Next / previous page", "→ / ←"], ["Read mode (reflowed text)", "Ctrl+H"], ["Book mode (full screen)", "F5 · Esc to leave"], ["Read aloud / stop", "Ctrl+Shift+U"], ["Pause / resume reading", "Ctrl+Shift+Space"],
    ["Turn the page (Book)", "→ ← · Space · Page Up/Down · drag an edge"], ["Text size (Read, Book)", "Ctrl+= / Ctrl+− · pinch"],
    ["Full screen", "F11"], ["Sidebar", "F4"], ["Dark pages", "Ctrl+Shift+N"], ["Light / dark theme", "Ctrl+Shift+D"],
  ]],
  ["Pages", [
    ["Rotate right / left", "R / Shift+R"], ["Delete page", "Delete"], ["Page grid (organize pages)", "Ctrl+Shift+G"], ["Undo / redo", "Ctrl+Z / Ctrl+Y"],
    ["Select several (thumbnails)", "Ctrl+click, Shift+click"],
    ["Document properties", "Ctrl+D"], ["Rename a bookmark", "F2 · double-click"],
  ]],
  ["Search and text", [
    ["Find", "Ctrl+F"], ["Next / previous match", "Enter / Shift+Enter, F3"], ["Commands and search", "Ctrl+K"],
    ["Copy selected text", "Ctrl+C"], ["Select page text", "Ctrl+A"], ["Hand / select tool", "H / V"],
  ]],
];

/** Settings › Keyboard shortcuts. */
export function Shortcuts() {
  return (
    <>
      <h3>Keyboard shortcuts</h3>
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

/** Settings › About. */
export function About() {
  const [version, setVersion] = useState("");
  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);
  return (
    <>
      <h3>Paginaut {version}</h3>
      <span class="muted">A fast reader and editor for PDFs, e-books, comics and images.</span>
      <div class="about-rows">
        <div><span>Engine</span><span>MuPDF 1.28.5 (native, multi-threaded)</span></div>
        <div><span>Text recognition</span><span>Tesseract 5 (Apache-2.0), models from tessdata 4.1.0</span></div>
        <div><span>RAR comics</span><span>unarr (LGPL-3.0) for RAR 4, libarchive (BSD) for RAR 5</span></div>
        <div><span>App</span><span>Tauri 2 · WebView2</span></div>
        <div><span>Threads</span><span>{navigator.hardwareConcurrency} logical processors</span></div>
        <div><span>Licence</span><span>AGPL-3.0 (MuPDF's licence). Notices: THIRD-PARTY-NOTICES.md, installed with the app</span></div>
      </div>
      <Updates />
    </>
  );
}

function Updates() {
  const s = updateState.value;
  const auto = updatesSetting().auto;
  const pct = s.phase === "downloading" && s.total ? Math.round((s.done / s.total) * 100) : 0;
  return (
    <div class="tool-field upd">
      <label>Updates</label>
      {s.phase === "checked" && s.result.available ? (
        <div class="upd-new">
          <b>Paginaut {s.result.available.version} is available</b>
          {s.result.available.notes && <p class="upd-notes">{s.result.available.notes}</p>}
          <div class="theme-row">
            <button class="btn pri sm" onClick={() => void installUpdate()}>
              Update and restart
            </button>
            <span class="hint">Downloaded, its signature checked, then installed; Paginaut restarts.</span>
          </div>
        </div>
      ) : (
        <div class="theme-row">
          <button class="btn sm" disabled={s.phase === "checking" || s.phase === "downloading"} onClick={() => void checkForUpdates()}>
            {s.phase === "checking" ? "Checking…" : "Check for updates"}
          </button>
          <span class="hint">
            {s.phase === "checked" && (s.result.nonePublished ? "No releases have been published yet." : `You have the newest version (${s.result.current}).`)}
            {s.phase === "error" && s.message}
          </span>
        </div>
      )}
      {s.phase === "downloading" && (
        <div class="theme-row">
          <span class="rf-bar upd-bar">
            <span style={{ width: `${pct}%` }} />
          </span>
          <span class="hint">Downloading… {pct}%</span>
          <button class="btn ghost sm" onClick={stopUpdate}>
            Stop
          </button>
        </div>
      )}
      <label class="check-row set-check">
        <input type="checkbox" checked={auto} onChange={(e) => setAutoUpdates((e.target as HTMLInputElement).checked)} />
        <span>
          Check automatically once a day
          <small>Asks github.com for the newest release; nothing about you or your files is sent.</small>
        </span>
      </label>
    </div>
  );
}
