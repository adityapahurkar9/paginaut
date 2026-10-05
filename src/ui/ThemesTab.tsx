// Settings › Themes: the presets and your own themes, light or dark, and the editor for your own (colours, paper,
// a font for headings and buttons, or all of it from a picture). Changes show on the whole window as you make them.
// Below them, the skin: a photo behind the bars and Home.
import { useEffect, useState } from "preact/hooks";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { isDark, MATCH_SYSTEM, onLinux, settings, showToast, updateSettings } from "../store";
import {
  allThemes, chooseTheme, contrastNotes, currentTheme, deleteCustom, fitAdjust, fitProblems, installedFonts, loadFontLists, lookFor, paperNote, PRESETS, previewTheme,
  saveCustom, themeFromPicture, userFonts, type Look, type Palette, type Theme,
} from "../themes";
import { testHook } from "./PagesUI";
import { removeSkin, setSkin, skinFromBytes, skinFromPicture, skinNotes, skinThumb } from "../skin";

const lookOf = (t: Theme) => lookFor(t, isDark.value);
const modesOf = (t: Theme) => (t.light && t.dark ? "Light and dark" : t.dark ? "Dark only" : "Light only");

/** The CSS font list of a theme's font setting (a file's font shows as its name here). */
const fontCss = (d?: string) => (d && !d.startsWith("file:") ? d : undefined);
/** A font scaled as it is on the window, so wide ones fit the preview too. */
const adjusts = new Map<string, number | null>();
function fontStyle(d?: string) {
  const f = fontCss(d);
  if (!f) return {};
  if (!adjusts.has(f)) adjusts.set(f, fitAdjust(f));
  const a = adjusts.get(f);
  return { fontFamily: f, fontSizeAdjust: a ? a.toFixed(3) : undefined };
}

function Preview({ t }: { t: Theme }) {
  const l = lookOf(t);
  const p = l.pal;
  return (
    <div class="tprev" aria-hidden="true">
      <i style={{ background: p.chrome }} />
      <i style={{ background: p.surface, borderBottom: `1px solid ${p.canvas}` }} />
      <div class="tp-body" style={{ background: p.canvas }}>
        <div class="tp-page" style={{ background: p.panel, color: p.tx, ...fontStyle(t.display) }}>
          Aa Bb
          <span style={{ background: p.tx, width: "80%" }} />
          <span style={{ background: p.tx, width: "55%" }} />
        </div>
        <div class="tp-page" style={{ background: l.paper.paper, color: l.paper.ink, flex: 0.6, ...fontStyle(t.display) }}>
          Aa
        </div>
        <i class="tp-btn" style={{ background: p.acc }} />
      </div>
    </div>
  );
}

export function ThemesTab() {
  const s = settings.value;
  const cur = currentTheme();
  const [draft, setDraft] = useState<Theme | null>(null);
  // The picture a new theme's colours came from, as read (it can be the skin too, without reading it again).
  const [pic, setPic] = useState<ArrayBuffer | null>(null);
  // Leaving the tab (or Settings) ends a preview.
  useEffect(() => () => void (previewTheme.value = null), []);
  const single = !cur.light || !cur.dark;
  return (
    <>
      <h3>Themes</h3>
      <span class="hint">Colours, a font for headings and buttons, and the paper of Read and Book modes (when its paper is Auto).</span>
      <div class="tool-field">
        <label>Light or dark</label>
        <div class="seg">
          {([["light", "Light"], ["dark", "Dark"], ["system", MATCH_SYSTEM]] as const).map(([v, label]) => (
            <button key={v} class={`segb ${s.theme === v ? "on" : ""}`} onClick={() => updateSettings({ theme: v })}>
              {label}
            </button>
          ))}
        </div>
        {single && <span class="hint">{cur.name} is {cur.dark ? "dark" : "light"} only.</span>}
      </div>
      <div class="theme-grid">
        {allThemes().map((t) => (
          <button key={t.id} class={`theme-card ${t.id === cur.id ? "on" : ""}`} onClick={() => chooseTheme(t.id)} title={t.blurb}>
            <Preview t={t} />
            <b>{t.name}</b>
            <small>{t.blurb}</small>
            <span class="theme-modes">{t.custom ? `Yours · ${modesOf(t)}` : modesOf(t)}</span>
          </button>
        ))}
      </div>
      {!draft && (
        <div class="theme-row">
          <button class="btn sm" onClick={() => (setPic(null), setDraft(copyOf(cur, !cur.custom)))}>
            {cur.custom ? `Edit ${cur.name}…` : "Make your own palette…"}
          </button>
          <button class="btn sm" onClick={() => void fromPicture((t, bytes) => (setPic(bytes), setDraft(t)))}>
            Make one from a picture…
          </button>
          {cur.custom && (
            <button class="btn ghost sm" onClick={() => deleteCustom(cur.id)}>
              Delete {cur.name}
            </button>
          )}
        </div>
      )}
      {draft && <Editor key={draft.id} start={draft} picture={pic} setPicture={setPic} onDone={() => (setPic(null), setDraft(null))} />}
      <SkinSection />
    </>
  );
}

