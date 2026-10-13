// Themes: colours, a font for headings and buttons, and the paper of Read and Book modes. A theme gives six base
// colours per mode (light, dark or both); every other shade of the interface is worked out from them. The
// user's own themes start from a preset or from a picture's colours.
import { effect, signal } from "@preact/signals";
import { api } from "./api";
import { isDark, settings, updateSettings } from "./store";
import { contrast, darkPaperOf, hsl, lookFor, PRESETS, toHex, toHsl, tokens, type Look, type Palette, type Paper, type Rgb, type Theme } from "./themedata";

export * from "./themedata";

export function allThemes(): Theme[] {
  return [...PRESETS, ...(settings.value.customThemes ?? [])];
}

export function currentTheme(): Theme {
  const id = settings.value.look ?? "standard";
  return allThemes().find((t) => t.id === id) ?? PRESETS[0];
}

/** Light or dark, as chosen, unless the theme has only one. */
export function darkFor(t: Theme, systemDark: boolean) {
  if (!t.light) return true;
  if (!t.dark) return false;
  const m = settings.value.theme;
  return m === "dark" || (m === "system" && systemDark);
}


/** Where the colours of a palette are hard to read: a note per problem. */
export function contrastNotes(p: Palette): string[] {
  const notes: string[] = [];
  if (contrast(p.tx, p.panel) < 4.5 || contrast(p.tx, p.surface) < 4.5) notes.push("Low contrast: text on the panels may be hard to read.");
  if (contrast(p.acc, p.panel) < 2.5) notes.push("Low contrast: the accent is hard to see on the panels.");
  if (contrast(p.tx, p.canvas) < 3) notes.push("Low contrast: messages shown behind the pages (while a file opens) may be hard to read.");
  const on = contrast("#ffffff", p.acc) >= contrast("#111111", p.acc) ? "#ffffff" : "#111111";
  if (contrast(on, p.acc) < 3) notes.push("Low contrast: labels on accent buttons may be hard to read.");
  return notes;
}

export function paperNote(p: Paper) {
  return contrast(p.paper, p.ink) < 4.5 ? "Low contrast: reading text on this paper may be hard." : "";
}

// ---------------------------------------------------------------- fonts

const REF = `"Segoe UI Variable Text", "Segoe UI", sans-serif`;
const SAMPLE = "Home View Comment Tools Organize Convert Settings";
let canvas: HTMLCanvasElement | null = null;
const ctx = () => (canvas ??= document.createElement("canvas")).getContext("2d")!;

function metrics(font: string) {
  const c = ctx();
  c.font = `100px ${font}`;
  const x = c.measureText("x");
  return { xh: x.actualBoundingBoxAscent / 100, w: c.measureText(SAMPLE).width / 100 };
}

/** The first family of a CSS font list that is really there (its letters differ from the fallbacks'). */
export function firstInstalled(list: string): string | null {
  const c = ctx();
  const fams = list.split(",").map((f) => f.trim()).filter((f) => !["serif", "sans-serif", "monospace", "system-ui", "cursive"].includes(f));
  for (const f of fams) {
    const differs = ["monospace", "serif"].some((fb) => {
      c.font = `100px ${fb}`;
      const a = c.measureText(SAMPLE).width;
      c.font = `100px ${f}, ${fb}`;
      return Math.abs(c.measureText(SAMPLE).width - a) > 0.5;
    });
    if (differs) return f;
  }
  return null;
}

/**
 * How much to scale a font so it fits where the usual one does: the same x-height as Segoe UI, a little smaller
 * still when its letters are wide. As a font-size-adjust value (x-height as a fraction of the font size).
 */
export function fitAdjust(font: string) {
  const ref = metrics(REF);
  const m = metrics(font);
  if (!m.xh || !ref.xh) return null;
  const widthAtRef = m.w * (ref.xh / m.xh);
  const f = widthAtRef > ref.w * 1.1 ? (ref.w * 1.1) / widthAtRef : 1;
  return Math.max(0.34, Math.min(0.6, ref.xh * f));
}

/** Fonts added from files: loaded into the page once, by name. */
const loaded = new Map<string, string>();
export const userFonts = signal<{ name: string; file: string }[]>([]);
export const installedFonts = signal<string[]>([]);

