// The desktop's colour themes on Android: the same presets (themedata.ts), turned into this stylesheet's
// tokens. Standard is mobile.css's own colours; the others set theirs on <html>.
import { computed, effect } from "@preact/signals";
import { contrast, darkPaperOf, lookFor, mix, rgba, type Palette, type Paper } from "../themedata";
import { isDark, look } from "./state";

/** Every colour mobile.css uses, from a theme's six. */
function tokens(p: Palette, dark: boolean, hc = false): Record<string, string> {
  const onAcc = contrast("#ffffff", p.acc) >= contrast("#111111", p.acc) ? "#ffffff" : "#111111";
  return {
    "--bg": p.surface,
    "--panel": p.panel,
    "--bar": p.panel,
    "--tx": p.tx,
    "--tx2": hc ? p.tx : mix(p.tx, p.surface, 0.7),
    "--tx3": hc ? p.tx : mix(p.tx, p.surface, 0.58),
    "--bd": hc ? p.tx : rgba(p.tx, dark ? 0.08 : 0.1),
    "--bd2": hc ? mix(p.tx, p.panel, 0.5) : rgba(p.tx, dark ? 0.18 : 0.2),
    "--hover": rgba(p.tx, dark ? 0.07 : 0.06),
    "--acc": p.acc,
    "--acc-soft": rgba(p.acc, dark ? 0.16 : 0.12),
    "--on-acc": onAcc,
    "--canvas": p.canvas,
    "--sel": rgba(p.acc, dark ? 0.35 : 0.28),
    // The Open card: the accent, deepened so white reads on it.
    "--card-from": mix(p.acc, "#000000", 0.4),
    "--card-to": mix(p.acc, "#000000", 0.72),
  };
}
const NAMES = Object.keys(tokens({ chrome: "#000", surface: "#000", panel: "#000", canvas: "#000", tx: "#000", acc: "#000" }, false));

/**
 * The display fonts of the desktop's presets are Windows fonts; these are Android's nearest (its own
 * families: Noto Serif, Roboto Condensed, Droid Sans Mono). Headings only, as on the desktop's "headings" scope.
 */
const DISPLAY: Record<string, string> = {
  steampunk: "serif",
  cyberpunk: `"sans-serif-condensed", sans-serif`,
  paper: "serif",
  library: "serif",
  terminal: "monospace",
  blueprint: `"sans-serif-condensed", sans-serif`,
  rose: "serif",
};
export const displayFont = (id: string) => DISPLAY[id] ?? null;

effect(() => {
  const t = look.value;
  const dark = isDark.value;
  const root = document.documentElement;
  const vars = t.id === "standard" ? {} : tokens(lookFor(t, dark).pal, dark, t.hc);
  for (const k of NAMES) root.style.removeProperty(k);
  for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v);
  const font = displayFont(t.id);
  if (font) root.style.setProperty("--font-display", font);
  else root.style.removeProperty("--font-display");
  root.classList.toggle("hc", !!t.hc);
});

/** The theme's paper for Read and Book (Plain paper), light and dark; none for Standard (the usual white and dark). */
export const themePaper = computed<{ light: Paper; dark: Paper } | null>(() => {
  const t = look.value;
  if (t.id === "standard") return null;
  return { light: lookFor(t, false).paper, dark: darkPaperOf(t) };
});

/** The reading view's colours for a theme's paper. */
export function paperVars(p: Paper, dark: boolean): Record<string, string> {
  return { "--rf-paper": p.paper, "--rf-ink": p.ink, "--rf-ink-soft": mix(p.ink, p.paper, 0.55), "--rf-book-desk": mix(p.paper, "#000000", dark ? 0.55 : 0.25) };
}
