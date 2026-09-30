# Paginaut roadmap

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
- [x] Pages render without their fillable fields and Paginaut draws them as real inputs in place (like Chrome): instant
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
- [x] Signing goes through Windows' own crypto, so the token asks for its PIN in its own window (Paginaut never sees it).
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
      starting it with PAGINAUT_TEST_CERT=1; normal starts never show it
- [ ] Check with your DSC token: sign, PIN in the token's window, open the result in Adobe Reader
- [ ] Later: trusted timestamp (needs internet), revocation check (needs internet), .pfx certificate files

Also from testing:
- [x] Left/Right arrows go to the next/previous page in Single and Double views (a spread in Double) and a screenful
      in Read mode, whatever the zoom
- [x] The + next to the tabs opens a Home tab (recent files, open) instead of the Open dialog; Ctrl+T too. A file
      opened from it takes its place
## 0.4.0: new name, layout and memory
- [x] Renamed from Folio to Paginaut ("Folio" is taken by several PDF apps, including a Windows one on the Microsoft
      Store). New identifier `app.paginaut.reader`, `paginaut.exe`, `paginaut://` protocol, PAGINAUT_* developer
      variables. The first start copies saved signatures and settings (recent files, reading positions, layout
      fixes) over from Folio; an old Folio install is left alone until it's uninstalled
- [x] Page box, zoom and the view modes (Single / Double / Read / Book) moved to the middle of the status bar;
      the zoom menu opens upwards. Find stays at the right end of the toolbar
- [x] Narrow windows (side by side with another app): toolbar buttons that don't fit go behind a » button, whose
      panel opens under it, right-aligned
- [x] Memory: a closed document's rendered pages are dropped at once (they stayed in the cache, ~100 MB);
      pages scrolled away free their canvases at once instead of at the next garbage collection; while
      minimized or with no document open, WebView2 is asked to give memory back. Text PDF + 914 MB scan,
      then closing both: stayed at ~690 MB before, now back to ~250 MB within 40 s (212 MB at start)

## Phase 5 (0.5.0): OCR (Tesseract)
Text recognition with the Tesseract 5 built into our MuPDF libraries (no separate module), through a small bridge of
our own (src-tauri/native/ocr.cpp): MuPDF's own OCR path reloads the models for every page and allows one recognition
at a time in the whole process.
- [x] Pages recognised in parallel: one Tesseract per worker, half the workers (at most 8). 300 dpi grey render;
      ~1.4 s a page on one core, 24 pages in 4.5 s on 8 (synthetic 300 dpi scan, mean confidence 95). Peak ~50 MB
      per recognition thread, all given back when the job ends
- [x] Only what isn't text yet is read: page text (visible or invisible, e.g. an earlier OCR layer) is left out of
      the render, so mixed pages and scans that already have text never get it twice; pages with nothing left skip
      Tesseract. Words it's unsure of (confidence < 25) or that are only punctuation are dropped
- [x] The text goes into the PDF as an invisible layer (like scanners and Adobe): an invisible "GlyphLessFont",
      each word stretched to its box on the line's baseline (skew followed), right-to-left words placed from the right.
      Marked streams (/PaginautOCR) so "Remove recognized text" can take them out again. One undo step; Ctrl+S keeps
      it (incremental: +83 KB for 12 pages)
- [x] Search, selection, copy and Read/Book mode work on it right away: while pages have unsaved text, background
      jobs (search, reflow) read the document in memory instead of private copies of the file. Read mode re-reads
      after recognising, undo and redo; OCR text isn't mistaken for monospace, and its size is the line's
- [x] Written in reading order: lines Tesseract read across a gutter (an article line joined to the sidebar beside
      it) are split, and the pieces ordered by XY-cut (columns only where both sides are at least a fifth of the width,
      so full-width text, line numbers and table rows keep their order). MuPDF groups paragraphs in that order, so
      columns stay apart in Read mode
