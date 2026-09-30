// Skins: a photo behind the title bar, menu tabs, toolbar, side bars and status bar, and on Home (behind the pages
// too, if ticked). Each bar shows it through its own colour, so labels stay readable; menus, panes, dialogs and the
// pages stay solid. The photo is kept shrunk to 1920 pixels, and its blurred copy is made once whenever the photo or
// the blur changes, so scrolling costs nothing.
import { effect, signal } from "@preact/signals";
import { api } from "./api";
import { settings, updateSettings } from "./store";
import { contrast, lookNow, mix, toHex, type Rgb } from "./themes";

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

/** The photo's colours, a few hundred of them (for the readability check). */
export const skinSample = signal<Rgb[] | null>(null);
/** The photo, small, for the Themes tab. */
export const skinThumb = signal<string | null>(null);

let photo: { file: string; bmp: ImageBitmap } | null = null;
let url: string | null = null;
let shown = "";
let n = 0;

async function source(file: string) {
  if (photo?.file === file) return photo.bmp;
  const bmp = await createImageBitmap(new Blob([await api.skinBytes(file)]));
  photo?.bmp.close();
  photo = { file, bmp };
  const c = document.createElement("canvas");
  c.width = 24;
  c.height = 16;
  const g = c.getContext("2d")!;
  g.drawImage(bmp, 0, 0, c.width, c.height);
  const d = g.getImageData(0, 0, c.width, c.height).data;
  const px: Rgb[] = [];
  for (let i = 0; i < d.length; i += 4) px.push([d[i], d[i + 1], d[i + 2]]);
  skinSample.value = px;
  const t = document.createElement("canvas");
  t.height = 90;
  t.width = Math.round((bmp.width / bmp.height) * 90);
  t.getContext("2d")!.drawImage(bmp, 0, 0, t.width, t.height);
  skinThumb.value = t.toDataURL("image/jpeg", 0.8);
  return bmp;
}

async function show(skin: Skin) {
  const k = ++n;
  let bmp: ImageBitmap;
  try {
    bmp = await source(skin.file);
  } catch {
    return;
  }
  if (k !== n) return;
  const c = document.createElement("canvas");
  c.width = bmp.width;
  c.height = bmp.height;
  const g = c.getContext("2d")!;
  const r = skin.blur;
  // Drawn a little larger when blurred, so the edges don't fade.
  if (r > 0) g.filter = `blur(${r}px)`;
  g.drawImage(bmp, -2 * r, -2 * r, bmp.width + 4 * r, bmp.height + 4 * r);
  const blob = await new Promise<Blob | null>((res) => c.toBlob(res, "image/jpeg", 0.9));
  if (k !== n || !blob) return;
  const next = URL.createObjectURL(blob);
  const root = document.documentElement;
  root.style.setProperty("--skin-img", `url("${next}")`);
  root.classList.add("skin");
  if (url) URL.revokeObjectURL(url);
  url = next;
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
    if (!s) {
      photo?.bmp.close();
      photo = null;
      skinSample.value = skinThumb.value = null;
    }
    return hide();
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

/** Makes a picture the skin (keeping the amount, blur and pages choices of the one before). */
export async function skinFromPicture(path: string) {
  const bmp = await createImageBitmap(new Blob([await api.readPicture(path)]));
  const f = Math.min(1, MAX / Math.max(bmp.width, bmp.height));
  const c = document.createElement("canvas");
  c.width = Math.max(1, Math.round(bmp.width * f));
  c.height = Math.max(1, Math.round(bmp.height * f));
  c.getContext("2d")!.drawImage(bmp, 0, 0, c.width, c.height);
  bmp.close();
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
export function skinNotes(): string[] {
  const s = settings.value.skin;
  const px = skinSample.value;
  if (!s || s.off || !px) return [];
  const p = lookNow().pal;
  const hardest = (bg: string, keep: number) => {
    const cs = px.map((c) => contrast(p.tx, mix(bg, toHex(c), keep))).sort((a, b) => a - b);
    return cs[Math.floor(cs.length * 0.1)];
  };
  return hardest(p.surface, 1 - s.show) < 4.5 || hardest(p.chrome, 1 - s.show) < 4.5
    ? ["Low contrast: labels on the bars may be hard to read over this photo. Show less of it, or blur it more."]
    : [];
}