function SkinSection() {
  const sk = settings.value.skin;
  const notes = skinNotes.value;
  const choose = async () => {
    const path = await pickPicture("A photo for the skin");
    if (!path) return;
    try {
      await skinFromPicture(path);
    } catch (e) {
      showToast(`Couldn't use that picture: ${e instanceof Error ? e.message : e}`, 5000);
    }
  };
  return (
    <>
      <h3 class="set-sub">Skin</h3>
      <span class="hint">A photo behind the title bar, toolbar, side bars and Home, seen through the theme's colours. Menus, panes, dialogs and the pages stay solid.</span>
      <div class="skin-row">
        {sk && skinThumb.value && <img class="skin-thumb" src={skinThumb.value} alt="The skin's photo" />}
        <button class="btn sm" onClick={() => void choose()}>
          {sk ? "Another photo…" : "Choose a photo…"}
        </button>
        {sk && (
          <button class="btn ghost sm" onClick={() => void removeSkin()}>
            No skin
          </button>
        )}
      </div>
      {sk && (
        <div class="tool-field">
          <label class="check-row">
            <input type="checkbox" checked={!sk.off} onChange={(e) => setSkin({ off: !(e.target as HTMLInputElement).checked })} />
            Show the skin
          </label>
          <label class="skin-slider">
            <span>How much shows through</span>
            <input type="range" min={10} max={70} value={Math.round(sk.show * 100)} onInput={(e) => setSkin({ show: +(e.target as HTMLInputElement).value / 100 })} />
            <output>{Math.round(sk.show * 100)}%</output>
          </label>
          <label class="skin-slider">
            <span>Blur</span>
            <input type="range" min={0} max={24} value={sk.blur} onChange={(e) => setSkin({ blur: +(e.target as HTMLInputElement).value })} />
            <output>{sk.blur ? `${sk.blur}px` : "None"}</output>
          </label>
          <label class="check-row">
            <input type="checkbox" checked={sk.pages} onChange={(e) => setSkin({ pages: (e.target as HTMLInputElement).checked })} />
            Also behind the pages
          </label>
          {notes.map((n) => (
            <span key={n} class="theme-warn">
              {n}
            </span>
          ))}
        </div>
      )}
    </>
  );
}

async function pickPicture(title: string): Promise<string | null> {
  const hook = testHook()?.pick;
  const path = hook ? hook.shift() : await open({ multiple: false, title, filters: [{ name: "Pictures", extensions: ["png", "jpg", "jpeg", "jfif", "bmp", "gif", "webp"] }] });
  return typeof path === "string" ? path : null;
}

function copyOf(t: Theme, asNew: boolean): Theme {
  const c: Theme = JSON.parse(JSON.stringify(t));
  if (asNew) {
    c.id = `custom-${Date.now().toString(36)}`;
    // The next free number: "My palette", then "My palette 2", 3… after the highest in use.
    const used = (settings.value.customThemes ?? []).map((x) => /^My palette(?: (\d+))?$/.exec(x.name)).filter((m) => m).map((m) => +(m![1] ?? 1));
    const top = used.length ? Math.max(...used) : 0;
    c.name = top ? `My palette ${top + 1}` : "My palette";
    c.blurb = "Your own colours";
    c.hc = t.hc;
  }
  c.custom = true;
  return c;
}