- [x] Offer bar when text is part of the picture (12 pages sampled: under 30 characters of text, and over 0.3% of the
      page drawn once text is left out): scans, and also web pages printed to PDF or documents whose text became
      outlines. Scans that already have a text layer (e.g. from the scanner) aren't offered. Recognize text; ×
      stops offering it for that file. Tools › OCR and the command palette: Recognize text…,
      Remove recognized text
- [x] Recognize text dialog: all pages / this page / a range ("1-5, 8"), languages (chips; each extra one is
      slower), accuracy Standard (fast models) or Best (tessdata_best, downloaded once). Remembers the last choice.
      A warning on digitally signed documents. Pictures (PNG, JPEG, TIFF…) are saved as a PDF first
- [x] Progress in the bar with Stop (pages already read keep their text); saving while it runs offers
      "Stop and save"; closing the tab stops it
- [x] English, Hindi and Marathi (fast models, 7.3 MB) built into the installer (native/get-tessdata.ps1 fetches
      them, pinned by SHA-256). 123 more languages, and best models for all, download from Tesseract's own
      repositories at tag 4.1.0 through WinHTTP (system proxy and certificates), checked against the size and git
      hash GitHub lists (tools/make-ocr-langs.mjs); stored under %LOCALAPPDATA%\app.paginaut.reader\tessdata, and can
      be removed from the dialog
- [x] Models are read through our own wide-character file reader, so Windows user names with non-English letters work
Later:
- [ ] Sideways pages: our Tesseract build has no orientation detection (it needs the legacy engine); rotate such pages
      first. A try-each-rotation fallback on low confidence could come later
- [x] Marathi check: Maharashtra Times article printed to PDF (8 pages, text as outlines): Marathi + English, 1,789
      words in 1.9 s, 91% mean confidence on page 2; Marathi search finds the words. Few misreads (a large bold
      heading dropped words); Best models may help
- [ ] Recognise again with other settings (today: Remove recognized text, then Recognize text)
## Phase 6: Tools suite, in three parts, then Redact
Installer versions follow the phases: part 6.1 = 0.6.10, 6.2 = 0.6.20, 6.3 = 0.6.30, 6.5 = 0.6.50; fixes in between
count up from there (0.6.11, 0.6.12…).

### Phase 6.1 (0.6.10): Pages (done)
Foundation:
- [x] Pages from other documents: incoming pages are copied into the open document as one MuPDF journal step (like OCR
      and forms), so the page order can point at them, undo/redo come for free and saving drops what isn't used (a
      full rewrite when pages were added and then removed again). PDF pages keep their comments and form fields
      (fields whose names are taken get "_2"); links to pages that come along (a table of contents) point at their new
      place, links to pages left behind are dropped, web links kept. Other
      formats (pictures, EPUB, XPS…) are drawn into new PDF pages, JPEGs kept as they are. Search and Read mode see
      the new pages at once. 300 pages of a 241 MB scan in 0.3 s (src-tauri/examples/insert_test.rs)
- [x] Fix: saving after deleting or moving pages lost the form (fields became dead), page labels, attachments,
      document scripts and viewer settings: MuPDF's page rearranging rebuilds the document catalog with only a few
      entries. They're put back now; form fields are trimmed to the kept pages and page labels renumbered (deleting
      page 5 of 1…10 gives 1…9, front matter in i, ii, iii stays apart). Saving rotations failed once undo was on

Build documents from pages:
- [x] Insert pages (Tools › Insert, command palette): from other files (several at once, a page range for one,
      password-protected ones unlocked in the dialog), after/before the current page, at the start or end; pictures
      fitted to this document's page size, A4, Letter or their own size. Blank page: same size as the current page,
      straight after it. One undo step each
- [x] Merge (Tools › Organize, command palette, Home page): files into one new PDF, starting with the open document;
      reorder by dragging or arrows, a page range per file, password-protected files unlocked in the dialog; a bookmark
      per file with that file's own bookmarks inside (only those whose pages were taken); clashing form field names
      renamed; pictures fitted to A4, Letter or their own size. Saved beside the target first, then opened in a tab
      (4 files, 20 pages: 23 ms)
