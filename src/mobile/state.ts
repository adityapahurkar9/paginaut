// The Android viewer's state: settings, recent files, the open document and what's shown over it.
import { computed, effect, signal } from "@preact/signals";
import type { DocInfo } from "../api";

// ---------------------------------------------------------------- settings

export type Theme = "system" | "light" | "dark";
/** Pages: scrolled continuously, one at a time (snapping), or two side by side (wide screens). */
export type Layout = "scroll" | "paged" | "double";
/** How a document is read: its pages, or its text reflowed (Read: a scroll; Book: turned pages). */
export type Mode = "pages" | "read" | "book";

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
  theme: Theme;
  /** Night mode: pages and text dark, whatever the theme. */
  night: boolean;
  layout: Layout;
  text: TextSettings;
}

const DEFAULTS: Settings = {
  theme: "system",
  night: false,
  layout: "scroll",
  text: { size: 18, font: "serif", justify: false, spacing: 1.55, margins: "normal", paper: "auto" },
};

function load<T>(key: string, fallback: T): T {
  try {
    const v = localStorage.getItem(key);
    return v ? (JSON.parse(v) as T) : fallback;
  } catch {
    return fallback;
  }
}

function save(key: string, v: unknown) {
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

/** The app's own colours are dark (the theme, or the system's when it follows it). */
export const isDark = computed(() => (settings.value.theme === "system" ? systemDark.value : settings.value.theme === "dark"));

effect(() => {
  document.documentElement.dataset.theme = isDark.value ? "dark" : "light";
});

// ---------------------------------------------------------------- recent files

export interface Recent {
  /** Where the file came from (content://…); the key. */
  uri: string;
  name: string;
  /** Its copy in the cache (may have been cleared by Android). */
  path: string;
  format: string;
  pages: number;
  /** Where you were: page, fraction of it scrolled past, and the way you read it. */
  page: number;
  frac: number;
  mode: Mode;
  at: number;
}

const MAX_RECENT = 60;
export const recent = signal<Recent[]>(load<Recent[]>("paginaut.m.recent", []));

export function remember(r: Recent) {
  recent.value = [r, ...recent.value.filter((x) => x.uri !== r.uri)].slice(0, MAX_RECENT);
  save("paginaut.m.recent", recent.value);
}

export function forget(uri: string) {
  recent.value = recent.value.filter((x) => x.uri !== uri);
  save("paginaut.m.recent", recent.value);
}

export function clearRecent() {
  recent.value = [];
  save("paginaut.m.recent", []);
}

// ---------------------------------------------------------------- the open document

export interface OpenDoc {
  info: DocInfo;
  name: string;
  uri: string;
  path: string;
}

export const doc = signal<OpenDoc | null>(null);
/** Displayed page (0-based). */
export const page = signal(0);
export const mode = signal<Mode>("pages");
/** The reader's bars are shown (a tap toggles them). */
export const chrome = signal(true);

export type Sheet = "view" | "contents" | "info" | "more" | "settings" | null;
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
