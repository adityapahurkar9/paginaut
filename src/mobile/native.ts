// Android's side (PaginautPlugin.kt, through src-tauri/src/mobile.rs): files, sharing, printing, system bars.
import { invoke } from "@tauri-apps/api/core";
import type { BookMeta, ShelfItem } from "./shelf";

/** A document ready for the engine; empty when nothing was picked or sent. */
export interface Picked {
  path?: string;
  name?: string;
  /** Where it came from (content://…, or a path the shelf found), to open it again. */
  uri?: string;
  /** Read in place (or copied for this visit): let go once it closes. */
  inPlace?: boolean;
  /** Its source can be opened again whenever (picked here, in a shelf folder, a path with all-files access). */
  lasting?: boolean;
  /** `path` is a copy kept in the cache. */
  kept?: boolean;
  /** Its contents' fingerprint ("" if it couldn't be read). */
  fp?: string;
}

/** What the app keeps and can reach. */
export interface Storage {
  /** Copies of files lent by other apps: when each was last opened (ms). */
  copies: { path: string; name: string; size: number; at: number }[];
  /** Files picked in Paginaut (it can open them again). */
  picked: string[];
  /** Folders chosen for the shelf. */
  folders: { uri: string; name: string }[];
  allFiles: boolean;
}

export const native = {
  pickFile: () => invoke<Picked>("pick_file"),
  /** A file another app sent ("Open with", Share), once. */
  takeIntent: () => invoke<Picked>("take_intent"),
  reopen: (uri: string) => invoke<Picked>("reopen", { uri }),
  /** `source`: where it came from, shared instead when the file is read in place. */
  share: (path: string, source?: string) => invoke<void>("share", { path, source }),
  /** Lets go of a file read in place once its document has closed. */
  release: (path: string) => invoke<void>("release", { path }).catch(() => {}),
  shareText: (text: string) => invoke<void>("share_text", { text }),
  /** Android's print dialog; documents other than PDFs are written as a PDF first. */
  print: (doc: number, path: string, title: string, pdf: boolean) => invoke<void>("print", { doc, path, title, pdf }),
  setBars: (dark: boolean, immersive: boolean) => invoke<void>("set_bars", { dark, immersive }).catch(() => {}),
  storage: () => invoke<Storage>("storage"),
  /** Deletes these copies from the cache. */
  dropCopies: (paths: string[]) => invoke<void>("drop_copies", { paths }).catch(() => {}),
  /** Asks for a folder for the shelf (empty if none was chosen). */
  addFolder: () => invoke<{ uri?: string; name?: string }>("add_folder"),
  removeFolder: (uri: string) => invoke<void>("remove_folder", { uri }),
  /** Shows where "All files access" is turned on or off. */
  allFilesAccess: () => invoke<void>("all_files_access"),
  scanBooks: () => invoke<{ items: ShelfItem[] }>("scan_books"),
  /** Title, author and pages of a book, and its cover made (by its fingerprint). */
  bookCover: (path: string, fp: string) => invoke<BookMeta>("book_cover", { path, fp }),
  /** What the page must keep clear of (bars, camera cutout, keyboard), in CSS px. */
  safeArea: () => invoke<{ top: number; right: number; bottom: number; left: number }>("safe_area"),
};
