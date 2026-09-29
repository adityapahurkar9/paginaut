// A slim bar over a fillable form: how many fields, and quick actions.
import { useState } from "preact/hooks";
import { FormInput, X } from "lucide-preact";
import { clearForm } from "../forms";
import { selectedAnnot } from "../annots";
import * as P from "../pages";
import { settings, updateSettings, type Tab } from "../store";

const dismissed = new WeakSet<Tab>();

export function FormBar({ tab }: { tab: Tab }) {
  const [, redraw] = useState(0);
  const f = tab.forms.value;
  if (!f || dismissed.has(tab) || (!f.fields && !f.xfa)) return null;
  // The selected annotation's bar takes its place.
  if (selectedAnnot.value?.tab === tab) return null;
  const close = () => {
    dismissed.add(tab);
    redraw((n) => n + 1);
  };
  if (!f.fields)
    return (
      <div class="formbar warn">
        <FormInput size={16} />
        <span>This is a dynamic (XFA) form. Folio can show it but not fill it in; it needs Adobe Reader.</span>
        <button class="ib sm" aria-label="Dismiss" onClick={close}>
          <X size={14} />
        </button>
      </div>
    );
  const hl = settings.value.highlightFields;
  return (
    <div class="formbar">
      <FormInput size={16} />
      <span>
        This form has {f.fields} fillable field{f.fields === 1 ? "" : "s"}. Click a field to fill it in; Tab moves to the next one.
      </span>
      <button class={`btn ghost ${hl ? "on" : ""}`} onClick={() => updateSettings({ highlightFields: !hl })}>
        {hl ? "Hide highlights" : "Highlight fields"}
      </button>
      <button class="btn ghost" onClick={() => void clearForm(tab)}>Clear form</button>
      <button class="btn ghost" onClick={() => void P.saveFlattened(tab)} title="A copy where the answers are part of the page and can't be changed">
        Save flattened copy
      </button>
      <button class="ib sm" aria-label="Dismiss" onClick={close}>
        <X size={14} />
      </button>
    </div>
  );
}
