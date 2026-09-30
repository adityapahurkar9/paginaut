// Themes: colours, a font for headings and buttons, and the paper of Read and Book modes. A theme gives six base
// colours per mode (light, dark or both); every other shade of the interface is worked out from them. The
// user's own themes start from a preset or from a picture's colours.
import { effect, signal } from "@preact/signals";
import { api } from "./api";
import { isDark, settings, updateSettings } from "./store";

// ---------------------------------------------------------------- colours

export type Rgb = [number, number, number];

export function parse(hex: string): Rgb {
  const h = hex.replace("#", "");
  const n = parseInt(h.length === 3 ? h.split("").map((c) => c + c).join("") : h.slice(0, 6), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

export const toHex = ([r, g, b]: Rgb) => `#${[r, g, b].map((v) => Math.round(Math.max(0, Math.min(255, v))).toString(16).padStart(2, "0")).join("")}`;

/** `a` mixed into `b`: t = 1 is all `a`. */
export function mix(a: string, b: string, t: number) {
  const [x, y] = [parse(a), parse(b)];
  return toHex([0, 1, 2].map((i) => x[i] * t + y[i] * (1 - t)) as Rgb);
}

const rgba = (c: string, a: number) => {
  const [r, g, b] = parse(c);
  return `rgba(${r}, ${g}, ${b}, ${a})`;
};

function luminance(c: string) {
  const f = (v: number) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  const [r, g, b] = parse(c);
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
}

/** WCAG contrast ratio, 1–21. */
export function contrast(a: string, b: string) {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

export function hsl(h: number, s: number, l: number) {
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  return toHex([f(0) * 255, f(8) * 255, f(4) * 255]);
}

export function toHsl(c: string): [number, number, number] {
  const [r, g, b] = parse(c).map((v) => v / 255);
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  const h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return [h * 60, s, l];
}

// ---------------------------------------------------------------- themes

/** The base colours of one mode. */
export interface Palette {
  /** Title bar and side bar. */
  chrome: string;
  /** Toolbar, menu tabs, pane backgrounds. */
  surface: string;
  /** Dialogs, menus, cards. */
  panel: string;
  /** Behind the pages. */
  canvas: string;
  /** Text. */
  tx: string;
  /** Accent: selected things, main buttons, links. */
  acc: string;
}

/** Read and Book modes' paper and ink. */
export interface Paper {
  paper: string;
  ink: string;
}

export interface Look {
  pal: Palette;
  paper: Paper;
}

export interface Theme {
  id: string;
  name: string;
  blurb: string;
  light?: Look;
  dark?: Look;
  /** Font for headings, tab names and buttons (a CSS font list), or none for the usual. */
  display?: string;
  /** "headings": the font only for headings and titles (too wide or ornate for buttons). */
  scope?: "all" | "headings";
  /** High contrast: pure colours, thick outlines, larger text. */
  hc?: boolean;
  /** Made by the user. */
  custom?: boolean;
}

const L = (chrome: string, surface: string, panel: string, canvas: string, tx: string, acc: string, paper: string, ink: string): Look => ({
  pal: { chrome, surface, panel, canvas, tx, acc },
  paper: { paper, ink },
});

export const PRESETS: Theme[] = [
  {
    id: "standard",
    name: "Standard",
    blurb: "Paginaut's own warm greys and blue",
    light: L("#e9e7e3", "#f7f6f3", "#ffffff", "#dedcd7", "#1d1c1a", "#2b58d1", "#fbfaf6", "#22211e"),
    dark: L("#17171a", "#1f1f23", "#26262b", "#121214", "#ececef", "#8aa6ff", "#1d1d20", "#d9d6ce"),
  },
  {
    id: "steampunk",
    name: "Steampunk",
    blurb: "Brass, copper and dark walnut, with verdigris",
    display: `"Palatino Linotype", "Book Antiqua", Georgia, serif`,
    light: L("#d6bf98", "#e8d8b8", "#f6ecd6", "#b89c74", "#2e1f14", "#2f7361", "#f1e4c6", "#3a2717"),
    dark: L("#1c140e", "#281d15", "#33261b", "#130d09", "#ecd9b8", "#c8923f", "#2b2017", "#e6d2ae"),
  },
  {
    id: "cyberpunk",
    name: "Cyberpunk",
    blurb: "Neon magenta and cyan on near-black",
    display: `Bahnschrift, "Segoe UI", sans-serif`,
    dark: L("#08080f", "#0f0f1b", "#161627", "#05050a", "#e8f4ff", "#ff2bd6", "#0d1224", "#bfeeff"),
  },
  {
    id: "nord",
    name: "Nord",
    blurb: "Arctic blue-greys and frost",
    light: L("#d8dee9", "#e5e9f0", "#eceff4", "#c5cedb", "#2e3440", "#5e81ac", "#f2f4f8", "#2e3440"),
    dark: L("#242933", "#2e3440", "#3b4252", "#1f232b", "#eceff4", "#88c0d0", "#2e3440", "#d8dee9"),
  },
  {
    id: "solarized",
    name: "Solarized",
    blurb: "Ethan Schoonover's calm base tones",
    light: L("#eee8d5", "#f5efdc", "#fdf6e3", "#ddd4b8", "#073642", "#268bd2", "#fdf6e3", "#073642"),
    dark: L("#00212b", "#002b36", "#073642", "#001a21", "#eee8d5", "#2aa198", "#002b36", "#c5cfcf"),
  },
  {
    id: "forest",
    name: "Forest",
    blurb: "Moss, pine and bark, with amber",
    display: `Candara, Corbel, "Segoe UI", sans-serif`,
    light: L("#d9e2d0", "#e6ecdf", "#f3f6ee", "#c1cdb4", "#1f2a1c", "#9a6512", "#f1f5ec", "#1f2a1c"),
    dark: L("#121a13", "#182219", "#1f2b20", "#0c120d", "#dfe8d8", "#e0a53a", "#16201a", "#d7e2cf"),
  },
  {
    id: "ocean",
    name: "Ocean",
    blurb: "Deep sea blues, teal and sand",
    light: L("#d4e6ee", "#e2eff4", "#f2f8fa", "#b7d0dc", "#0f2a3a", "#0b7f7f", "#eef7f9", "#0f2a3a"),
    dark: L("#07161f", "#0b1f2b", "#112a38", "#041018", "#d8eef5", "#3cc6c0", "#0a1c28", "#cfe7ef"),
  },
  {
    id: "paper",
    name: "Paper",
    blurb: "Ink on paper, with a red pencil",
    display: `"Sitka Heading", "Sitka Text", Georgia, serif`,
    light: L("#ebe7de", "#f4f1ea", "#fbf9f4", "#dad4c8", "#1a1a1a", "#b3261e", "#f8f4ea", "#1c1b19"),
  },
  {
    id: "library",
    name: "Library",
    blurb: "Leather, oxblood and gold leaf",
    display: `"Book Antiqua", "Palatino Linotype", Garamond, Georgia, serif`,
    light: L("#d9c6ad", "#e8d9c4", "#f5ecdf", "#b89c7e", "#2b1a12", "#7d1f1f", "#f3e6cc", "#3b2a1c"),
    dark: L("#1c120d", "#251811", "#2f1f16", "#140c08", "#ecdcc5", "#d4a537", "#2a1d14", "#e5d3b5"),
  },
  {
    id: "midnight",
    name: "Midnight",
    blurb: "Indigo night with a violet glow",
    dark: L("#0d0f1f", "#12152b", "#191d38", "#080a16", "#e4e6ff", "#9d7cff", "#10142a", "#d9dcf7"),
  },
  {
    id: "terminal",
    name: "Terminal",
    blurb: "Phosphor green and amber on black",
    display: `"Cascadia Mono", "Cascadia Code", Consolas, monospace`,
    dark: L("#040704", "#0a0f0a", "#0f160f", "#020402", "#b9f6c0", "#ffb000", "#050805", "#7dff9a"),
  },
  {
    id: "blueprint",
    name: "Blueprint",
    blurb: "Engineer's blue, white lines, orange marks",
    display: `Bahnschrift, "Segoe UI", sans-serif`,
    light: L("#cfdcef", "#dfe8f5", "#eef3fa", "#a8bedd", "#0f2a5c", "#c75a14", "#eef3fa", "#0f2a5c"),
    dark: L("#0b2a55", "#0f3366", "#153d77", "#082147", "#eaf2ff", "#ff8c42", "#0f3366", "#e6efff"),
  },
  {
    id: "sakura",
    name: "Sakura",
    blurb: "Cherry blossom pink and plum",
    display: `Corbel, "Segoe UI", sans-serif`,
    light: L("#f5dfe6", "#fae9ee", "#fff5f8", "#e8c7d2", "#3b1f2b", "#b23d6e", "#fff6f8", "#3b1f2b"),
    dark: L("#1f121a", "#2a1822", "#341e2a", "#170c12", "#f6e3ea", "#f08ab4", "#2a1822", "#f2dde6"),
  },
  {
    id: "rose",
    name: "Rosé",
    blurb: "Dusty rose, wine and gold",
    display: `Constantia, "Palatino Linotype", Georgia, serif`,
    light: L("#ecd6d3", "#f4e4e1", "#fbf2f0", "#d8bbb6", "#3a1e24", "#8e2c48", "#fbeff0", "#3a1e24"),
    dark: L("#1c1013", "#26161a", "#311c22", "#140a0d", "#f3dfe0", "#d6ae5c", "#2c1a1f", "#efd9db"),
  },
  {
    id: "desert",
    name: "Desert",
    blurb: "Sand and terracotta, with cactus green",
    display: `Candara, Corbel, "Segoe UI", sans-serif`,
    light: L("#ead9bf", "#f2e5d0", "#faf2e5", "#d6bd98", "#3a2616", "#46703a", "#f7ecd9", "#3a2616"),
    dark: L("#1d140c", "#281c12", "#332418", "#140d07", "#f0dfc6", "#d7774a", "#2a1e14", "#ecd9bd"),
  },
  {
    id: "contrast",
    name: "High contrast",
    blurb: "Pure black and white, strong outlines, larger text",
    hc: true,
    light: L("#ffffff", "#ffffff", "#ffffff", "#c8c8c8", "#000000", "#0000c8", "#ffffff", "#000000"),
    dark: L("#000000", "#000000", "#000000", "#1c1c1c", "#ffffff", "#ffe600", "#000000", "#ffffff"),
  },
];

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

/** Every colour of the interface from the six base colours. */
export function tokens(p: Palette, dark: boolean, hc = false): Record<string, string> {
  const onAcc = contrast("#ffffff", p.acc) >= contrast("#111111", p.acc) ? "#ffffff" : "#111111";
  return {
    "--chrome": p.chrome,
    "--surface": p.surface,
    "--panel": p.panel,
    "--canvas": p.canvas,
    "--bd": hc ? p.tx : mix(p.tx, p.panel, dark ? 0.17 : 0.16),
    "--bd2": hc ? mix(p.tx, p.panel, 0.5) : mix(p.tx, p.panel, dark ? 0.11 : 0.1),
    "--tx": p.tx,
    "--tx2": hc ? p.tx : mix(p.tx, p.surface, 0.7),
    "--tx3": hc ? p.tx : mix(p.tx, p.surface, 0.58),
    "--hover": rgba(p.tx, dark ? 0.08 : 0.06),
    "--active": rgba(p.tx, dark ? 0.13 : 0.1),
    "--acc": p.acc,
    "--acc-soft": mix(p.acc, p.panel, dark ? 0.24 : 0.14),
    "--on-acc": onAcc,
    "--sel": rgba(p.acc, dark ? 0.35 : 0.28),
  };
}

/** Where the colours of a palette are hard to read: a note per problem. */
export function contrastNotes(p: Palette): string[] {
  const notes: string[] = [];
  if (contrast(p.tx, p.panel) < 4.5 || contrast(p.tx, p.surface) < 4.5) notes.push("Low contrast: text on the panels may be hard to read.");
  if (contrast(p.acc, p.panel) < 2.5) notes.push("Low contrast: the accent is hard to see on the panels.");
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

/** A theme's dark paper: its own, or its light one turned dark (for dark pages with a light theme). */
function darkPaperOf(t: Theme): Paper {
  if (t.dark) return t.dark.paper;
  const p = t.light!.paper;
  return { paper: mix(p.ink, "#000000", 0.55), ink: mix(p.paper, "#000000", 0.86) };
}
/** The theme being shown before it's chosen (the editor's preview). */
export const previewTheme = signal<Theme | null>(null);

let applying = 0;
async function apply(t: Theme, systemIsDark: boolean) {
  const n = ++applying;
  const dark = darkFor(t, systemIsDark);
  const look = (dark ? t.dark : t.light) ?? t.dark ?? t.light!;
  const root = document.documentElement;
  root.dataset.theme = dark ? "dark" : "light";
  isDark.value = dark;
  root.classList.toggle("hc", !!t.hc);
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
