// The Reading pane (right side): how Read and Book modes show the text (size, font, spacing, margins, alignment,
// paper), changed next to the text itself. Book mode is full screen: it keeps its own Aa panel.
import { AudioLines, Wrench, X } from "lucide-preact";
import { speaking, toggleReadAloud } from "../speech";
import { VoiceChoices } from "./SpeechBar";
import { bookScreen, setViewMode, viewModeOf } from "../modes";
import { closePane, pane } from "../panes";
import { activeTab } from "../store";
import { startLayoutEdit } from "./LayoutPanel";
import { TypeControls } from "./Reflow";

export function ReadingPane() {
  if (pane.value !== "reading" || bookScreen.value) return null;
  const tab = activeTab.value;
  const ready = tab?.status.value === "ready" ? tab : null;
  const mode = ready ? viewModeOf(ready) : null;
  return (
    <aside class="signpane reading" aria-label="Reading">
      <div class="signpane-head">
        <strong>Reading</strong>
        <button class="ib sm" aria-label="Close the Reading pane" title="Close" onClick={() => closePane("reading")}>
          <X size={15} />
        </button>
      </div>
      <div class="signpane-body">
        {mode !== "read" && (
          <p class="muted small pane-note">
            How Read and Book modes show the text: the text reflowed into one comfortable column (Read, Ctrl+H) or a two-page book (Book, F5).{" "}
            {ready && (
              <button class="linkish" onClick={() => void setViewMode(ready, "read")}>
                Read mode
              </button>
            )}
          </p>
        )}
        <section class="pane-sec set-type">
          <TypeControls />
        </section>
        <section class="pane-sec read-aloud">
          <label>Read aloud</label>
          <VoiceChoices />
          <button class="btn sm" disabled={!ready && !speaking.value} onClick={() => toggleReadAloud(ready)} title="From where you are, or the text you selected (Ctrl+Shift+U)">
            <AudioLines size={15} strokeWidth={1.7} /> {speaking.value ? "Stop reading" : "Read aloud"}
          </button>
        </section>
        {ready && (mode === "read" || mode === "book") && (
          <section class="pane-sec">
            <button class="pane-row" onClick={() => void startLayoutEdit(ready)}>
              <Wrench size={16} strokeWidth={1.7} />
              <span>Fix the reading layout…</span>
              <small>columns, order, pictures</small>
            </button>
          </section>
        )}
      </div>
    </aside>
  );
}