export async function loadFontLists() {
  const [a, b] = await Promise.all([api.fontsInstalled().catch(() => []), api.fontsUser().catch(() => [])]);
  installedFonts.value = a;
  userFonts.value = b;
}

/** A font the user added, ready to use: its CSS family name. */
export async function userFontFamily(file: string): Promise<string | null> {
  if (loaded.has(file)) return loaded.get(file)!;
  try {
    const bytes = await api.fontBytes(file);
    const family = `Pg User ${file.replace(/\.[^.]+$/, "")}`;
    const face = new FontFace(family, bytes);
    await face.load();
    document.fonts.add(face);
    loaded.set(file, `"${family}"`);
    return `"${family}"`;
  } catch {
    return null;
  }
}

/** A theme's display font as a CSS list, a user font ("file:NAME") loaded first. */
async function displayList(t: Theme): Promise<string | null> {
  const d = t.display;
  if (!d) return null;
  if (d.startsWith("file:")) {
    const fam = await userFontFamily(d.slice(5));
    return fam ? `${fam}, ${REF}` : null;
  }
  return firstInstalled(d) ? `${d}, ${REF}` : null;
}

/** Labels that don't fit with the current fonts (toolbar, tabs, side bar, pane titles). */
export function fitProblems(): string[] {
  const out: string[] = [];
  const els = document.querySelectorAll<HTMLElement>(".rtab, .tb > span, .rail-btn span, .signpane-head strong, .set-tab");
  for (const el of els) {
    if (!el.offsetParent) continue;
    const vertical = el.closest(".rail-btn");
    const over = vertical ? el.scrollHeight > el.clientHeight + 1 : el.scrollWidth > el.clientWidth + 1;
    if (over && el.textContent) out.push(el.textContent.trim());
  }
  return [...new Set(out)];
}

// ---------------------------------------------------------------- applying

const systemDark = matchMedia("(prefers-color-scheme: dark)");
const systemDarkNow = signal(systemDark.matches);
systemDark.addEventListener("change", () => (systemDarkNow.value = systemDark.matches));

/** The paper of Read and Book modes' "Auto", from the theme (null: the usual white or dark). */
export const themePaper = signal<{ now: Paper; dark: boolean; darkPaper: Paper } | null>(null);

/** The theme being shown before it's chosen (the editor's preview). */
export const previewTheme = signal<Theme | null>(null);


/** The colours showing now (light or dark; the preview's, if any). */
export function lookNow(): Look {
  return lookFor(previewTheme.value ?? currentTheme(), isDark.value);
}

let applying = 0;
async function apply(t: Theme, systemIsDark: boolean) {
  const n = ++applying;
  const dark = darkFor(t, systemIsDark);
  const look = lookFor(t, dark);
  const root = document.documentElement;
  root.dataset.theme = dark ? "dark" : "light";
  isDark.value = dark;
  root.classList.toggle("hc", !!t.hc);
  // A preset's own touches (a brass sheen, a blueprint's grid, a terminal's scan lines): look-<id> in the stylesheet.
  for (const c of [...root.classList]) if (c.startsWith("look-")) root.classList.remove(c);
  if (!t.custom) root.classList.add(`look-${t.id}`);
  // The standard theme is the stylesheet's own colours.
  const vars = t.id === "standard" ? {} : tokens(look.pal, dark, t.hc);
  for (const k of Object.keys(TOKEN_NAMES)) root.style.removeProperty(k);
  for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v);
  themePaper.value = t.id === "standard" ? null : { now: look.paper, dark, darkPaper: darkPaperOf(t) };
  const list = await displayList(t);
  if (n !== applying) return;
  if (list) {
    root.style.setProperty("--font-display", list);
    const adj = fitAdjust(list);
    if (adj) root.style.setProperty("--fd-adjust", adj.toFixed(3));
    else root.style.removeProperty("--fd-adjust");
  } else {
    root.style.removeProperty("--font-display");
    root.style.removeProperty("--fd-adjust");
  }
  root.classList.toggle("fd-headings", t.scope === "headings");
}

const TOKEN_NAMES = Object.fromEntries(Object.keys(tokens(PRESETS[0].light!.pal, false)).map((k) => [k, 1]));

effect(() => {
  void settings.value.theme;
  void settings.value.look;
  void settings.value.customThemes;
  const t = previewTheme.value ?? currentTheme();
  void apply(t, systemDarkNow.value);
});

