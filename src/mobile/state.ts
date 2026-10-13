// The Android viewer's state: settings, the open document and what's shown over it.
import { computed, effect, signal } from "@preact/signals";
import type { DocInfo } from "../api";
import { PRESETS, type Theme as Look } from "../themedata";

// ---------------------------------------------------------------- settings

export type Theme = "system" | "light" | "dark";
/** Pages: scrolled continuously, one at a time (snapping), or two side by side (wide screens). */
export type Layout = "scroll" | "paged" | "double";
/** How a document is read: its pages, or its text reflowed (Read: a scroll; Book: turned pages). */
export type Mode = "pages" | "read" | "book";

/**
 * A tablet: the screen's shorter side is at least 600 dp, Android's own line. Book mode and two pages
 * side by side are for tablets; on a phone they'd only be one more choice for too little room.
 */
export const isTablet = Math.min(screen.width, screen.height) >= 600;

export interface TextSettings {
  size: number;
  font: "serif" | "sans";
  justify: boolean;
  spacing: number;
  margins: "narrow" | "normal" | "wide";
  /** Paper in Read and Book: white (dark at night or in the dark theme), or sepia. */
  paper: "auto" | "sepia";
}

export interface Settings {
  /** Light or dark (a theme with only one of them keeps it). */
  theme: Theme;
  /** The colour theme: one of the desktop's presets (themedata.ts), by id. */
  look: string;
  /** Hide from screenshots: nothing outside Paginaut can capture what it shows (Android's secure window). */
  secure: boolean;
  /** Night mode: pages and text dark, whatever the theme. */
  night: boolean;
  layout: Layout;
  text: TextSettings;
}

const DEFAULTS: Settings = {
  theme: "system",
  look: "standard",
  secure: false,
  night: false,
  layout: "scroll",
  text: { size: 18, font: "serif", justify: false, spacing: 1.55, margins: "normal", paper: "auto" },
};

export function load<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v ? (JSON.parse(v) as T) : fallback;
  } catch {
    return fallback;
  }
}

export function save(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    /* not persisted */
  }
}

const saved = load<Partial<Settings>>("paginaut.m.settings", {});
export const settings = signal<Settings>({ ...DEFAULTS, ...saved, text: { ...DEFAULTS.text, ...saved.text } });

export function updateSettings(patch: Partial<Settings>) {
  settings.value = { ...settings.value, ...patch };
  save("paginaut.m.settings", settings.value);
}

export function updateText(patch: Partial<TextSettings>) {
  updateSettings({ text: { ...settings.value.text, ...patch } });
}

const systemDark = signal(matchMedia("(prefers-color-scheme: dark)").matches);
matchMedia("(prefers-color-scheme: dark)").addEventListener("change", (e) => (systemDark.value = e.matches));

/** Light or dark as chosen (the system's when it follows it), whatever the theme. */
export const wantsDark = computed(() => (settings.value.theme === "system" ? systemDark.value : settings.value.theme === "dark"));

/** The colour theme chosen (Standard if it's gone). */
export const look = computed<Look>(() => PRESETS.find((t) => t.id === settings.value.look) ?? PRESETS[0]);

/** The app's own colours are dark (the theme, or the system's when it follows it; a dark-only or light-only theme decides). */
export const isDark = computed(() => {
  const t = look.value;
  if (!t.light) return true;
  if (!t.dark) return false;
  return wantsDark.value;
});

effect(() => {
  document.documentElement.dataset.theme = isDark.value ? "dark" : "light";
});

// ---------------------------------------------------------------- the open document

export interface OpenDoc {
  info: DocInfo;
  name: string;
  uri: string;
  path: string;
  /** Its record (where you are, bookmarks: records.ts). */
  rec: string;
  /** Read in place (or copied for this visit): let go when it closes. */
  inPlace?: boolean;
}

export const doc = signal<OpenDoc | null>(null);
/** Displayed page (0-based). */
export const page = signal(0);
export const mode = signal<Mode>("pages");
/** How the open document's pages are shown in Pages mode (kept with it: records.ts). */
export const layout = signal<Layout>("paged");
/** The reader's bars are shown (a tap toggles them). */
export const chrome = signal(true);

export type Sheet = "view" | "contents" | "info" | "more" | "settings" | "storage" | "shelf" | null;
export const sheet = signal<Sheet>(null);
export const searching = signal(false);
export const busy = signal<string | null>(null);

// ---------------------------------------------------------------- messages

export const toast = signal<string | null>(null);
let toastTimer = 0;
export function say(msg: string) {
  toast.value = msg;
  clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (toast.value = null), 3200);
}

export function errorText(e: unknown) {
  return String((e as Error)?.message ?? e);
}
