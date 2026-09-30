// Your saved signatures and initials (encrypted on disk by the app, per Windows
// user), and placing them on a page. A placed signature is a PDF stamp with the
// picture as its appearance: Chrome and Adobe show it, and it can be moved and
// resized until you save a locked copy.
import { effect, signal } from "@preact/signals";
import { invoke } from "@tauri-apps/api/core";
import { annotTool, editAnnot, registerPlacer, selectWhenShown } from "./annots";
import { activeTab, settings, showToast, type Tab } from "./store";
import { openPane } from "./panes";

export type SigKind = "signature" | "initials";

export interface SigEntry {
  id: string;
  kind: SigKind;
  name: string;
  created: number;
  /** Base64 PNG, transparent background, cropped to the ink. */
  png: string;
}

/** Saved entries (null until loaded). */
export const sigs = signal<SigEntry[] | null>(null);

/** A picture dropped from Explorer while the dialog is open (its path). */
export const droppedPicture = signal<string | null>(null);

/** The Sign pane is making a new signature or initials (shown in its "My signatures" section). */
export const creating = signal<SigKind | null>(null);

/** Section of the Sign pane to bring into view. */
export type PaneSection = "mine" | "marks" | "dsc" | "doc";
export const paneFocus = signal<{ section: PaneSection; n: number } | null>(null);

/** Opens the Sign pane, optionally scrolled to a section. */
export function openSignPane(section?: PaneSection) {
  openPane("sign");
  if (section) paneFocus.value = { section, n: (paneFocus.value?.n ?? 0) + 1 };
}

/** Opens the pane at "My signatures" with the maker open. */
export function newSig(kind: SigKind) {
  creating.value = kind;
  openSignPane("mine");
}

export async function loadSigs() {
  try {
    sigs.value = await invoke<SigEntry[]>("sig_list");
  } catch (e) {
    sigs.value = [];
    showToast(String(e));
  }
}

export async function addSig(kind: SigKind, name: string, png: string) {
  const e = await invoke<SigEntry>("sig_add", { kind, name, png });
  sigs.value = [...(sigs.value ?? []), e];
  return e;
}

export async function deleteSig(id: string) {
  await invoke("sig_delete", { id });
  sigs.value = (sigs.value ?? []).filter((e) => e.id !== id);
}

export const sigsOf = (kind: SigKind) => (sigs.value ?? []).filter((e) => e.kind === kind);

/** Width and height of a PNG, from its header. */
export function pngSize(b64: string): [number, number] {
  const head = atob(b64.slice(0, 44));
  const u32 = (o: number) => ((head.charCodeAt(o) << 24) | (head.charCodeAt(o + 1) << 16) | (head.charCodeAt(o + 2) << 8) | head.charCodeAt(o + 3)) >>> 0;
  return [u32(16), u32(20)];
}

/** Size on the page in points: a signature about half an inch tall, initials a bit smaller. */
export function sigSizePt(e: SigEntry): [number, number] {
  const [w, h] = pngSize(e.png);
  const aspect = w / Math.max(1, h);
  let ph = e.kind === "initials" ? 26 : 38;
  let pw = ph * aspect;
  const max = e.kind === "initials" ? 90 : 190;
  if (pw > max) {
    pw = max;
    ph = pw / aspect;
  }
  return [pw, ph];
}

/** Picks a saved entry to place with the next click on a page. */
export function pickSig(id: string) {
  annotTool.value = annotTool.value === `sig:${id}` ? null : `sig:${id}`;
}

/** Places a saved entry centred on `at` (page points) and selects it for moving and resizing. */
export async function placeSig(tab: Tab, src: number, id: string, at: [number, number]) {
  const e = sigs.value?.find((s) => s.id === id);
  if (!e) return;
  const [w, h] = sigSizePt(e);
  const [x, y] = at;
  const n = await editAnnot(tab, src, {
    kind: "Stamp",
    props: { rect: [x - w / 2, y - h / 2, x + w / 2, y + h / 2], image: e.png, contents: e.kind === "initials" ? "Initials" : "Signature" },
  });
  if (n !== null) selectWhenShown(tab, src, n);
}