export function chooseTheme(id: string) {
  updateSettings({ look: id });
}

export function saveCustom(t: Theme) {
  const list = (settings.value.customThemes ?? []).filter((x) => x.id !== t.id);
  updateSettings({ customThemes: [...list, { ...t, custom: true }], look: t.id });
}

export function deleteCustom(id: string) {
  const list = (settings.value.customThemes ?? []).filter((x) => x.id !== id);
  updateSettings({ customThemes: list, look: settings.value.look === id ? "standard" : settings.value.look });
}

// ---------------------------------------------------------------- a palette from a picture

/** A theme from a picture's colours: its main colour for the background, its liveliest for the accent. */
export async function themeFromPicture(bytes: ArrayBuffer, name: string): Promise<Theme> {
  const bmp = await createImageBitmap(new Blob([bytes]));
  const size = 72;
  const c = document.createElement("canvas");
  c.width = c.height = size;
  const g = c.getContext("2d")!;
  g.drawImage(bmp, 0, 0, size, size);
  bmp.close();
  const data = g.getImageData(0, 0, size, size).data;
  const px: Rgb[] = [];
  for (let i = 0; i < data.length; i += 4) if (data[i + 3] > 128) px.push([data[i], data[i + 1], data[i + 2]]);
  if (!px.length) throw new Error("The picture has no colours to use.");
  // k-means, six colours, started from evenly spread pixels.
  const k = Math.min(6, px.length);
  let centres: Rgb[] = Array.from({ length: k }, (_, i) => px[Math.floor(((i + 0.5) * px.length) / k)]);
  let counts: number[] = [];
  for (let iter = 0; iter < 10; iter++) {
    const sums = centres.map(() => [0, 0, 0]);
    counts = centres.map(() => 0);
    for (const p of px) {
      let best = 0, bd = Infinity;
      centres.forEach((c, j) => {
        const d = (p[0] - c[0]) ** 2 + (p[1] - c[1]) ** 2 + (p[2] - c[2]) ** 2;
        if (d < bd) (bd = d), (best = j);
      });
      counts[best]++;
      for (let q = 0; q < 3; q++) sums[best][q] += p[q];
    }
    centres = centres.map((c, j) => (counts[j] ? (sums[j].map((v) => v / counts[j]) as Rgb) : c));
  }
  const colours = centres.map((c, j) => ({ hex: toHex(c), n: counts[j] / px.length })).filter((c) => c.n > 0.01);
  const main = colours.slice().sort((a, b) => b.n - a.n)[0];
  const lively = colours.slice().sort((a, b) => toHsl(b.hex)[1] * Math.sqrt(b.n) - toHsl(a.hex)[1] * Math.sqrt(a.n))[0];
  const [h, s] = toHsl(main.hex);
  let [ah, as] = toHsl(lively.hex);
  // A grey picture: an accent of its own hue, livelier.
  if (as < 0.25) (ah = h), (as = 0.55);
  const bs = Math.min(s, 0.3);
  const light: Palette = {
    chrome: hsl(h, bs * 0.8, 0.87),
    surface: hsl(h, bs * 0.7, 0.93),
    panel: hsl(h, bs * 0.5, 0.975),
    canvas: hsl(h, bs * 0.8, 0.8),
    tx: hsl(h, 0.25, 0.12),
    acc: hsl(ah, Math.max(as, 0.5), 0.4),
  };
  for (let l = 0.4; contrast(light.acc, light.panel) < 3 && l > 0.15; l -= 0.04) light.acc = hsl(ah, Math.max(as, 0.5), l);
  const dark: Palette = {
    chrome: hsl(h, bs * 0.7, 0.09),
    surface: hsl(h, bs * 0.7, 0.12),
    panel: hsl(h, bs * 0.6, 0.16),
    canvas: hsl(h, bs * 0.6, 0.065),
    tx: hsl(h, 0.2, 0.91),
    acc: hsl(ah, Math.max(as, 0.55), 0.64),
  };
  return {
    id: `custom-${Date.now().toString(36)}`,
    name,
    blurb: "From a picture",
    custom: true,
    light: { pal: light, paper: { paper: hsl(h, bs * 0.4, 0.96), ink: light.tx } },
    dark: { pal: dark, paper: { paper: hsl(h, bs * 0.4, 0.13), ink: hsl(h, 0.15, 0.86) } },
  };
}
