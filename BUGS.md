# Paginaut bugs

Open bugs, newest first. Features and plans stay in ROADMAP.md; a fixed bug is ticked here and its fix is noted under
the release that carries it in ROADMAP.md.

## Open

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
