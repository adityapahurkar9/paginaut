// The bar shown for a selected annotation: colour, line thickness, its note
// text (every annotation can carry a comment), and delete.
import { useEffect, useState } from "preact/hooks";
import { MessageSquareText, Trash2, X } from "lucide-preact";
import { deleteSelected, editAnnot, hexToRgb, PALETTE, rgbToHex, selectedAnnot } from "../annots";
import type { Tab } from "../store";

const NAMES: Record<string, string> = {
  Highlight: "Highlight", Underline: "Underline", StrikeOut: "Strike-through", Squiggly: "Squiggly underline", Ink: "Drawing",
  Text: "Note", FreeText: "Text box", Square: "Rectangle", Circle: "Ellipse", Line: "Line", Stamp: "Stamp", Polygon: "Polygon",
  PolyLine: "Lines", FileAttachment: "Attachment", Caret: "Insert mark",
};
const HAS_WIDTH = new Set(["Ink", "Square", "Circle", "Line", "Polygon", "PolyLine"]);

export function AnnotBar({ tab }: { tab: Tab }) {
  const s = selectedAnnot.value;
  const [text, setText] = useState("");
  const [open, setOpen] = useState(false);
  useEffect(() => {
    setText(s?.a.text ?? "");
    // Notes open straight into their text.
    setOpen(!!s && s.a.t === "Text");
  }, [s?.a.n, s?.src]);
  if (!s || s.tab !== tab) return null;
  const a = s.a;
  const isText = a.t === "FreeText";
  const color = rgbToHex(isText ? a.tc : a.c);
  const set = (props: Parameters<typeof editAnnot>[2]["props"]) => void editAnnot(tab, s.src, { num: a.n, props });
  const saveText = () => {
    if (text !== a.text) set({ contents: text });
  };
  return (
    <div class="annotbar" onKeyDown={(e) => e.stopPropagation()} onPointerDown={(e) => e.stopPropagation()}>
      <strong>{a.t === "Stamp" && (a.text === "Signature" || a.text === "Initials") ? a.text : NAMES[a.t] ?? a.t}</strong>
      {a.author && <span class="muted">{a.author}</span>}
      {a.t !== "Stamp" && (
        <div class="swatches">
          {PALETTE.map((c) => (
            <button
              key={c}
              class={`sw ${c === color ? "on" : ""}`}
              style={{ background: c }}
              aria-label={`Colour ${c}`}
              onClick={() => (isText ? set({ font: ["Helv", a.fs || 12, hexToRgb(c)] }) : set({ color: hexToRgb(c) }))}
            />
          ))}
        </div>
      )}
      {HAS_WIDTH.has(a.t) && (
        <select class="inp sm" value={String(a.bw ?? 1)} onChange={(e) => set({ border: Number((e.currentTarget as HTMLSelectElement).value) })} title="Line thickness">
          {[0.5, 1, 1.5, 2, 3, 5, 8].map((w) => (
            <option key={w} value={String(w)}>
              {w} pt
            </option>
          ))}
        </select>
      )}
      {isText && (
        <select class="inp sm" value={String(a.fs || 12)} onChange={(e) => set({ font: ["Helv", Number((e.currentTarget as HTMLSelectElement).value), a.tc ?? [0, 0, 0]] })} title="Text size">
          {[8, 9, 10, 11, 12, 14, 16, 18, 24, 32].map((z) => (
            <option key={z} value={String(z)}>
              {z} pt
            </option>
          ))}
        </select>
      )}
      {!isText && (
        <button class={`ib sm ${open ? "on" : ""}`} title="Comment" aria-label="Comment" onClick={() => setOpen(!open)}>
          <MessageSquareText size={16} />
        </button>
      )}
      <button class="ib sm" title="Delete (Del)" aria-label="Delete" onClick={() => void deleteSelected()}>
        <Trash2 size={16} />
      </button>
      <button class="ib sm" title="Done (Esc)" aria-label="Close" onClick={() => (selectedAnnot.value = null)}>
        <X size={15} />
      </button>
      {open && !isText && (
        <textarea
          class="inp annot-note"
          placeholder="Add a comment…"
          value={text}
          ref={(el) => {
            if (el && a.t === "Text" && !a.text) requestAnimationFrame(() => el.focus());
          }}
          onInput={(e) => setText((e.currentTarget as HTMLTextAreaElement).value)}
          onBlur={saveText}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) (e.currentTarget as HTMLTextAreaElement).blur();
          }}
        />
      )}
    </div>
  );
}
