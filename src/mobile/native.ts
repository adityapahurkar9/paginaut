// Android's side (PaginautPlugin.kt, through src-tauri/src/mobile.rs): files, sharing, printing, system bars.
import { invoke } from "@tauri-apps/api/core";

/** A document copied into the app's cache; empty when nothing was picked or sent. */
export interface Picked {
  path?: string;
  name?: string;
  /** Where it came from (content://…), to open it again from Recent. */
  uri?: string;
  /** A big file read in place (or copied for this visit): not kept, so not listed in Recent. */
  direct?: boolean;
}

export const native = {
  pickFile: () => invoke<Picked>("pick_file"),
  /** A file another app sent ("Open with", Share), once. */
  takeIntent: () => invoke<Picked>("take_intent"),
  reopen: (uri: string) => invoke<Picked>("reopen", { uri }),
  /** `source`: where it came from, shared instead when the file is read in place. */
  share: (path: string, source?: string) => invoke<void>("share", { path, source }),
  /** Lets go of a big file read in place once its document has closed. */
  release: (path: string) => invoke<void>("release", { path }).catch(() => {}),
  shareText: (text: string) => invoke<void>("share_text", { text }),
  /** Android's print dialog; documents other than PDFs are written as a PDF first. */
  print: (doc: number, path: string, title: string, pdf: boolean) => invoke<void>("print", { doc, path, title, pdf }),
  setBars: (dark: boolean, immersive: boolean) => invoke<void>("set_bars", { dark, immersive }).catch(() => {}),
  /** Drops cached copies of documents not in `keep` (paths of Recent's copies and the open one). */
  pruneCache: (keep: string[]) => invoke<void>("prune_cache", { keep }).catch(() => {}),
  /** What the page must keep clear of (bars, camera cutout, keyboard), in CSS px. */
  safeArea: () => invoke<{ top: number; right: number; bottom: number; left: number }>("safe_area"),
};
