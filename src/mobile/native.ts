// Android's side (PaginautPlugin.kt, through src-tauri/src/mobile.rs): files, sharing, printing, system bars.
import { invoke } from "@tauri-apps/api/core";

/** A document copied into the app's cache; empty when nothing was picked or sent. */
export interface Picked {
  path?: string;
  name?: string;
  /** Where it came from (content://…), to open it again from Recent. */
  uri?: string;
}

export const native = {
  pickFile: () => invoke<Picked>("pick_file"),
  /** A file another app sent ("Open with", Share), once. */
  takeIntent: () => invoke<Picked>("take_intent"),
  reopen: (uri: string) => invoke<Picked>("reopen", { uri }),
  share: (path: string) => invoke<void>("share", { path }),
  /** Android's print dialog; documents other than PDFs are written as a PDF first. */
  print: (doc: number, path: string, title: string, pdf: boolean) => invoke<void>("print", { doc, path, title, pdf }),
  setBars: (color: string, dark: boolean, immersive: boolean) => invoke<void>("set_bars", { color, dark, immersive }).catch(() => {}),
};
