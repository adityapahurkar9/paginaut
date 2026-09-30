// The Markdown pane (right side): a .md file's source, the pages following as you type; Ctrl+S saves the file.
import { useEffect, useRef, useState } from "preact/hooks";
import { X } from "lucide-preact";
import { isMarkdown, loadMdSource, mdInput, mdLoaded, mdSource, saveMarkdown } from "../markdown";
import { bookScreen } from "../modes";
import { closePane, pane } from "../panes";
import { activeTab, type Tab } from "../store";

export function MarkdownPane() {
  const tab = activeTab.value;
  const md = pane.value === "markdown" && !bookScreen.value && !!tab && isMarkdown(tab.path);
  void mdLoaded.value;
  // Another Markdown tab: its source is read when the pane first shows it.
  useEffect(() => {
    if (md && !mdSource(tab)) void loadMdSource(tab);
  }, [md, tab]);
  if (pane.value !== "markdown" || bookScreen.value) return null;
  if (!md || !mdSource(tab)) {
    return (
      <aside class="signpane mdpane" aria-label="Markdown">
        <div class="signpane-head">
          <strong>Markdown</strong>
          <button class="ib sm" aria-label="Close the Markdown pane" title="Close" onClick={() => closePane("markdown")}>
            <X size={15} />
          </button>
        </div>
        <div class="signpane-body">
          <p class="muted small">{md ? "Reading…" : "Open a Markdown file (.md) to edit its text here; the pages follow as you type."}</p>
        </div>
      </aside>
    );
  }
  return <Editor key={tab.key} tab={tab} />;
}

function Editor({ tab }: { tab: Tab }) {
  const src = mdSource(tab)!;
  const [text, setText] = useState(src.text);
  const area = useRef<HTMLTextAreaElement>(null);
  const unsaved = tab.docChanged.value;
  // Start at the top of the text, ready to type.
  useEffect(() => {
    const a = area.current;
    if (!a) return;
    a.setSelectionRange(0, 0);
    a.focus({ preventScroll: true });
    a.scrollTop = 0;
  }, []);
  return (
    <aside class="signpane mdpane" aria-label="Markdown">
      <div class="signpane-head">
        <strong>Markdown</strong>
        <button class="ib sm" aria-label="Close the Markdown pane" title="Close (your changes stay until you close the file)" onClick={() => closePane("markdown")}>
          <X size={15} />
        </button>
      </div>
      <div class="signpane-body">
        <div class="md-bar">
          <span>{unsaved ? "Changes not saved yet" : "Saved"}</span>
          <button class="btn pri sm" disabled={!unsaved} onClick={() => void saveMarkdown(tab)}>
            Save
          </button>
        </div>
        <textarea
          ref={area}
          class="md-source"
          spellcheck={false}
          aria-label="Markdown source"
          value={text}
          onInput={(e) => {
            const v = (e.target as HTMLTextAreaElement).value;
            setText(v);
            mdInput(tab, v);
          }}
          onKeyDown={(e) => {
            // Typing is for the text, not the app's shortcuts; Ctrl+S still saves.
            if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
              e.preventDefault();
              void saveMarkdown(tab);
            } else if (e.key === "Tab") {
              e.preventDefault();
              const t = e.currentTarget;
              t.setRangeText("    ", t.selectionStart, t.selectionEnd, "end");
              setText(t.value);
              mdInput(tab, t.value);
            }
            e.stopPropagation();
          }}
        />
        <span class="hint">Headings #, lists -, **bold**, *italic*, tables with |, pictures ![](file.png) from this file's folder.</span>
      </div>
    </aside>
  );
}
