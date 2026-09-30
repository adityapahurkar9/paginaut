// Settings (Help › Settings, Ctrl+,): every setting in one place, a tab per area.
import { useEffect } from "preact/hooks";
import { signal } from "@preact/signals";
import type { ComponentChildren } from "preact";
import { ArrowDown, ArrowUp, X } from "lucide-preact";
import { compareOptions, setCompareOption } from "../compare";
import { hasModel, loadLangs, ocrLangs, rememberChoice, type OcrQuality } from "../ocr";
import { engines, loadEngines, officeDialog, setPreference } from "../office";
import { moveOnRail, onRail, railOrder, railShown, resetRail, setOnRail, setRailShown } from "../panes";
import { settings, updateSettings, type Settings } from "../store";
import { PANES } from "./Rail";
import { TypeControls } from "./Reflow";

export type SettingsTab = "general" | "reading" | "sidebar" | "signing" | "ocr" | "office" | "compare";

export const settingsOpen = signal<SettingsTab | null>(null);

export function openSettings(tab: SettingsTab = "general") {
  settingsOpen.value = tab;
}

const TABS: [SettingsTab, string][] = [
  ["general", "General"],
  ["reading", "Read and Book"],
  ["sidebar", "Side bar"],
  ["signing", "Signing"],
  ["ocr", "Text recognition"],
  ["office", "Office files"],
  ["compare", "Compare"],
];

export function SettingsDialog() {
  const tab = settingsOpen.value;
  useEffect(() => {
    if (!tab) return;
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        settingsOpen.value = null;
      }
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [!!tab]);
  if (!tab) return null;
  return (
    <div class="overlay" onPointerDown={(e) => e.target === e.currentTarget && (settingsOpen.value = null)}>
      <div class="dialog settings-dialog" role="dialog" aria-label="Settings">
        <button class="ib sm close" aria-label="Close" onClick={() => (settingsOpen.value = null)}>
          <X size={16} />
        </button>
        <nav class="set-tabs" role="tablist">
          <h2>Settings</h2>
          {TABS.map(([id, label]) => (
            <button key={id} role="tab" aria-selected={tab === id} class={`set-tab ${tab === id ? "on" : ""}`} onClick={() => (settingsOpen.value = id)}>
              {label}
            </button>
          ))}
        </nav>
        <div class="set-body" onKeyDown={(e) => e.stopPropagation()}>
          {tab === "general" && <General />}
          {tab === "reading" && <Reading />}
          {tab === "sidebar" && <SideBar />}
          {tab === "signing" && <Signing />}
          {tab === "ocr" && <Ocr />}
          {tab === "office" && <Office />}
          {tab === "compare" && <Compare />}
        </div>
      </div>
    </div>
  );
}

function Field({ label, hint, children }: { label: string; hint?: string; children: ComponentChildren }) {
  return (
    <div class="tool-field">
      <label>{label}</label>
      {children}
      {hint && <span class="hint">{hint}</span>}
    </div>
  );
}

function Check({ on, label, set, hint }: { on: boolean; label: string; set: (v: boolean) => void; hint?: string }) {
  return (
    <label class="check-row set-check">
      <input type="checkbox" checked={on} onChange={(e) => set((e.target as HTMLInputElement).checked)} />
      <span>
        {label}
        {hint && <small>{hint}</small>}
      </span>
    </label>
  );
}

function Seg<T extends string | boolean>({ value, items, set }: { value: T; items: [T, string][]; set: (v: T) => void }) {
  return (
    <div class="seg">
      {items.map(([v, label]) => (
        <button key={String(v)} class={`segb ${value === v ? "on" : ""}`} onClick={() => set(v)}>
          {label}
        </button>
      ))}
    </div>
  );
}

const put = (patch: Partial<Settings>) => updateSettings(patch);

function General() {
  const s = settings.value;
  return (
    <>
      <h3>General</h3>
      <Field label="Theme">
        <Seg value={s.theme} items={[["light", "Light"], ["dark", "Dark"], ["system", "Match Windows"]]} set={(theme) => put({ theme })} />
      </Field>
      <Field label="Pages">
        <Check on={s.night} label="Dark pages (night mode)" hint="Pages shown light on dark; photos keep their colours. Ctrl+Shift+N" set={(night) => put({ night })} />
        <Check on={s.coverSeparate} label="Cover page on its own in Double page view" set={(coverSeparate) => put({ coverSeparate })} />
        <Check on={s.highlightFields} label="Highlight fillable form fields" set={(highlightFields) => put({ highlightFields })} />
      </Field>
      <Field label="Saving">
        <Check on={s.autoSaveRotations} label="Save rotations automatically" hint="PDFs only, when nothing else changed." set={(autoSaveRotations) => put({ autoSaveRotations })} />
      </Field>
    </>
  );
}

function Reading() {
  return (
    <>
      <h3>Read and Book modes</h3>
      <span class="hint">How the reflowed text looks (also under the Aa button while reading).</span>
      <div class="set-type">
        <TypeControls />
      </div>
    </>
  );
}

