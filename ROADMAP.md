# Folio roadmap

## Phase 1: Viewer core (done)
Native MuPDF engine with a worker pool, virtual scrolling, previews then sharp renders, tiles at deep zoom,
thumbnails and outline, search, text selection and copy, links, password PDFs, themes and night mode,
command palette, tabs and recent files. Installer 9.2 MB.

## Phase 2: Page tools, and feedback from Phase 1 testing (done)
- [x] Rotate and delete from the page itself: hover buttons, R / Shift+R / Del, multi-select in thumbnails
      (Ctrl/Shift+click)
- [x] Drag to reorder pages in the thumbnails (with drop marker and auto-scroll)
- [x] Undo / redo (Ctrl+Z / Ctrl+Y, toolbar buttons)
- [x] Ctrl+S saves only the changes (incremental: 0.7 s and +376 bytes for a 254 MB scan); Ctrl+Shift+S
      Save as (full clean copy); non-PDF files save as a new PDF; files MuPDF had to repair are rewritten
      safely through a temporary file. Unsaved dot on tabs, Save button, "Save changes?" on closing a tab
      or the window, optional auto-save for rotations (command palette)
- [x] Memory during search: each search now frees its private document copies as soon as it ends and uses at
      most 8 threads. NSE prospectus, 6 searches: 1,061 MB → 477 MB peak, settles ~440 MB (was 988 MB, never
      released). Status bar now reports private memory (like Task Manager), not working sets
- [x] Window close (X) and Alt+F4 work (missing "destroy window" permission)
- [x] Minimize/restore animation: checked, it already animates
- [x] Press feedback on all buttons
- [x] Trackpad pinch-zoom scales pages smoothly during the gesture, then re-renders sharp; animated zoom steps
- [x] Side-to-side scrolling limited to the width of the pages near you (one wide page elsewhere no longer adds
      empty space)
- [x] Command palette search: results list with page numbers and highlighted snippets; match case, whole word
      and regex toggles (also in the find bar)
- [x] Book mode: single page, two pages, two pages with separate cover
- [x] Page labels ("iv", "A-3") in the page box, status bar and thumbnails; typing a label jumps to it
- [x] Night mode keeps photos and logos in their real colours (full-page scans still darken)
- [x] "Open with" / second launch opens the file as a tab in the running window

Feedback on 0.2.0 (fixed in 0.2.1):
- [x] Regex search no longer bounces the view: search starts at your page, and the view jumps once, to the first
      match after you, only after every page in between has been searched. Searches stop at 20,000 matches
      (a pattern like "." would match every character). The regex button's tooltip has examples
- [x] Touchpad pinch-zoom works: Tauri had switched WebView2 pinch off, so pinches never reached the page. Pinch is
      back on at the WebView2 level; the page follows the fingers 1:1; Ctrl+wheel/Ctrl+= never zoom the UI itself
- [x] Menu tabs: File (menu) · Home · View · Tools · Help. Save and Save as are always on the Home toolbar. Tools shows
      the planned tool set with their phase; Help has keyboard shortcuts (F1) and About
- [x] Two-page "fit" now fits the spread you're on (a few landscape pages no longer shrink every spread)

Deferred:
- [ ] Windows 11 Snap Layouts on the custom maximize button. The page area is a WebView2 window owned by
      another process, so Windows never asks our window about the button. Needs a small transparent window over
      the button; planned with Phase 8 polish. (Win+Z already opens Snap Layouts.)
- [ ] Faster JPEG decoding for heavy scans (first sharp page on the 914 MB scan: 0.31 s vs 0.3 s target).
      Needs libjpeg-turbo built into MuPDF (extra build tools: NASM, CMake).