// While a signature is picked, a faint copy follows the pointer over the pages at its real size.
let ghost: HTMLImageElement | null = null;
function moveGhost(ev: PointerEvent) {
  const tool = annotTool.value;
  const v = activeTab.value?.viewer;
  const pg = (ev.target as Element | null)?.closest?.<HTMLElement>(".pg");
  const e = tool?.startsWith("sig:") ? sigs.value?.find((s) => s.id === tool.slice(4)) : null;
  if (!ghost || !v || !pg || !e) {
    if (ghost) ghost.style.display = "none";
    return;
  }
  const view = Number(pg.dataset.index);
  const a = v.clientToPage(view, ev.clientX, ev.clientY);
  const b = v.clientToPage(view, ev.clientX + 100, ev.clientY);
  if (!a || !b) return;
  const scale = 100 / Math.max(1e-3, Math.hypot(b[0] - a[0], b[1] - a[1]));
  const [w, h] = sigSizePt(e);
  ghost.style.display = "block";
  ghost.style.width = `${w * scale}px`;
  ghost.style.height = `${h * scale}px`;
  ghost.style.transform = `translate(${ev.clientX - (w * scale) / 2}px, ${ev.clientY - (h * scale) / 2}px)`;
}

effect(() => {
  const tool = annotTool.value;
  const e = tool?.startsWith("sig:") ? sigs.value?.find((s) => s.id === tool.slice(4)) : null;
  if (e) {
    if (!ghost) {
      ghost = document.createElement("img");
      ghost.className = "sig-ghost";
      document.body.append(ghost);
      window.addEventListener("pointermove", moveGhost);
    }
    ghost.src = `data:image/png;base64,${e.png}`;
    // Drawn like the pages in night mode.
    ghost.classList.toggle("night", settings.value.night);
    ghost.style.display = "none";
  } else if (ghost) {
    window.removeEventListener("pointermove", moveGhost);
    ghost.remove();
    ghost = null;
  }
});

// ---------------------------------------------------------------- making the picture

export const INK = [
  ["#16181d", "Black"],
  ["#1b3fa6", "Blue"],
  ["#0b2a6f", "Dark blue"],
] as const;

/** Handwriting fonts that come with Windows. */
export const SCRIPT_FONTS = ["Segoe Script", "Ink Free", "Lucida Handwriting", "Gabriola"];

/** Crops a canvas to its ink (non-transparent pixels) plus a small margin; base64 PNG, or null if empty. */
export function cropToInk(c: HTMLCanvasElement, pad = 6): string | null {
  const ctx = c.getContext("2d")!;
  const { width: W, height: H } = c;
  const d = ctx.getImageData(0, 0, W, H).data;
  let x0 = W, y0 = H, x1 = -1, y1 = -1;
  for (let y = 0; y < H; y++)
    for (let x = 0; x < W; x++)
      if (d[(y * W + x) * 4 + 3] > 8) {
        if (x < x0) x0 = x;
        if (x > x1) x1 = x;
        if (y < y0) y0 = y;
        if (y > y1) y1 = y;
      }
  if (x1 < 0) return null;
  x0 = Math.max(0, x0 - pad);
  y0 = Math.max(0, y0 - pad);
  x1 = Math.min(W - 1, x1 + pad);
  y1 = Math.min(H - 1, y1 + pad);
  const out = document.createElement("canvas");
  out.width = x1 - x0 + 1;
  out.height = y1 - y0 + 1;
  out.getContext("2d")!.drawImage(c, x0, y0, out.width, out.height, 0, 0, out.width, out.height);
  return out.toDataURL("image/png").split(",")[1];
}

