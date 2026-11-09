# Paginaut bugs

Open bugs, newest first. Features and plans stay in ROADMAP.md; a fixed bug is ticked here and its fix is noted under
the release that carries it in ROADMAP.md.

## Open

(none)

## Fixed

### Found while fixing the code review (2026-11-09, Windows and Linux; fixed the same day, next release 0.9.11)

- [x] A save that fails after it started writing (disk full, the target locked by another program) throws away every
      unsaved change in the engine: save_with reopens the file from disk on failure, because the page order and
      rotations were already applied to the document in memory. The tab still says it has changes and its Undo
      steps point at nothing; what's shown is the file as it was last saved. Needs the document kept as it was
      (apply page edits to a copy, or only when the save can't fail any more). (src-tauri/src/engine/mod.rs, save_with)
      Fixed: what the save changes in the document (page order and rotations, the page tree, smaller pictures) is one
      step of the undo journal, taken back when writing fails, so the document and its undo steps are as before.
      Full writes are made from a copy of the document opened on its own (MuPDF's full write tidies the document it
      writes, which couldn't be taken back), Save as writes beside the target first, and an in-place save checks
      the file can be written before starting. If the file can't be replaced at the very end, the saved copy beside
      it is kept and the document goes on from it ("your changes are kept… save again"). After a failed save the
      tab may still count as changed after undoing everything (an extra "Save changes?" at worst), and Redo is
      cleared. save_test: Save as into a folder, in-place saves on a file another program holds, then a save that
      works

### Code review, Windows and Linux (found 2026-11-05; fixed 2026-11-09, next release 0.9.11)

Most serious first.

- [x] Unsaved redactions, text edits, picture edits and Sanitize don't reach search, Read and Book mode or Compare:
      those keep reading the file on disk, so a word just redacted is still found by search and still shows in Read
      mode until the file is saved. redact_apply, edit_text, picture_edit and sanitize never set content_edited
      (page size, stamps, inserted pages and OCR do). (src-tauri/src/engine/mod.rs)
      Fixed: every edit of page content goes through one method, Doc::content_changed (see the cleanup below).
      redact_test checks that a search no longer finds removed text before saving
- [x] A comment that fails halfway (bad picture data, font or icon) stays in the document as an undo step while the
      app shows an error, and the app's Undo gets out of step with the engine's. annot_edit calls end_op instead of
      abandon_op on failure. (src-tauri/src/engine/mod.rs, annot_edit)
      Fixed: abandon_op on failure. redact_test checks a stamp with a bad picture leaves no change behind
- [x] Linux: Save as onto an existing file whose name differs only in case (Report.pdf → report.pdf) writes the
      changes into Report.pdf and then shows report.pdf unchanged. same_file compares lowercased paths.
      (src-tauri/src/engine/mod.rs, same_file)
      Fixed: Windows still ignores case; Linux compares the files themselves (device and inode), so the same file
      reached another way (a link, a case-insensitive disk) is still known
- [x] Closing a tab while its file is still loading (an Office file being converted, a big scan) leaves the document
      open in the engine until Paginaut quits; on Windows the file stays locked. closeTabNow only closes documents
      whose info has arrived. (src/store.ts, closeTabNow and loadTab)
      Fixed: a closed tab is marked; loadTab closes a document that arrives for one
- [x] Attachments that can run programs are opened: the blocked list only covers Windows types, so .desktop, .sh,
      .run, .AppImage, .py, .pl and .deb open on Linux, and .msix, .appx, .appinstaller, .ps1xml, .settingcontent-ms,
      .jnlp and macro-enabled Office files (.xlam, .ppam, .dotm, .xltm, .potm, .ppsm) open on Windows.
      (src-tauri/src/desk.rs, RISKY)
      Fixed: those and more (Windows script hosts, Access databases, .xlsb, Linux scripts, packages and launchers);
      one list on both systems
- [x] Linux: with report.pdf open, opening Report.pdf (a different file) just shows report.pdf's tab. Paths are
      compared lowercased in about 15 places in the page code, so Recent positions, OCR dismissals and layout fixes
      are shared between such files too. (src/store.ts, openFiles; also shelf.ts, ocr.ts, PagesUI.tsx, App.tsx,
      reflow/overrides.ts, ConvertPane.tsx)
      Fixed: store.ts pathKey/samePath (case ignored on Windows only) used in all of them, and in appwindows.rs for
      "open in another window". Layout fixes saved on Linux before are still found under their old key
- [x] "Save a copy as" moves the tab onto the copy: later saves go into the copy, and the original never gets the
      edits. (src/pages.ts, save)
      Fixed: the copy is written from the document in memory (save_parts) and the tab stays on its file, edits still
      to save there. A copy over the file itself is an ordinary save; one over a file open in another tab is
      refused. The buttons and menus now say "Save a copy" (they said "Save as", which reads as moving)
- [x] "Keep it PDF/A" changes the document before saving; if the save then fails (file locked, disk full) those
      changes stay in memory with no step in the app's history, and the next Undo undoes them instead of your last
      edit. (src/pages.ts, save: pdfaKeep before api.save)
      Fixed: the fixes are a step in the tab's history as soon as they're made, so Undo stays in step whatever the
      save does (see the open bug above for what a failed save does to the rest)
- [x] Signing a PDF made from an Office file into a new file still treats the signed copy as made from the Office
      file: the tab goes by the Office file's name, and saving or signing in place is refused. sign() doesn't clear
      `converted` as save_with does. (src-tauri/src/engine/mod.rs, sign)
      Fixed: sign() clears it after signing into a new file
- [x] Closing a tab may leave copies of its document open on idle worker threads (up to 8 whole documents) until
      some other job arrives: maintain() can wake a worker just before it starts waiting, and the wakeup is lost.
      (src-tauri/src/engine/pool.rs)
      Fixed: maintain() (and shutdown) change the epoch under the queue's lock, which workers hold from checking it
      to waiting
- [x] A crash inside one page of a search or Read mode scan means it never reports finishing: the search keeps
      spinning, and the worker copies aren't let go. (src-tauri/src/engine/pool.rs and scan_pages in engine/mod.rs)
      Fixed: each page's work runs guarded (engine::guarded); a fault becomes that page's error, so the job ends
      and says so. Text recognition's runners too. A file that can't be opened again for a scan is an error now,
      not an empty finished scan
- [x] Saved signatures can get the same id (added, one deleted and another added within the same second); deleting
      one then deletes both. (src-tauri/src/desk.rs, sig_add)
      Fixed: ids from the time in nanoseconds, made unique among the saved ones
- [x] Linux: the default author of comments and redaction marks is empty (user_name reads Windows' USERNAME, not
      USER). (src-tauri/src/app.rs)
      Fixed: USERNAME, then USER, then LOGNAME
- [x] Redact: the Phone pattern also marks the last 10 digits of longer numbers (account and card numbers): no word
      boundary when there's no +91 or 0 in front. (src/redact.ts, PATTERNS)
      Fixed: a word boundary in front when there's no prefix
- [x] Windows: a font name like "C:x.ttf" reaches outside the fonts folder (read by font_bytes, deleted by
      font_remove). fonts::file doesn't refuse ':'. (src-tauri/src/fonts.rs)
      Fixed: only one plain file name is taken (no ':', no folders, no drive or stream), and the path must sit
      directly in the fonts folder
- [x] A missing file can stay in Recent: forgetFile compares paths exactly, rememberFile ignores case.
      (src/store.ts)
      Fixed: both use samePath

Smaller cleanups found in the same review:

- [x] Opening a PDF reads every page just to check whether its page labels are plain 1, 2, 3 (slow on files of
      thousands of pages, and it pushes the viewer's pages out of the cache); check the catalog's /PageLabels first.
      (src-tauri/src/engine/mod.rs, page_labels)
      Done: no /PageLabels, no labels; otherwise they're worked out from the label ranges (label_of, unit-tested)
      without loading a page
- [x] Redact patterns with a check (Aadhaar) ask the engine for the text under each hit one at a time: thousands of
      round trips on a long statement. Batch them by page. (src/redact.ts, checkHits)
      Done: area_texts reads a page's text once for all its hits (fo_area_texts_json)
- [x] Updates read the whole downloaded installer (up to 400 MB) into memory to check its signature; minisign can
      check it in pieces. (src-tauri/src/update.rs, download)
      Done: update::verify_file checks it in 1 MB pieces (old-style signatures, which sign the bytes themselves,
      are still read whole); tested with the release key's test file
- [x] content_edited is set by hand in each editing method, which is how four of them were missed; set it in one
      place where page content changes. (src-tauri/src/engine/mod.rs)
      Done: Doc::content_changed; doc_step takes whether a step changes page content
- [x] Paths are compared with ad-hoc toLowerCase() in about 15 places; one helper that ignores case on Windows only
      would fix the Linux bugs above together. (src/)
      Done: store.ts pathKey/samePath
- [x] "Write next to the target, then rename" is written out three times and the copies differ: save_flattened
      writes straight to the target, so a failed flatten leaves half a file. (src-tauri/src/engine/mod.rs: merge,
      save_parts, save_flattened)
      Done: one write_beside, used by all three

### Android (0.6.5; fixed 2026-11-09 for the next Android release, 0.6.6; not yet checked on a phone)

- [x] Opening a PDF from another app (Gmail, WhatsApp: Open with › Paginaut) while Paginaut isn't running: the app
      starts and says "This file isn't there any more (moved or deleted)". Opening the same file again from the other
      app, with Paginaut now running, works. Reported 2026-10-31.
      The message is the FileNotFoundException branch of copyThenResolve in
      src-tauri/gen/android/app/src/main/java/app/paginaut/reader/PaginautPlugin.kt; on a cold start the shared
      file's URI is read before it can be opened (or after the grant that came with it no longer applies)
      Fixed (in code): the sent file is read as soon as Android hands it over (PaginautPlugin.load / onNewIntent start
      the copy at once), not seconds later when the page asks; and a "not found" from the sending app's provider is
      asked again once, 0.7 s later, before it's believed. To check on a phone: Gmail and WhatsApp, Paginaut closed

### Windows (0.9.10; fixed 2026-11-09, next release 0.9.11)

- [x] Redact: the Redact pane opens on the right over the document, hiding part of the page instead of making
      room for it. Reported 2026-10-31 (src/ui/RedactPanel.tsx)
      Fixed: Redact is a pane like Sign and Convert (src/ui/RedactPane.tsx): the page makes room, and it has a
      button on the side bar. Another pane taking the place ends redacting; another PDF tab coming forward takes it
      over
- [x] Redact: dragging over text marks a rectangle instead of selecting the text, although the pane says "Drag over
      text to mark it. On scans and pictures, drag a box." Reported 2026-10-31
      Found: dragging over real text marks the text (checked on text-1000.pdf); a box is marked where the letters are
      part of the picture, as in Maharashtra Times.pdf (a web page saved with its text as shapes), which looks like
      text but has none to select. Fixed: the first time that happens Paginaut says so and how to mark words (OCR
      first), the pane's hint says it, and the box shows while dragging (next entry)
- [x] Redact: nothing is drawn while dragging out a box; the mark only shows after the mouse button is let go.
      Reported 2026-10-31
      Fixed: the box is drawn as you drag whenever it's what will be marked (no text under the drag, or "Mark boxes
      even over text" on); over text the selection shows as before
