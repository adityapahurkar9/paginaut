# Paginaut bugs

Open bugs, newest first. Features and plans stay in ROADMAP.md; a fixed bug is ticked here and its fix is noted under
the release that carries it in ROADMAP.md.

## Open

### Code review, Windows and Linux (found 2026-11-05, not yet seen in use)

Most serious first.

- [ ] Unsaved redactions, text edits, picture edits and Sanitize don't reach search, Read and Book mode or Compare:
      those keep reading the file on disk, so a word just redacted is still found by search and still shows in Read
      mode until the file is saved. redact_apply, edit_text, picture_edit and sanitize never set content_edited
      (page size, stamps, inserted pages and OCR do). (src-tauri/src/engine/mod.rs)
- [ ] A comment that fails halfway (bad picture data, font or icon) stays in the document as an undo step while the
      app shows an error, and the app's Undo gets out of step with the engine's. annot_edit calls end_op instead of
      abandon_op on failure. (src-tauri/src/engine/mod.rs, annot_edit)
- [ ] Linux: Save as onto an existing file whose name differs only in case (Report.pdf → report.pdf) writes the
      changes into Report.pdf and then shows report.pdf unchanged. same_file compares lowercased paths.
      (src-tauri/src/engine/mod.rs, same_file)
- [ ] Closing a tab while its file is still loading (an Office file being converted, a big scan) leaves the document
      open in the engine until Paginaut quits; on Windows the file stays locked. closeTabNow only closes documents
      whose info has arrived. (src/store.ts, closeTabNow and loadTab)
- [ ] Attachments that can run programs are opened: the blocked list only covers Windows types, so .desktop, .sh,
      .run, .AppImage, .py, .pl and .deb open on Linux, and .msix, .appx, .appinstaller, .ps1xml, .settingcontent-ms,
      .jnlp and macro-enabled Office files (.xlam, .ppam, .dotm, .xltm, .potm, .ppsm) open on Windows.
      (src-tauri/src/desk.rs, RISKY)
- [ ] Linux: with report.pdf open, opening Report.pdf (a different file) just shows report.pdf's tab. Paths are
      compared lowercased in about 15 places in the page code, so Recent positions, OCR dismissals and layout fixes
      are shared between such files too. (src/store.ts, openFiles; also shelf.ts, ocr.ts, PagesUI.tsx, App.tsx,
      reflow/overrides.ts, ConvertPane.tsx)
- [ ] "Save a copy as" moves the tab onto the copy: later saves go into the copy, and the original never gets the
      edits. (src/pages.ts, save)
- [ ] "Keep it PDF/A" changes the document before saving; if the save then fails (file locked, disk full) those
      changes stay in memory with no step in the app's history, and the next Undo undoes them instead of your last
      edit. (src/pages.ts, save: pdfaKeep before api.save)
- [ ] Signing a PDF made from an Office file into a new file still treats the signed copy as made from the Office
      file: the tab goes by the Office file's name, and saving or signing in place is refused. sign() doesn't clear
      `converted` as save_with does. (src-tauri/src/engine/mod.rs, sign)
- [ ] Closing a tab may leave copies of its document open on idle worker threads (up to 8 whole documents) until
      some other job arrives: maintain() can wake a worker just before it starts waiting, and the wakeup is lost.
      (src-tauri/src/engine/pool.rs)
- [ ] A crash inside one page of a search or Read mode scan means it never reports finishing: the search keeps
      spinning, and the worker copies aren't let go. (src-tauri/src/engine/pool.rs and scan_pages in engine/mod.rs)
- [ ] Saved signatures can get the same id (added, one deleted and another added within the same second); deleting
      one then deletes both. (src-tauri/src/desk.rs, sig_add)
- [ ] Linux: the default author of comments and redaction marks is empty (user_name reads Windows' USERNAME, not
      USER). (src-tauri/src/app.rs)
- [ ] Redact: the Phone pattern also marks the last 10 digits of longer numbers (account and card numbers): no word
      boundary when there's no +91 or 0 in front. (src/redact.ts, PATTERNS)
- [ ] Windows: a font name like "C:x.ttf" reaches outside the fonts folder (read by font_bytes, deleted by
      font_remove). fonts::file doesn't refuse ':'. (src-tauri/src/fonts.rs)
- [ ] A missing file can stay in Recent: forgetFile compares paths exactly, rememberFile ignores case.
      (src/store.ts)

Smaller cleanups found in the same review:

- [ ] Opening a PDF reads every page just to check whether its page labels are plain 1, 2, 3 (slow on files of
      thousands of pages, and it pushes the viewer's pages out of the cache); check the catalog's /PageLabels first.
      (src-tauri/src/engine/mod.rs, page_labels)
- [ ] Redact patterns with a check (Aadhaar) ask the engine for the text under each hit one at a time: thousands of
      round trips on a long statement. Batch them by page. (src/redact.ts, checkHits)
- [ ] Updates read the whole downloaded installer (up to 400 MB) into memory to check its signature; minisign can
      check it in pieces. (src-tauri/src/update.rs, download)
- [ ] content_edited is set by hand in each editing method, which is how four of them were missed; set it in one
      place where page content changes. (src-tauri/src/engine/mod.rs)
- [ ] Paths are compared with ad-hoc toLowerCase() in about 15 places; one helper that ignores case on Windows only
      would fix the Linux bugs above together. (src/)
- [ ] "Write next to the target, then rename" is written out three times and the copies differ: save_flattened
      writes straight to the target, so a failed flatten leaves half a file. (src-tauri/src/engine/mod.rs: merge,
      save_parts, save_flattened)

### Android (0.6.5)

- [ ] Opening a PDF from another app (Gmail, WhatsApp: Open with › Paginaut) while Paginaut isn't running: the app
      starts and says "This file isn't there any more (moved or deleted)". Opening the same file again from the other
      app, with Paginaut now running, works. Reported 2026-10-31.
      The message is the FileNotFoundException branch of copyThenResolve in
      src-tauri/gen/android/app/src/main/java/app/paginaut/reader/PaginautPlugin.kt; on a cold start the shared
      file's URI is read before it can be opened (or after the grant that came with it no longer applies)

### Windows (0.9.10; Linux not checked yet)

- [ ] Redact: the Redact pane opens on the right over the document, hiding part of the page instead of making
      room for it. Reported 2026-10-31 (src/ui/RedactPanel.tsx)
- [ ] Redact: dragging over text marks a rectangle instead of selecting the text, although the pane says "Drag over
      text to mark it. On scans and pictures, drag a box." Reported 2026-10-31
- [ ] Redact: nothing is drawn while dragging out a box; the mark only shows after the mouse button is let go.
      Reported 2026-10-31

## Fixed

(none yet)