- [x] Drag thumbnails from one open document into another: holding over the other document's tab opens it, then drop
      among its thumbnails (or on the tab itself: at the end). The pages come as they're shown there (rotations,
      unsaved form answers and comments included); one undo step
- [x] Right-click on thumbnails: rotate, delete, save as a new PDF, insert pages from a file, insert a blank page
- [x] Extract (Tools › Organize, right-click, command palette): the selected pages as a new PDF, opened in a tab.
      Unsaved changes and rotations are included; the document itself stays as it is
- [x] Split into files: every N pages, by page ranges ("1-3, 4-10, 11-", one file each), by top-level bookmarks (files
      named after them), by file size (pages grouped counting shared fonts and pictures once per file: a 241 MB scan
      into 37 MB files, grouped in 3 ms, written in 0.9 s). Preview of the files, folder choice, asks before
      replacing files
- [x] Page grid (View tab, Tools › Organize, Ctrl+Shift+G): all pages as big thumbnails in place of the pages. Size by
      slider or Ctrl+wheel (remembered); select (click, Ctrl, Shift, Ctrl+A), drag to reorder or onto another
      document's tab, rotate, delete, save as a new PDF, insert, right-click menu; double-click opens a page, Esc or
      Done goes back

Page size (crop and resize in one tool; Tools › Organize, right-click on pages, command palette):
- [x] Crop box on the page (whole page in view): drag edges, corners or the box; size shown on it; shape lock: any,
      the page's own, paper (A3, A4, A5, A6, B5, Letter, Legal, Tabloid), photo (passport 35×45 mm, 4×6 in, 5×7 in),
      screen (1:1, 4:5, 2:3, 3:4, 9:16) with portrait/landscape; width × height typed in mm / in / pt; Whole page;
      Fit to content (trims empty margins). The lock follows the page as shown, even on turned pages
