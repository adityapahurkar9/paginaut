// Skins: a photo behind the title bar, menu tabs, toolbar, side bars and status bar, and on Home (behind the pages
// too, if ticked). Each bar shows it through its own colour, so labels stay readable; menus, panes, dialogs and the
// pages stay solid. The photo is kept shrunk to 1920 pixels, and its blurred copy is made once whenever the photo or
// the blur changes, so scrolling costs nothing. Nothing stays decoded in between: only the blurred copy (as the
// browser's own image) and a small thumbnail.
import { computed, effect, signal } from "@preact/signals";
import { api } from "./api";
import { settings, updateSettings } from "./store";
import { contrastOfLuminances, lookNow, luminanceRgb, parse, type Rgb } from "./themes";

export interface Skin {
  /** The photo, in the app's skin folder. */
  file: string;
  /** How much of the photo shows through the bars, 0.1–0.7 (Home shows more). */
  show: number;
  /** Blur, in pixels. */
  blur: number;
  /** Behind the pages too. */
  pages: boolean;
  /** Turned off for now (View › Theme). */
  off?: boolean;
}

const MAX = 1920;

/** The photo shown in the Themes tab, and its colours (a few hundred, for the readability check), by file. */
const about = signal<{ file: string; thumb: string; sample: Rgb[] } | null>(null);
export const skinThumb = computed(() => (about.value && about.value.file === settings.value.skin?.file ? about.value.thumb : null));

let url: string | null = null;
let shown = "";
/** Bumped by every show, hide and look-up: a slower one that finishes late sees it has been overtaken. */
let n = 0;

async function decode(file: string) {
  return createImageBitmap(new Blob([await api.skinBytes(file)]));
}

/** The thumbnail and colour sample of a photo (from a decoded copy the caller still owns). */
function describe(file: string, bmp: ImageBitmap) {
  const c = document.createElement("canvas");
  c.width = 24;
  c.height = 16;
  const g = c.getContext("2d")!;
  g.drawImage(bmp, 0, 0, c.width, c.height);
  const d = g.getImageData(0, 0, c.width, c.height).data;
  const sample: Rgb[] = [];
  for (let i = 0; i < d.length; i += 4) sample.push([d[i], d[i + 1], d[i + 2]]);
  const t = document.createElement("canvas");
  t.height = 90;
  t.width = Math.max(1, Math.round((bmp.width / bmp.height) * 90));
  t.getContext("2d")!.drawImage(bmp, 0, 0, t.width, t.height);
  about.value = { file, thumb: t.toDataURL("image/jpeg", 0.8), sample };
}

async function show(skin: Skin) {
  const k = ++n;
  let bmp: ImageBitmap;
  try {
    bmp = await decode(skin.file);
  } catch {
    return;
  }
  if (k !== n) return bmp.close();
  let blob: Blob | null;
  try {
    if (about.value?.file !== skin.file) describe(skin.file, bmp);
    const c = document.createElement("canvas");
    c.width = bmp.width;
    c.height = bmp.height;
    const g = c.getContext("2d")!;
    const r = skin.blur;
    // Drawn a little larger when blurred, so the edges don't fade.
    if (r > 0) g.filter = `blur(${r}px)`;
    g.drawImage(bmp, -2 * r, -2 * r, bmp.width + 4 * r, bmp.height + 4 * r);
    blob = await new Promise<Blob | null>((res) => c.toBlob(res, "image/jpeg", 0.9));
  } finally {
    bmp.close();
  }
  if (k !== n || !blob) return;
  const next = URL.createObjectURL(blob);
  const root = document.documentElement;
  root.style.setProperty("--skin-img", `url("${next}")`);
  root.classList.add("skin");
  if (url) URL.revokeObjectURL(url);
  url = next;
}

/** Only the thumbnail (the skin is off, but the Themes tab still shows which photo it is). */
async function lookUp(file: string) {
  const k = ++n;
  try {
    const bmp = await decode(file);
    if (k === n) describe(file, bmp);
    bmp.close();
  } catch {
    // The Themes tab shows no thumbnail.
  }
}