function SideBar() {
  const order = railOrder();
  return (
    <>
      <h3>Side bar</h3>
      <span class="hint">The buttons at the right edge of the window, one for each pane. Tick the ones you want, and put them in the order you like.</span>
      <Check on={railShown()} label="Show the side bar" set={setRailShown} />
      <div class="set-rail">
        {order.map((id, i) => {
          const p = PANES[id];
          const Icon = p.icon;
          return (
            <div key={id} class={`set-rail-row ${onRail(id) ? "" : "off"}`}>
              <input type="checkbox" checked={onRail(id)} aria-label={`${p.label} on the side bar`} onChange={(e) => setOnRail(id, (e.target as HTMLInputElement).checked)} />
              <Icon size={17} strokeWidth={1.7} />
              <span>
                {p.label}
                {id === "markdown" && <small> (shown for .md files)</small>}
              </span>
              <button class="ib sm" aria-label="Move up" title="Move up" disabled={i === 0} onClick={() => moveOnRail(id, -1)}>
                <ArrowUp size={14} />
              </button>
              <button class="ib sm" aria-label="Move down" title="Move down" disabled={i === order.length - 1} onClick={() => moveOnRail(id, 1)}>
                <ArrowDown size={14} />
              </button>
            </div>
          );
        })}
      </div>
      <div>
        <button class="btn sm" onClick={resetRail}>
          Back to the usual
        </button>
      </div>
    </>
  );
}

function Signing() {
  const d = settings.value.dsc;
  const set = (patch: Partial<Settings["dsc"]>) => put({ dsc: { ...d, ...patch } });
  return (
    <>
      <h3>Signing with a certificate</h3>
      <span class="hint">What a new digital signature starts with (each can be changed while signing, in the Sign pane).</span>
      <Field label="Reason">
        <input class="inp" value={d.reason} onInput={(e) => set({ reason: (e.target as HTMLInputElement).value })} />
      </Field>
      <Field label="Location">
        <input class="inp" value={d.location} placeholder="e.g. Pune" onInput={(e) => set({ location: (e.target as HTMLInputElement).value })} />
      </Field>
      <Field label="The signature">
        <Seg value={d.place} items={[["box", "A box on the page"], ["invisible", "Invisible"]]} set={(place) => set({ place })} />
        <Check on={d.showDate} label="Show the date in the box" set={(showDate) => set({ showDate })} />
        <Check on={d.lock} label="Lock the document after signing" hint="No further changes allowed (certifies it)." set={(lock) => set({ lock })} />
        <Check on={d.newFile} label="Save the signed document as a new file" hint="Otherwise the file itself is signed." set={(newFile) => set({ newFile })} />
      </Field>
    </>
  );
}

function Ocr() {
  const o = settings.value.ocr;
  useEffect(() => {
    if (!ocrLangs.value.length) void loadLangs();
  }, []);
  const langs = ocrLangs.value.filter((l) => hasModel(l, o.quality) || o.langs.includes(l.code));
  const toggle = (code: string, on: boolean) => {
    const next = on ? [...o.langs, code] : o.langs.filter((c) => c !== code);
    if (next.length) rememberChoice(next, o.quality);
  };
  return (
    <>
      <h3>Text recognition (OCR)</h3>
      <span class="hint">The languages and quality Tools › OCR › Recognize text starts with. More languages can be downloaded there.</span>
      <Field label="Quality" hint="Best reads hard scans better, with larger downloads per language.">
        <Seg value={o.quality} items={[["fast", "Fast"], ["best", "Best"]]} set={(q: OcrQuality) => rememberChoice(o.langs, q)} />
      </Field>
      <Field label="Languages" hint={langs.length ? undefined : "Reading the list…"}>
        <div class="set-langs">
          {langs.map((l) => (
            <label key={l.code} class="check-row">
              <input type="checkbox" checked={o.langs.includes(l.code)} onChange={(e) => toggle(l.code, (e.target as HTMLInputElement).checked)} />
              {l.name}
              {!hasModel(l, o.quality) && <small class="muted"> (not downloaded)</small>}
            </label>
          ))}
        </div>
      </Field>
    </>
  );
}

function Office() {
  const pref = settings.value.officeEngine;
  const e = engines.value;
  useEffect(() => {
    void loadEngines();
  }, []);
  const lo = !!(e?.pack || e?.installed);
  const choice = (v: typeof pref, label: string, note: string, disabled = false) => (
    <label class={`check-row office-choice ${disabled ? "off" : ""}`}>
      <input type="radio" name="set-office" checked={pref === v} disabled={disabled} onChange={() => setPreference(v)} />
      <span>
        {label}
        <small>{note}</small>
      </span>
    </label>
  );
  return (
    <>
      <h3>Office files</h3>
      <span class="hint">Word, Excel and PowerPoint files open as PDFs made from them. The program that makes them decides how close they look.</span>
      <Field label="Open Office files with">
        {choice("auto", "The best one here", "Microsoft Office, else LibreOffice, else the text preview")}
        {choice("msoffice", "Microsoft Office", e && !e.msoffice ? "Not installed" : "Word, Excel or PowerPoint", !!e && !e.msoffice)}
        {choice("libreoffice", "LibreOffice", e && !lo ? "Not here yet: get it under Office suite choices" : "Free", !!e && !lo)}
        {choice("builtin", "Text preview", "Quickest, but only the words")}
      </Field>
      <div>
        <button class="btn sm" onClick={() => (officeDialog.value = true)}>
          Office suite choices…
        </button>
      </div>
    </>
  );
}

function Compare() {
  const o = compareOptions();
  return (
    <>
      <h3>Compare</h3>
      <span class="hint">How Tools › Compare starts (each can be switched on the Compare tab's bar).</span>
      <Check on={o.matchCase} label="Capitals count (“Apple” and “apple” differ)" set={(matchCase) => setCompareOption({ matchCase })} />
      <Check on={o.skipEdges} label="Leave out running headers, footers and page numbers" set={(skipEdges) => setCompareOption({ skipEdges })} />
      <Check on={o.pictures} label="Compare drawings and scans too (two PDFs with pages of the same size)" set={(pictures) => setCompareOption({ pictures })} />
    </>
  );
}