- [x] Crop mode: "Hide edges" (the page's crop box; can be shown again) or "Remove outside" (text, pictures and
      drawings outside deleted through MuPDF's redaction, comments outside dropped)
- [x] ☐ Change the page size: paper and photo sizes or custom; content Fit (all of it shows), Fill (covers the page,
      overflow cut off) or Actual size, placed with a 3×3 anchor. The page's content becomes one form object drawn
      with a matrix (stays vector and searchable); its own rotation is made part of it; comments, links and form
      fields move and scale with it (tested: a form turned landscape still fills in)
- [x] Fill for the new space: nothing (see-through), a colour (with a picker from the screen), or the edges stretched
      out (the outermost pixel rows and columns of the content, for scans). Only the new margins are filled
- [x] Apply to: this page, selected, all, odd, even, a range (same margins on each). One undo step; page sizes come
      back on undo/redo (src-tauri/examples/pagesize_test.rs)
- [ ] Optional, later: content-aware fill of the new margins (inpainting from the page's own picture)

Strike-through in both places (highlight stays in the Comment tab only):
- [x] Fill & Sign strike line (Sign tab, Sign pane, Stamp menu): drag a black line across anything, scans too ("strike
      out whichever is not applicable"); it snaps level or upright when the drag nearly is; stays on for more lines
- [x] Comment tab Strike and Underline on scans: where there's no text, a line along the drag

- [x] Placeholder: Tools › Organize › "Edit PDF" shown as coming in 6.3

### Phase 6.2 (0.6.20): Stamp, protect, extract (done)
- [x] Night mode on scans and pages that are one big picture (screenshots, web pages printed as images): the photos in
      them are found in the rendered page (cells of mostly mid-tones or colour, joined into regions, paper edges
      trimmed) and keep their real colours, while text stays dark. Text-only scans get none
Stamped on pages (Tools › Organize, command palette). Each is one piece of content drawn from a form object,
marked as a pagination artifact (header/footer or watermark) in a flagged content stream, so Remove takes it off
again and adding again replaces it. Text is laid out by MuPDF's HTML engine with its built-in fonts, so Marathi and
Hindi are shaped properly. Placed on the page as shown (turned pages get them at the top as you see it):
- [x] Page numbers, headers and footers: six boxes (header/footer left, centre, right) with fill-ins {page} {pages}
      {label} {date} {file} {bates}; one-click styles (1, Page 1 of N, 1 / N, Bates); font, size, colour, bold;
      margins; page range and starting number (a cover left out with "2-"). 1000 pages in about 0.6 s
- [x] Bates numbering: prefix, digits, first number, suffix
- [x] Watermark: text (font, size, colour, bold) or a picture (width as % of the page), angle 0/45/-45/90, top, middle
      or bottom, in front or behind, opacity, which pages; in its own "Watermark" layer (optional content), marked
      as a watermark for Acrobat (src-tauri/examples/stamp_test.rs)
- [ ] Copy and search of stamped Devanagari text: conjunct letters come out as unknown characters (the shaped
      glyphs have no text mapping); they look right

Document details:
- [x] Bookmarks editor in the Outline panel: + Bookmark (for the page you're on; selected text becomes the title),
      rename (F2 or double-click), drag to reorder or nest, right-click to point at the current page, add inside,
      move in/out, delete. Each change is one undo step; pages show their labels
- [ ] Build bookmarks from headings (large or bold lines)
- [x] Document properties (Ctrl+D): title, author, subject, keywords; file, kind, size, maker, dates shown
- [x] Page labels dialog: ranges (from page, 1/i/I/a/A or none, prefix, start at) with a preview; remove all

Security:
- [x] Protect (Organize menu): password to open (AES-256) and/or a permissions password with what's allowed
      (printing, copying, comments and forms, changing pages, other changes); shows the current protection
- [x] Unlock / change: needs the permissions password (the open password alone can't lift restrictions); remove
      protection. A wrong permissions password no longer leaves the file unreadable (MuPDF drops its keys on a
      failed try; they're restored)
- [x] Warning on digitally signed documents (their signatures stop being valid after a rewrite)

Size and repair:
- [x] Reduce file size: Tidy up (lossless: de-duplicate, object streams, maximum compression), Smaller (pictures
      at 150 dpi, photos JPEG) and Smallest (100 dpi, all JPEG); black-and-white scans stay sharp. Save a copy or
      replace; size before and after. A 242 MB 300-page scan: 86 MB / 38 MB; a colour newspaper 5.4 MB -> 1.9 MB
- [x] Fix (MuPDF): gray and RGB pictures with an ICC profile came out garbled when recompressed as JPEG
      (patched in native/build-mupdf.ps1)
- [x] Repair: rewrites a damaged file and says what was lost
- [x] Fix: saving a file MuPDF repaired on opening wrote a page tree it couldn't open again ("cycle in page
      tree"); the tree is now rebuilt from the pages that can be read, and the save says how many were lost

Extract content (right-click the item on the page; Tools › Organize; command palette):
- [x] Pictures (right-click one on the page): copy, or save as stored (JPEG and JPEG 2000 byte for byte, full
      resolution), as PNG (with transparency) or as a one-page PDF (picture stored as is, at its size on the page).
      "Save all pictures" for all, selected or listed pages: repeats saved once, small ones (icons, specks) skipped
- [x] Tables (right-click one on the page, "Table on this page", or "Read as a table" on a drawn area): found by
      MuPDF's table hunt (57 tables on 60 pages of a prospectus in 0.3 s); copy as a real table (cells in Excel
      and Word), save as Excel (.xlsx: figures as numbers, "1,23,456.50" and "(1,234)" too; spanned cells merged;
      column widths as on the page) or CSV (UTF-8, Marathi safe). The table dialog shows it over its picture: drag
      the column lines, click to add one, double-click to remove; the columns are then exactly those. Heading lines
      are joined into one row (years stay headings)
- [ ] Tables, later: rows as adjustable as columns; group headings spread over the columns they span; percentages
      as numbers; the same menu in Read/Book mode
- [x] Any area, charts and drawings: "Select an area" (right-click menu, Organize), drag a box: copy its text,
      copy as picture, save as PDF (vector; the rest of the page is hidden, not removed), SVG (vector, text as
      outlines) or PNG at 150/300/600 dpi; turned as the page is shown
- [x] Attachments: files embedded in the PDF (on the document or pinned to a page) in a paperclip side panel:
      open (double-click; programs and scripts are refused, only saved), save, save all
- [x] Fix: copying a picture made WebView2 ask for clipboard permission (the write now starts inside the click)

### Phase 6.3 (0.6.30): Edit PDF (done)
- [x] Edit text (Tools › Edit, Organize, command palette): paragraphs outlined; click one and change its words in
      place. The old text is removed through MuPDF's redaction (text only; pictures and drawings under it stay) and
      the new text is laid out by MuPDF's HTML engine in the same box, size, line spacing, colour and alignment
      (left, right, centre, justified), in the paragraph's own font where it has the letters and a close font
      letter by letter where it doesn't (Marathi in a Times paragraph works). First baseline lands exactly where
      it was. One undo step, 5 ms (src-tauri/examples/edit_test.rs)
- [x] Pictures on the page (the same Edit mode; Tools › Insert › Picture): click one to move it, resize it
      (corners keep its shape, Shift frees them), replace it (fitted into its box) or delete it (Delete key).
      Moved pictures keep their turn and flip; pictures and paragraphs stack as the page draws them, so the one
      on top is the one clicked. Each change one undo step
- [x] Attachments: attach files (Organize, the paperclip panel's +) and remove them (a file pinned to a page loses
      its pin); undo and redo refresh the list
- [x] Fix: page rewrites (text edits, pictures, crop with Remove outside) dropped the form objects page numbers
      and watermarks draw with, so they vanished or couldn't be removed; they're set aside and put back now
- [x] Fix: stamped text sat 12 pt inside its box (MuPDF's style sheet gives the body a 1em margin)
- [x] Fix (MuPDF): attaching a file left a journal operation open, so undo stopped working (patched in
      native/build-mupdf.ps1)
- [ ] Later: edit a single line inside a table cell; bold/italic changes within a paragraph; keep a moved
      picture's place in the drawing order (it goes on top now)

### Phase 6.5 (0.6.50): Redact (done)
- [x] Redact panel (Tools › Redact, Organize, command palette). Marks are standard Redact annotations (red
      outlines, saved with the file until applied): drag over text to mark it, drag a box on scans (or anywhere with
      "Mark boxes even over text"), or find and mark every match of a word, phrase or pattern. Clear marks; each
      step one undo step
- [x] Remove (default): text, picture pixels and drawings under the marks deleted through MuPDF's redaction, then
      checked (letters still readable under the boxes are counted and reported; 0 in the tests). Cover only: boxes
      drawn over, the text stays, and the panel and message say so plainly (letters under them counted)
- [x] Box colour and optional text on the boxes ("REDACTED", white on dark boxes, dark on light ones); page numbers
      and watermarks survive
- [x] Patterns: PAN (holder letter checked), Aadhaar (Verhoeff check digit, so "2026 2025 2024" in a table isn't
      one), phone, email, IFSC, GSTIN, plus any word or regular expression. On a prospectus: 151 emails, 9 phone
      numbers, the company's 2 PANs, 1 IFSC; no false Aadhaar numbers
- [x] Sanitize the whole document: metadata (info, XMP, editing history), scripts (document, page, link and field
      JavaScript), attached files, comments, hidden text (off by default: it also takes out recognized text)
- [x] Warning on digitally signed documents
- [x] Fix: MuPDF's pattern engine misreads a "-" at the end of a character class, and gives up on long pages with
      patterns like [..]+@; the email pattern avoids both, and a pattern that gives up on some pages keeps what it
      found elsewhere (the panel says so) (src-tauri/examples/redact_test.rs)

## Phase 7 (0.7.0): Office files (done)
- [x] Office quality test first (tools/office): make-samples.py makes a mixed set with Microsoft Office itself (a report
      with headings, lists, a table crossing pages, a picture, footnote, header/footer, two columns, a text box,
      Hindi/Marathi, a landscape section; a letter; a budget with formats, formulas, a chart and a too-wide sheet; an
      invoice fitted to a page; a 7-slide deck with a picture, table, shapes, chart and Devanagari; plus
      password-protected ones) and Office's own PDFs as the reference. compare.py scores each converter: ink in the same
      places per page (plain SSIM rated blank-ish pages highly whatever was on them), words missing / out of order /
      added (Indic text by the picture only: Word's PDFs give it back garbled), page count and size; writes a side-by-side
      report.html and checks against baseline.json (the regression test)
- [x] Results: MuPDF's built-in Office support is a text extractor (0.00–0.24; no fonts, pictures, bullets, table grid,
      number formats or slides: a 7-slide deck becomes 2 pages of lines). LibreOffice 0.97–1.00 once it has the fonts
      (0.74 on the report before: Office's default font Aptos is a cloud font only Office sees); the budget scores 0.67
      because a too-wide sheet breaks into pages one column differently (its tables, formats and chart are right).
      Microsoft Office 1.00
- [x] Open .docx/.doc/.rtf/.odt, .xlsx/.xls/.ods, .pptx/.ppt/.odp (and templates, macro-enabled): converted to a PDF
      (cached, so reopening is instant), shown as that PDF under the Office file's name. Everything PDF works on it
      (mark up, sign, redact, edit); Save means Save as PDF (the cached PDF is never overwritten). Merge and Insert take
      Office files too. Bar over the document: which program made it, Save as PDF, Convert with… another one
- [x] Converters, tiered: Microsoft Office when installed (Word/Excel/PowerPoint each detected; driven through COM by
      name, as the typed interfaces are often broken by click-to-run installs; read-only, macros off, no prompts:
      password-protected files fail with a message instead of asking; 1–3 s a file) → LibreOffice (installed, found
      through its registry entry, or our pack; ~1.5 s a file, the first after setup ~10 s) → MuPDF's text preview
      (.docx/.xlsx/.pptx only, laid out on A4)
- [x] Office files dialog (File menu, command palette): programs found (Office version, LibreOffice version/size),
      preference (best here / Microsoft Office / LibreOffice / text preview), Download LibreOffice / Remove
- [x] LibreOffice pack: newest stable release from download.documentfoundation.org, SHA-256 checked against the one the
      Document Foundation publishes, code signature checked (The Document Foundation), unpacked with an administrative
      install (no system install, no admin rights), trimmed to what converting needs (1.6 GB → 633 MB: other languages'
      menus and dictionaries, help, icon themes left out; its own fonts and the C++ runtime moved where it looks for
      them), profile built. Office's cloud fonts (Aptos…) copied into its font folder. Conversions run one at a time in
      a job object (a stuck LibreOffice is stopped along with everything it started, after 3 minutes)
- [x] Downloads resume after a dropped connection, and move on from a slow mirror (the download site hands out a mirror
      per request; some send at 70 KB/s)
- [x] Convert to PDF for many files: each with its own program (Auto or a pick), PDFs next to the files or in a folder,
      never over an existing file ("report (2).pdf"), Open / Show in folder
- [x] Fixes found on the way: click-to-run Word can't see folders made in the local app data folder after it was set up
      ("The directory name isn't valid"), so it writes to the temp folder; Windows Installer won't open a package kept
      in the app's data folder (error 1619), so the download goes to the temp folder; LibreOffice needs absolute paths
      (a relative profile path stops it with a "bootstrap.ini is corrupt" message box)
- [x] Office file types aren't registered with the installer: Tauri's installer would make Paginaut their default
      program, taking .docx away from Word
- [x] Also: the Sign tab no longer opens the Sign pane by itself, and the pane starts closed
- Tests: src-tauri/examples/office_test.rs (both converters, protected files, opening through the engine, cache,
      Save as), office_convert.rs (MuPDF conversion + page dumps for compare.py), lo_setup.rs (pack from a downloaded
      installer)

## Phase 7.1 (0.7.10): Convert pane (done)
The Tools › Convert menu was a placeholder ("Phase 7"), and Phase 7 only covered Office files → PDF.
- [x] Convert pane on the right (in place of the Sign pane; opening one closes the other), opened and closed by
      Tools › Convert (0.7.11: straight away, no menu first) and from the command palette: "This document to" and "Files to PDF" in one place, Office suite choices at
      the bottom
- [x] This document to Word (.docx): Microsoft Word when installed (Word reads the PDF itself: paragraphs, tables and
      pictures editable, close to the layout; 1.5 s), else Paginaut's own writer (src-tauri/src/docx.rs): text as
      paragraphs with size, bold, italic, colour, alignment and font, tables as Word tables, pictures, in reading order,
      a page break per page, page numbers left out, symbol-font bullets as bullets. Tried and rejected: MuPDF's own
      .docx writer (files Word and LibreOffice refuse: line breaks inside text runs, "24.000000" sizes, pictures
      referenced but not packaged) and LibreOffice's PDF import (1,064 positioned text boxes; text on shaded table rows
      lost)
- [x] Excel (.xlsx): every table found on the pages, a sheet each ("Page 3", "Page 3 (2)"), numbers as numbers
- [x] PowerPoint (.pptx): LibreOffice when available (editable slides: text boxes, shapes, pictures), else Microsoft
      PowerPoint (a picture per slide, slide shaped like the pages); neither: offers LibreOffice
- [x] Images: PNG or JPEG (a file per page, numbered after the name chosen) or one multi-page TIFF (own writer,
      src-tauri/src/tiff.rs: baseline RGB, PackBits, resolution recorded), 72–600 dpi
- [x] Text (.txt): MuPDF's text writer, reading order
- [x] Pages: all, the selected ones, or a range; unsaved changes and rotations included. Open the result in its
      program or show it in its folder; Stop for long jobs
- [x] Files to PDF: Office files (Phase 7), web pages (HTML, laid out on A4), e-books, XPS, text; Pictures to one
      PDF: the Merge dialog with pictures picked
- [x] Office files dialog: "Office suite choices", each suite with Available / Not installed, LibreOffice's download
- Tests: src-tauri/examples/convert_test.rs (every format and program, pictures numbered, tables found, web page to
      A4, a PDF not converted again); unit tests for PackBits and font names

## Phase 7.5 (0.7.50): Compare
- [ ] Two PDFs side by side, scrolling together: word-by-word text diff with a list of changes, plus a picture overlay
      for changes in drawings and scans
- [ ] Word vs PDF: the .docx text read directly, compared with the PDF's text ("is this PDF the final version of this
      Word file?"). Text only, since no two programs draw a Word file quite the same way
- [ ] Excel vs PDF: cell by cell against the tables found in the PDF (Read-mode table detection); only as good as the
      printout (hidden columns, number formats and page breaks change what's in the PDF)

## Phase 7.6 (0.7.60): Easier to use
- [ ] Rework the menus, toolbar tabs and bars into fewer, clearer places: every command findable in one obvious spot,
      related tools together, rarely used ones out of the way, bars over the document that don't pile up
- [ ] Settings: one dialog (Help › Settings, and the command palette) with a tab per area, so every setting has one
      central place instead of being spread over menus and panels: appearance (theme, night mode), opening and saving
      (auto-save rotations…), Read and Book modes, comments and signing, text recognition (languages, downloads), Office
      files (suite choices, LibreOffice download), memory and performance

## Phase 8 (0.8.0): Packaging, updates and signing
- [ ] GitHub repository (the user has an account) hosting source, releases and update files
- [ ] "Check for updates" button that checks the repo's releases, plus optional automatic checks
      (Tauri updater: signed updates, verified before installing)
- [ ] Signed add-on list (voices, OCR languages, LibreOffice pack) hosted with releases, so add-ons can update
      without a full app update
- [ ] Dependency update routine: bump MuPDF / Tesseract / Rust and JS packages on a schedule (immediately for
      security fixes), each bump gated by the test suite (rendering, speed, memory, Office quality)
- [ ] Installer code signing
