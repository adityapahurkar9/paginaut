// Themes as data: the colour arithmetic and the presets, shared by the desktop (themes.ts) and the Android
// viewer (mobile/theme.ts). Nothing here touches the page or the settings.

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

export const rgba = (c: string, a: number) => {
  const [r, g, b] = parse(c);
  return `rgba(${r}, ${g}, ${b}, ${a})`;
};

/** WCAG relative luminance of a colour, 0–1. */
export function luminanceRgb([r, g, b]: Rgb) {
  const f = (v: number) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
}

/** WCAG contrast ratio, 1–21, of two luminances. */
export function contrastOfLuminances(a: number, b: number) {
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

/** WCAG contrast ratio, 1–21. */
export function contrast(a: string, b: string) {
  return contrastOfLuminances(luminanceRgb(parse(a)), luminanceRgb(parse(b)));
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
    blurb: "Brass plate, copper and oiled walnut, set in Victorian type",
    display: `"Bookman Old Style", Rockwell, Georgia, serif`,
    light: L("#bf9b62", "#e2cfa6", "#f2e6c8", "#b0936a", "#24170c", "#8a3f12", "#f1e3c4", "#2e1d10"),
    dark: L("#231a13", "#2c2118", "#372a1f", "#17110c", "#f0deb9", "#d6874a", "#2a1f16", "#ecd7b0"),
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
    dark: L("#1e1110", "#281716", "#331f1d", "#150b0a", "#efdcc8", "#d4a537", "#2a1a17", "#e8d5bd"),
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

/** A theme's dark paper: its own, or its light one turned dark (for dark pages with a light theme). */
export function darkPaperOf(t: Theme): Paper {
  if (t.dark) return t.dark.paper;
  const p = t.light!.paper;
  return { paper: mix(p.ink, "#000000", 0.55), ink: mix(p.paper, "#000000", 0.86) };
}

/** A theme's light or dark colours (its only ones, if it has one). */
export function lookFor(t: Theme, dark: boolean): Look {
  return ((dark ? t.dark : t.light) ?? t.dark ?? t.light)!;
}