async function fromPicture(done: (t: Theme, bytes: ArrayBuffer) => void) {
  const path = await pickPicture("A picture to take colours from");
  if (!path) return;
  try {
    const name = path.split(/[\\/]/).pop()!.replace(/\.[^.]+$/, "");
    const bytes = await api.readPicture(path);
    done(await themeFromPicture(bytes, name), bytes);
  } catch (e) {
    showToast(`Couldn't use that picture: ${e instanceof Error ? e.message : e}`, 5000);
  }
}

const COLOURS: [keyof Palette, string][] = [
  ["chrome", "Title bar and side bar"],
  ["surface", "Toolbar and panes"],
  ["panel", "Menus and dialogs"],
  ["canvas", "Behind the pages"],
  ["tx", "Text"],
  ["acc", "Accent"],
];

function Editor({ start, picture, setPicture, onDone }: { start: Theme; picture: ArrayBuffer | null; setPicture: (p: ArrayBuffer | null) => void; onDone: () => void }) {
  const [t, setT] = useState<Theme>(start);
  const [asSkin, setAsSkin] = useState(false);
  const [mode, setMode] = useState<"light" | "dark">(isDark.value && start.dark ? "dark" : start.light ? "light" : "dark");
  const [fit, setFit] = useState<string[]>([]);
  useEffect(() => {
    void loadFontLists();
  }, []);
  // Show it on the whole window while editing; check the labels still fit once it's drawn.
  useEffect(() => {
    previewTheme.value = t;
    const id = window.setTimeout(() => setFit(fitProblems()), 400);
    return () => clearTimeout(id);
  }, [t]);
  const look = t[mode] as Look | undefined;
  const setLook = (patch: Partial<Look>) => setT({ ...t, [mode]: { ...look!, ...patch } });
  const setModeOn = (m: "light" | "dark", on: boolean) => {
    const other = m === "light" ? "dark" : "light";
    if (!on && !t[other]) return;
    // A new mode starts from the standard theme's.
    const next = { ...t, [m]: on ? (t[m] ?? JSON.parse(JSON.stringify(PRESETS[0][m]))) : undefined };
    setT(next);
    setMode(on ? m : other);
  };
  const notes = look ? [...contrastNotes(look.pal), paperNote(look.paper)].filter(Boolean) : [];
  const save = () => {
    previewTheme.value = null;
    saveCustom({ ...t, name: t.name.trim() || "My palette" });
    if (picture && asSkin) skinFromBytes(picture).catch((e) => showToast(`Couldn't use that picture as the skin: ${e instanceof Error ? e.message : e}`, 5000));
    onDone();
  };
  const cancel = () => {
    previewTheme.value = null;
    onDone();
  };
  const addFont = async () => {
    const path = await open({ multiple: false, title: "A font file", filters: [{ name: "Fonts", extensions: ["ttf", "otf", "woff", "woff2"] }] });
    if (typeof path !== "string") return;
    try {
      const f = await api.fontAdd(path);
      await loadFontLists();
      setT({ ...t, display: `file:${f.file}` });
    } catch (e) {
      showToast(String(e), 5000);
    }
  };
  return (
    <div class="theme-edit" onKeyDown={(e) => e.stopPropagation()}>
      <div class="theme-row">
        <input class="inp" style={{ flex: 1 }} value={t.name} aria-label="Theme name" onInput={(e) => setT({ ...t, name: (e.target as HTMLInputElement).value })} />
        <button class="btn sm" onClick={() => void fromPicture((p, bytes) => (setPicture(bytes), setT({ ...p, id: t.id, name: t.name, display: t.display, scope: t.scope })))}>
          Colours from a picture…
        </button>
      </div>
      {picture && (
        <label class="check-row">
          <input type="checkbox" checked={asSkin} onChange={(e) => setAsSkin((e.target as HTMLInputElement).checked)} />
          Use the picture as the skin too (behind the bars and Home)
        </label>
      )}
      <div class="theme-row">
        <label class="check-row">
          <input type="checkbox" checked={!!t.light} onChange={(e) => setModeOn("light", (e.target as HTMLInputElement).checked)} />
          Light version
        </label>
        <label class="check-row">
          <input type="checkbox" checked={!!t.dark} onChange={(e) => setModeOn("dark", (e.target as HTMLInputElement).checked)} />
          Dark version
        </label>
        {t.light && t.dark && (
          <div class="seg" style={{ marginLeft: "auto" }}>
            <button class={`segb ${mode === "light" ? "on" : ""}`} onClick={() => setMode("light")}>
              Editing light
            </button>
            <button class={`segb ${mode === "dark" ? "on" : ""}`} onClick={() => setMode("dark")}>
              Editing dark
            </button>
          </div>
        )}
      </div>
      {look && (
        <div class="theme-colours">
          {COLOURS.map(([k, label]) => (
            <label key={k} class="theme-colour">
              <input type="color" value={look.pal[k]} onInput={(e) => setLook({ pal: { ...look.pal, [k]: (e.target as HTMLInputElement).value } })} />
              {label}
            </label>
          ))}
          <label class="theme-colour">
            <input type="color" value={look.paper.paper} onInput={(e) => setLook({ paper: { ...look.paper, paper: (e.target as HTMLInputElement).value } })} />
            Reading paper
          </label>
          <label class="theme-colour">
            <input type="color" value={look.paper.ink} onInput={(e) => setLook({ paper: { ...look.paper, ink: (e.target as HTMLInputElement).value } })} />
            Reading ink
          </label>
        </div>
      )}
      {notes.map((n) => (
        <span key={n} class="theme-warn">
          {n}
        </span>
      ))}
      <div class="tool-field">
        <label>Font for headings and buttons</label>
        <div class="theme-row">
          <select
            class="inp sm"
            style={{ flex: 1, minWidth: 0 }}
            value={t.display ?? ""}
            onChange={(e) => setT({ ...t, display: (e.target as HTMLSelectElement).value || undefined })}
          >
            <option value="">The usual (Segoe UI)</option>
            {t.display && !t.display.startsWith("file:") && !installedFonts.value.some((f) => `"${f}"` === t.display) && <option value={t.display}>{t.display.split(",")[0].replace(/"/g, "")} (the theme's)</option>}
            {userFonts.value.length > 0 && (
              <optgroup label="Fonts you added">
                {userFonts.value.map((f) => (
                  <option key={f.file} value={`file:${f.file}`}>
                    {f.name}
                  </option>
                ))}
              </optgroup>
            )}
            <optgroup label={onLinux ? "Installed fonts" : "Installed in Windows"}>
              {installedFonts.value.map((f) => (
                <option key={f} value={`"${f}"`}>
                  {f}
                </option>
              ))}
            </optgroup>
          </select>
          <button class="btn sm" onClick={() => void addFont()}>
            Add a font file…
          </button>
        </div>
        <div class="seg" style={{ alignSelf: "flex-start" }}>
          <button class={`segb ${t.scope !== "headings" ? "on" : ""}`} onClick={() => setT({ ...t, scope: "all" })}>
            Headings and buttons
          </button>
          <button class={`segb ${t.scope === "headings" ? "on" : ""}`} onClick={() => setT({ ...t, scope: "headings" })}>
            Headings only
          </button>
        </div>
        <span class="hint">Fonts are scaled to take the room the usual one does; a label that still doesn't fit is cut short with "…".</span>
        {fit.length > 0 && (
          <span class="theme-warn">
            Too wide for some labels ({fit.slice(0, 4).join(", ")}{fit.length > 4 ? "…" : ""}).{" "}
            {t.scope !== "headings" && (
              <button class="linkish" onClick={() => setT({ ...t, scope: "headings" })}>
                Use it for headings only
              </button>
            )}
          </span>
        )}
      </div>
      <div class="theme-row" style={{ justifyContent: "flex-end" }}>
        <button class="btn" onClick={cancel}>
          Cancel
        </button>
        <button class="btn pri" onClick={save}>
          Save and use
        </button>
      </div>
    </div>
  );
}