/** Renders typed text in a handwriting font, large enough to stay sharp when printed. */
export function typedCanvas(text: string, font: string, ink: string): HTMLCanvasElement {
  const size = 120;
  const c = document.createElement("canvas");
  const ctx = c.getContext("2d")!;
  const css = `${size}px "${font}"`;
  ctx.font = css;
  const m = ctx.measureText(text);
  c.width = Math.ceil(m.width + size);
  c.height = Math.ceil(size * 2);
  ctx.font = css;
  ctx.fillStyle = ink;
  ctx.textBaseline = "middle";
  ctx.fillText(text, size / 2, c.height / 2);
  return c;
}

/**
 * A photo of a signature on paper: finds the ink with Otsu's threshold on the
 * grey levels, makes the paper transparent (with soft edges), and recolours the
 * ink, or keeps its own colour when `ink` is null. `bias` (−1…1) moves the cut:
 * positive keeps fainter strokes, negative drops more of the paper's shading.
 */
export function inkFromPhoto(img: CanvasImageSource & { width: number; height: number }, ink: string | null, bias = 0): HTMLCanvasElement {
  const max = 1600;
  const s = Math.min(1, max / Math.max(img.width, img.height));
  const W = Math.max(1, Math.round(img.width * s));
  const H = Math.max(1, Math.round(img.height * s));
  const c = document.createElement("canvas");
  c.width = W;
  c.height = H;
  const ctx = c.getContext("2d", { willReadFrequently: true })!;
  ctx.drawImage(img, 0, 0, W, H);
  const id = ctx.getImageData(0, 0, W, H);
  const d = id.data;
  const grey = new Uint8Array(W * H);
  const hist = new Array(256).fill(0);
  for (let i = 0; i < W * H; i++) {
    const g = Math.round(0.299 * d[i * 4] + 0.587 * d[i * 4 + 1] + 0.114 * d[i * 4 + 2]);
    grey[i] = g;
    hist[g]++;
  }
  // Otsu: the grey level that best separates ink from paper.
  let sum = 0;
  for (let i = 0; i < 256; i++) sum += i * hist[i];
  let sumB = 0, wB = 0, best = 0, t = 128;
  for (let i = 0; i < 256; i++) {
    wB += hist[i];
    if (!wB) continue;
    const wF = W * H - wB;
    if (!wF) break;
    sumB += i * hist[i];
    const mB = sumB / wB, mF = (sum - sumB) / wF;
    const between = wB * wF * (mB - mF) * (mB - mF);
    if (between > best) {
      best = between;
      t = i;
    }
  }
  t = Math.max(10, Math.min(245, t + bias * 60));
  const soft = 18; // grey levels over which an edge fades out
  const rgb = ink ? [1, 3, 5].map((i) => parseInt(ink.slice(i, i + 2), 16)) : null;
  for (let i = 0; i < W * H; i++) {
    const a = Math.max(0, Math.min(1, (t + soft / 2 - grey[i]) / soft));
    const o = i * 4;
    if (rgb) {
      d[o] = rgb[0];
      d[o + 1] = rgb[1];
      d[o + 2] = rgb[2];
    }
    d[o + 3] = Math.round(a * 255);
  }
  // Drop specks: pixels with almost no ink around them.
  const alpha = new Uint8Array(W * H);
  for (let i = 0; i < W * H; i++) alpha[i] = d[i * 4 + 3];
  for (let y = 1; y < H - 1; y++)
    for (let x = 1; x < W - 1; x++) {
      const i = y * W + x;
      if (!alpha[i]) continue;
      let n = 0;
      for (let dy = -1; dy <= 1; dy++) for (let dx = -1; dx <= 1; dx++) if ((dx || dy) && alpha[i + dy * W + dx] > 64) n++;
      if (n < 2) d[i * 4 + 3] = 0;
    }
  ctx.putImageData(id, 0, 0);
  return c;
}

registerPlacer("sig", placeSig);