function hide() {
  n++;
  shown = "";
  const root = document.documentElement;
  root.classList.remove("skin", "skin-pages");
  root.style.removeProperty("--skin-img");
  if (url) URL.revokeObjectURL(url);
  url = null;
}

effect(() => {
  const s = settings.value.skin;
  if (!s || s.off) {
    hide();
    if (!s) about.value = null;
    else if (about.value?.file !== s.file) void lookUp(s.file);
    return;
  }
  const root = document.documentElement;
  root.style.setProperty("--skin-keep", `${Math.round((1 - s.show) * 100)}%`);
  root.style.setProperty("--skin-home-keep", `${Math.round(Math.max(0.12, 0.72 - s.show) * 100)}%`);
  root.style.setProperty("--skin-side-keep", `${Math.round(Math.min(0.97, 1.12 - s.show) * 100)}%`);
  root.classList.toggle("skin-pages", s.pages);
  const key = `${s.file}|${s.blur}`;
  if (key !== shown) {
    shown = key;
    void show(s);
  }
});

/** Photos being made skins, one at a time: the newest one picked wins, older ones still waiting are skipped. */
let making: Promise<unknown> = Promise.resolve();
let picked = 0;

function queue(bytes: () => Promise<ArrayBuffer>): Promise<void> {
  // Numbered when picked, not when read: a small file read quickly doesn't overtake a larger one picked after it.
  const k = ++picked;
  const read = bytes();
  const job = making.then(async () => {
    const b = await read;
    if (k === picked) await makeSkin(b, k);
  });
  making = job.catch(() => {});
  return job;
}

/** Makes a picture the skin (keeping the amount, blur and pages choices of the one before). */
export function skinFromPicture(path: string) {
  return queue(() => api.readPicture(path));
}

/** The same, from a picture already read (the theme editor's). */
export function skinFromBytes(bytes: ArrayBuffer) {
  return queue(async () => bytes);
}

async function makeSkin(bytes: ArrayBuffer, k: number) {
  const bmp = await createImageBitmap(new Blob([bytes]));
  const c = document.createElement("canvas");
  try {
    const f = Math.min(1, MAX / Math.max(bmp.width, bmp.height));
    c.width = Math.max(1, Math.round(bmp.width * f));
    c.height = Math.max(1, Math.round(bmp.height * f));
    c.getContext("2d")!.drawImage(bmp, 0, 0, c.width, c.height);
  } finally {
    bmp.close();
  }
  // A newer photo was picked while this one was shrunk: leave the folder to it.
  if (k !== picked) return;
  const file = await api.skinSet(c.toDataURL("image/jpeg", 0.88));
  const cur = settings.value.skin;
  updateSettings({ skin: { file, show: cur?.show ?? 0.35, blur: cur?.blur ?? 8, pages: cur?.pages ?? false } });
}

export function setSkin(patch: Partial<Skin>) {
  const s = settings.value.skin;
  if (s) updateSettings({ skin: { ...s, ...patch } });
}

export async function removeSkin() {
  updateSettings({ skin: undefined });
  await api.skinClear().catch(() => {});
}

/** Where the bars' labels may be hard to read over the photo (judged at its hardest tenth). */
export const skinNotes = computed((): string[] => {
  const s = settings.value.skin;
  const a = about.value;
  if (!s || s.off || !a || a.file !== s.file) return [];
  const p = lookNow().pal;
  const tx = luminanceRgb(parse(p.tx));
  const keep = 1 - s.show;
  const hardest = (bg: string) => {
    const b = parse(bg);
    const cs = a.sample
      .map((c) => contrastOfLuminances(tx, luminanceRgb([0, 1, 2].map((i) => b[i] * keep + c[i] * (1 - keep)) as Rgb)))
      .sort((x, y) => x - y);
    return cs[Math.floor(cs.length * 0.1)];
  };
  return hardest(p.surface) < 4.5 || hardest(p.chrome) < 4.5
    ? ["Low contrast: labels on the bars may be hard to read over this photo. Show less of it, or blur it more."]
    : [];
});
