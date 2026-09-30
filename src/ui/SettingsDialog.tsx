// Settings (Help › Settings, Ctrl+,): every setting in one place, a tab per area.
import { useEffect } from "preact/hooks";
import { Fragment, type ComponentChildren } from "preact";
import { ArrowDown, ArrowUp, X } from "lucide-preact";
import { compareOptions, setCompareOption } from "../compare";
import { DEFAULT_TSA } from "../dsc";
import { deleteModel, downloadModel, downloads, loadLangs, ocrLangs, rememberChoice, stopDownload, type OcrLang, type OcrQuality } from "../ocr";
import { moveOnRail, onRail, openPane, railOrder, railShown, resetRail, setOnRail, setRailShown } from "../panes";
import { settings, settingsOpen, updateSettings, type Settings, type SettingsTab } from "../store";
import { About, Shortcuts } from "./HelpDialogs";
import { OfficeSettings } from "./OfficeUI";
import { PANES } from "./Rail";
import { ThemesTab } from "./ThemesTab";
import { DefaultAppTab } from "./DefaultAppTab";

const TABS: [SettingsTab, string][] = [
  ["general", "General"],
  ["defaults", "Default app"],
  ["themes", "Themes"],
  ["sidebar", "Side bar"],
  ["signing", "Signing"],
  ["ocr", "Text recognition"],
  ["office", "Office files"],
  ["compare", "Compare"],
  ["shortcuts", "Keyboard shortcuts"],
  ["about", "About"],
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
          {tab === "defaults" && <DefaultAppTab />}
          {tab === "themes" && <ThemesTab />}
          {tab === "sidebar" && <SideBar />}
          {tab === "signing" && <Signing />}
          {tab === "ocr" && <Ocr />}
          {tab === "office" && <OfficeSettings />}
          {tab === "compare" && <Compare />}
          {tab === "shortcuts" && <Shortcuts />}
          {tab === "about" && <About />}
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
      <Field label="Look">
        <div>
          <button class="btn sm" onClick={() => (settingsOpen.value = "themes")}>
            Themes…
          </button>
        </div>
      </Field>
      <Field label="Pages">
        <Check on={s.night} label="Dark pages (night mode)" hint="Pages shown light on dark; photos keep their colours. Ctrl+Shift+N" set={(night) => put({ night })} />
        <Check on={s.coverSeparate} label="Cover page on its own in Double page view" set={(coverSeparate) => put({ coverSeparate })} />
        <Check on={s.highlightFields} label="Highlight fillable form fields" set={(highlightFields) => put({ highlightFields })} />
      </Field>
      <Field label="Read and Book modes" hint="Text size, font, spacing, margins and paper are in the Reading pane, next to the text (View › Text).">
        <div>
          <button class="btn sm" onClick={() => { settingsOpen.value = null; openPane("reading"); }}>
            Open the Reading pane
          </button>
        </div>
      </Field>
      <Field label="Saving">
        <Check on={s.autoSaveRotations} label="Save rotations automatically" hint="PDFs only, when nothing else changed." set={(autoSaveRotations) => put({ autoSaveRotations })} />
      </Field>
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
          Default order
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
      <Field label="Trusted timestamp" hint="A timestamp service signs the time with your signature, proving when you signed; the signature stays checkable after your certificate expires. Only a fingerprint of the signature is sent, never the document.">
        <Check on={d.timestamp !== false} label="Add a trusted timestamp (needs internet)" set={(timestamp) => set({ timestamp })} />
        <input class="inp" value={d.tsa ?? ""} placeholder={DEFAULT_TSA} aria-label="Timestamp service address" onInput={(e) => set({ tsa: (e.target as HTMLInputElement).value.trim() })} />
      </Field>
    </>
  );
}

const mb = (n: number) => `${Math.max(1, Math.round(n / 1048576))} MB`;

function Ocr() {
  const o = settings.value.ocr;
  useEffect(() => {
    void loadLangs();
  }, []);
  const all = ocrLangs.value;
  const active = downloads.value;
  // Languages here (built in, downloaded or chosen) first; the rest can be downloaded.
  const mine = all.filter((l) => l.builtin || l.fast || l.best || o.langs.includes(l.code));
  const others = all.filter((l) => !mine.includes(l));
  const toggle = (code: string, on: boolean) => {
    const next = on ? [...o.langs, code] : o.langs.filter((c) => c !== code);
    if (next.length) rememberChoice(next, o.quality);
  };
  const model = (l: OcrLang, q: OcrQuality) => {
    const key = `${l.code}/${q}`;
    const size = q === "best" ? l.bestSize : l.fastSize;
    if (key in active) {
      return (
        <span class="set-dl">
          <span class="rf-bar">
            <span style={{ width: `${Math.round(active[key] * 100)}%` }} />
          </span>
          <button class="linkish" onClick={() => stopDownload(l.code, q)}>
            Stop
          </button>
        </span>
      );
    }
    if (q === "fast" && l.builtin) return <span class="muted">Built in</span>;
    const has = q === "best" ? l.best : l.fast;
    return has ? (
      <button class="linkish danger" title="Delete the downloaded model" onClick={() => void deleteModel(l.code, q)}>
        Remove ({mb(size)})
      </button>
    ) : (
      <button class="linkish" onClick={() => void downloadModel(l.code, q)}>
        Download ({mb(size)})
      </button>
    );
  };
  return (
    <>
      <h3>Text recognition (OCR)</h3>
      <span class="hint">The languages Tools › OCR › Recognize text starts with, and the models downloaded for them (from Tesseract's official models).</span>
      <Field label="Accuracy" hint={o.quality === "fast" ? "Quick and good for clear scans." : "Best for faint or low-quality scans; about 3× slower, larger models."}>
        <Seg value={o.quality} items={[["fast", "Standard"], ["best", "Best"]]} set={(q: OcrQuality) => rememberChoice(o.langs, q)} />
      </Field>
      <div class="tool-field">
        <label>Languages</label>
        {!all.length ? (
          <span class="hint">Reading the list…</span>
        ) : (
          <div class="set-ocr">
            <span class="set-ocr-h">Use</span>
            <span class="set-ocr-h">Language</span>
            <span class="set-ocr-h">Standard</span>
            <span class="set-ocr-h">Best</span>
            {mine.map((l) => (
              <Fragment key={l.code}>
                <input type="checkbox" checked={o.langs.includes(l.code)} aria-label={`Use ${l.name}`} onChange={(e) => toggle(l.code, (e.target as HTMLInputElement).checked)} />
                <span>{l.name}</span>
                <span>{model(l, "fast")}</span>
                <span>{model(l, "best")}</span>
              </Fragment>
            ))}
          </div>
        )}
        {others.length > 0 && (
          <select
            class="inp sm set-ocr-add"
            value=""
            aria-label="Get another language"
            onChange={(e) => {
              const code = (e.target as HTMLSelectElement).value;
              if (code) void downloadModel(code, o.quality);
            }}
          >
            <option value="">Get another language…</option>
            {others.map((l) => (
              <option key={l.code} value={l.code}>
                {l.name} ({mb(o.quality === "best" ? l.bestSize : l.fastSize)})
              </option>
            ))}
          </select>
        )}
        <span class="hint">Each extra language ticked makes recognition slower: tick only the ones your documents use.</span>
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