- [ ] Drag-and-drop from Explorer: built, needs a manual check (can't be automated from here)

## Phase 2.5: Read and Book modes (reflowed text)
Four views: Single · Double (cover on its own optional) · Read · Book.
- [x] Engine: every page's text in reading order with styles, paragraphs split, tables found, images and drawings
      located (MuPDF structured text + table hunt). 668-page prospectus: 0.39 s on 8 threads. MuPDF's own column
      segmentation costs ~60 ms a page, so column order is done in the app instead (XY-cut on paragraph boxes)
- [x] Flow: running headers, footers and page numbers removed; headings from text size and weight; list numbers
      joined to their paragraphs; paragraphs broken by pages, columns or list layout rejoined; photos, tables and
      drawings kept as pictures cut from the page; scanned pages kept as whole-page pictures (until OCR)
- [x] Read mode (Ctrl+H): one scrolling column at a comfortable width, on a sheet of paper
- [x] Book mode (F5): full screen, two pages with no gap and a spine shadow, running heads and page numbers, 3D page
      turn (click, arrows/Space/Page keys, wheel, or drag a page edge), 60 fps; first spread in ~0.2 s, whole book
      paginated in the background; tables shrink into space at a page bottom only while readable
- [x] Text settings (Aa): size (also Ctrl+=/−, Ctrl+wheel, pinch), font (Sitka, Georgia, Segoe UI), line spacing,
      margins, justified/left, paper (auto, white, sepia, dark; tables invert on dark paper)
- [x] Click a picture to see it large; page box, thumbnails and outline jump within Read/Book; the place is kept
      when switching views
- [x] Fix layout (View tab, Book bar, palette): the reading layout drawn over the real pages as numbered boxes (blue
      text, orange pictures, grey left out). Columns for the whole document or one page (Auto/1/2/3); click a box
      to make it text / picture / left out; "Set order" (click boxes in reading order); draw a rectangle to make a
      region one picture, text, or left out; reset a page. Corrections are remembered per file and Read/Book
      rebuild in ~30 ms
- [x] Find (Ctrl+F) inside Read and Book modes, with match case / whole word / regex; Book mode turns to the match
- [x] Better joining: list items and headings the engine splits in two, paragraphs split with no gap between them
- [x] Page overlay layers in the viewer (used by Fix layout; forms and annotations build on them)
Later (after OCR, Phase 5):
- [ ] Read aloud on top of the same text (natural offline voices, start marker, skip headers/footers)

## Phase 3: Form filling (done)
- [x] Engine: fields listed per page (text, checkbox, radio, dropdown, list, signature), values set through MuPDF so
      the form's own scripts run: number/date formatting, validation (bad values are refused), calculated fields
- [x] Pages render without their fillable fields and Folio draws them as real inputs in place (like Chrome): instant
      typing, no flicker. Multi-line, password, comb (one character per box), editable dropdowns, required fields
- [x] Formatted display ("Rs 37.50") with the plain value while editing; Tab moves between fields
- [x] Undo/redo: one history for page edits and form edits (form steps live in MuPDF's journal; drawing pages and
      running format scripts are kept out of it so redo survives)
- [x] Save writes the answers into the PDF (incremental); unsaved dot and "save changes?" include form edits
- [x] Form bar: field count, highlight fields, clear form, save a flattened copy (answers become page content)
- [x] XFA (dynamic) forms are detected and explained instead of silently failing
- [x] Fix: files in folders with very long paths (over 260 characters) couldn't be opened or saved
## Phase 4: Annotations and signing (visual signatures, DSC token)
Annotations (done):
- [x] Comment tab: highlight / underline / strike-through on text (drag over it), area highlight (automatic on pages
      without text, i.e. scans), pen with smoothing, sticky notes, text boxes, rectangles, ellipses, lines, arrows,
      stamps (Approved, Draft, Confidential…), Fill & Sign marks (tick, cross, dot, today's date, your name)
- [x] Real PDF annotations made by MuPDF (they show in Chrome and Adobe); author = Windows user name, dated
- [x] Click to select: move, resize (boxes, text, stamps), recolour, line thickness, text size, a comment on any
      annotation, delete (Del). Every change is one undo step; saves into the PDF
- [x] Comments list in the side panel (all pages, without loading them): jump to and select
- [x] Colour and thickness remembered per kind of tool

Sign pane (done): everything about signing lives in one pane on the right instead of dialogs. The Sign tab opens it
(its "Sign panel" button shows or hides it; Tools › Sign too). Sections: My signatures (thumbnails, click to place, New
opens the maker right there, delete on hover), Fill & Sign marks, Digital signature (certificates and options, Sign),
Signatures in this document, Save locked copy. A signed PDF only shows the bar at the top; its Details button opens the
pane at that document's signatures.

Visual signatures (done):
- [x] Sign tab: Signature and Initials dropdowns with your saved ones (thumbnails), Fill & Sign marks (text, tick,
      cross, dot, date, name), Save locked copy, Certificate (DSC, next)
- [x] Make one by drawing (mouse, pen pressure or finger; smoothed, ink thins with speed), typing (handwriting fonts
      that come with Windows: Segoe Script, Ink Free, Lucida Handwriting, Gabriola), or from a photo of your
      signature on paper (paper removed with Otsu's threshold and soft edges, specks dropped, ink recoloured or kept;
      a slider for shaded paper vs faint strokes; choose, paste or drop the picture)
- [x] Ink colours: black, blue, dark blue
- [x] Saved in the app's data folder, encrypted for your Windows account (DPAPI): unreadable on another account or PC
- [x] Placing: a faint copy follows the pointer at its real size; click to place. It's a PDF stamp with a transparent
      picture (shows in Chrome and Adobe); move and resize (keeps its shape); one undo step
- [x] Save locked copy ("… (locked).pdf"): signatures, comments and form answers become part of the page
- [x] Fixes: new pictures (signatures, image stamps) are compressed when saving (a locked copy went from 360 KB to
      43 KB); night mode darkens transparent pictures with the text instead of keeping them as photos

DSC digital signatures (built; needs a check with a real token):
- [x] Certificates from the Windows store (your token's driver puts them there): only ones that can sign; ones apps make
      for themselves (self-signed, or not for documents) hidden behind "show other certificates"; token or PC shown;
      expired ones listed but not usable; Refresh after plugging in a token
- [x] Signing goes through Windows' own crypto, so the token asks for its PIN in its own window (Folio never sees it).
      CMS detached signature, SHA-256 (SHA-1 only if an old token can't do SHA-256), full certificate chain,
      signing time and signing-certificate-v2 attributes; reason, location and signer name recorded in the signature
- [x] Sign with a certificate (Sign pane › Digital signature, or click a form's empty signature field): reason (common ones or
      your own), location, the box shows your name or your saved handwritten signature, date; draw the box on the page
      or sign invisibly; save as a new file ("… (signed).pdf") or into this file; optionally lock the form
- [x] Saved incrementally: a second signer keeps the first signature valid (tested); unsaved changes are saved first
- [x] Checking: every signature on opening: content unchanged (Windows verifies the CMS against the signed bytes),
      signer, issuer, trusted by Windows (at signing time if the certificate has expired since), changes saved after
      signing. Bar at the top (valid / can't confirm signer / invalid) and the details in the Sign pane
- [x] Open signed version: the document exactly as a signature signed it, in a new tab
- [x] Tested with a throwaway certificate held in memory (engine test and in the app): sign, second signer, comment
      after signing, a changed byte (caught), open signed version. Developers can offer that certificate in the app by
      starting it with FOLIO_TEST_CERT=1; normal starts never show it
- [ ] Check with your DSC token: sign, PIN in the token's window, open the result in Adobe Reader
- [ ] Later: trusted timestamp (needs internet), revocation check (needs internet), .pfx certificate files

Also from testing:
- [x] Left/Right arrows go to the next/previous page in Single and Double views (a spread in Double) and a screenful
      in Read mode, whatever the zoom
- [x] The + next to the tabs opens a Home tab (recent files, open) instead of the Open dialog; Ctrl+T too. A file
      opened from it takes its place
## 0.4.0: layout and memory
- [x] Page box, zoom and the view modes (Single / Double / Read / Book) moved to the middle of the status bar;
      the zoom menu opens upwards. Find stays at the right end of the toolbar
- [x] Narrow windows (side by side with another app): toolbar buttons that don't fit go behind a » button, whose
      panel opens under it, right-aligned
- [x] Memory: a closed document's rendered pages are dropped at once (they stayed in the cache, ~100 MB);
      pages scrolled away free their canvases at once instead of at the next garbage collection; while
      minimized or with no document open, WebView2 is asked to give memory back. Text PDF + 914 MB scan,
      then closing both: stayed at ~690 MB before, now back to ~250 MB within 40 s (212 MB at start)

## Phase 5: OCR (C++ Tesseract module)
## Phase 6: Tools suite
## Phase 7: Conversions
- [ ] Office quality test first: convert a mixed set of real Office files with MuPDF and with Microsoft Office,
      then compare visually (page similarity score), by text (missing or reordered words) and by layout
      (page count, table breaks, images). Results go into a side-by-side report and stay as a regression test.
- [ ] Open .docx / .xlsx / .pptx directly via MuPDF's built-in Office support
- [ ] Office conversion engines, tiered: MuPDF built-in (always) → LibreOffice (installed or our pack) →
      Microsoft Office when present
- [ ] Engine checker: detects Microsoft Office (registry, version), an installed LibreOffice, and our pack
- [ ] Settings → Office conversion: shows detected engines, preference order (Auto or a fixed pick),
      "Download LibreOffice (~300 MB)". Each conversion dialog can override the engine for that file.
- [ ] LibreOffice pack: official TDF installer, verified against its published checksum, unpacked into
      Folio's own folder (no system install, no admin rights)

## Phase 8: Packaging, updates and signing
- [ ] GitHub repository (the user has an account) hosting source, releases and update files
- [ ] "Check for updates" button that checks the repo's releases, plus optional automatic checks
      (Tauri updater: signed updates, verified before installing)
- [ ] Signed add-on list (voices, OCR languages, LibreOffice pack) hosted with releases, so add-ons can update
      without a full app update
- [ ] Dependency update routine: bump MuPDF / Tesseract / Rust and JS packages on a schedule (immediately for
      security fixes), each bump gated by the test suite (rendering, speed, memory, Office quality)
- [ ] Installer code signing
