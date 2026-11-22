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
- [x] Windows 11 Snap Layouts on our maximize button (0.8.5, src-tauri/src/snap.rs): an invisible layered child
      window sits exactly over the page's maximize button (the page reports where it is, and hides it with the
      title bar in Book mode and full screen), above the WebView; it answers HTMAXBUTTON, maximizes or restores
      on a click, and tells the page when the mouse is over it so the button lights up. One pixel of non-client
      area is kept at the top while the window isn't maximized (Windows wants it before offering the layouts from
      a child window). Layered child windows need the app to declare Windows 10/11 support: Paginaut now has its
      own manifest (src-tauri/windows/app.manifest). Checked with hit tests and window messages; the flyout
      itself needs a real mouse over the button
- [ ] Faster JPEG decoding for heavy scans (first sharp page on the 914 MB scan: 0.31 s vs 0.3 s target).
      Needs libjpeg-turbo built into MuPDF (extra build tools: NASM, CMake). Left as is by choice (2026-10-01):
      the gain is small (0.31 s against 0.3 s)
- [x] Drag-and-drop from Explorer (checked by hand in 0.8.1)

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
- [x] Read aloud (0.8.2) on the same text, so running headers, footers and page numbers are skipped; see Phase 8
- [x] Natural offline voices (Piper), downloaded on request in Settings › Read aloud: 130 voices in 50 languages (Hindi, Marathi, Malayalam, Telugu, Nepali among them) pinned by hash to one commit of rhasspy/piper-voices; Piper 2023.11.14-2 runs as its own program; a sample plays before downloading; sentences are spoken one by one and words timed by length inside each; a downloaded voice is used first for its script

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
- [x] 0.8.2: certificate files (.pfx/.p12, password asked, key kept in memory only), trusted timestamps (RFC 3161,
      on by default, DigiCert's service unless changed; a failed timestamp offers signing without one), and "Check
      whether it was revoked" on a signature (asks the certificate's authority online). Signing in place goes
      through a copy, so a cancelled PIN or an unreachable timestamp service never leaves an empty signature in
      the file

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
- [x] Sideways and upside-down pages (0.8.2): a page that reads poorly is tried at each quarter turn (its middle
      only); a clear winner is read in full and its text turned back onto the page, so search and selection sit on
      the words. Lines Tesseract reads vertically by itself don't count (they'd misplace the text). The bar says
      how many were turned and offers "Turn upright". No extra model; upright pages cost nothing more (4 turned
      test pages in 7 s)
- [x] Marathi check: Maharashtra Times article printed to PDF (8 pages, text as outlines): Marathi + English, 1,789
      words in 1.9 s, 91% mean confidence on page 2; Marathi search finds the words. Few misreads (a large bold
      heading dropped words); Best models may help
- [x] Recognize again (0.8.2): the dialog offers to read pages recognized before again, with other languages or
      Best accuracy, replacing their text in the same undo step; "Recognize again…" on the finished bar
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
- [x] Content-aware fill of the new margins (0.8.6): "Paper" fills the new space with the page's own paper: blank
      patches (no ink, close to the paper's colour) from near the content's edges, laid at random and blended four
      at a time, fading into the softened edge of the content so there's no seam; saved as one JPEG behind the page
      with the content's place cut out. A page with text up to its edges (no blank paper) gets its edges stretched
      instead (pagesize_test: a scan on a bigger page)

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
- [x] Copy and search of stamped Devanagari text (0.8.3): joined letters (क्र, ष्ट्र, ज्ञ), vowel signs drawn
      before their letter (कि) and Arabic joining forms are drawn inside ActualText holding their letters (a
      pass-through device around the HTML engine's text, for stamps, edited text and redaction labels); plain
      letters and spaces stay as they are, so words stay searchable one by one. Two MuPDF patches in
      native/build-mupdf.ps1: its PDF writer device now writes ActualText (it dropped it), and its text
      extraction counted what's left of an ActualText in bytes, repeating the end of non-English replacements
      (which also affected other people's PDFs). Check: examples/shaped_dump.rs, stamp_test's exact Marathi line

Document details:
- [x] Bookmarks editor in the Outline panel: + Bookmark (for the page you're on; selected text becomes the title),
      rename (F2 or double-click), drag to reorder or nest, right-click to point at the current page, add inside,
      move in/out, delete. Each change is one undo step; pages show their labels
- [x] Bookmarks from headings (0.8.2; Outline › From headings, command palette): Read mode's headings nested by
      size, then Section/Part/Chapter, ALL CAPS, the rest; contents-page lines, misread text and bold pieces of
      sentences left out; replaces existing bookmarks after asking, one undo step (a 550-page prospectus: 1,208)
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
- [x] Tables to Excel: percentages as numbers (0.8.2: "12.5%" is 0.125 shown as 12.50%; "(3%)" is -3%)
- [x] Rows as adjustable as columns (0.8.4): orange row lines in the table dialog to drag, Shift+click to add,
      double-click to remove (joins the rows); once set, the table is read with exactly those rows
- [x] Group headings over their columns (0.8.4): a phrase that crosses column lines with only word spacing
      ("Three months period ended June 30,") is one cell spanning them; the table finder's own bordered spans are
      kept, and overlapping ones are joined (before, words of a heading were scattered over the columns, or lost
      when two of the finder's cells overlapped). The finder's tables now place letters like tables you draw
      lines for. "Join heading lines" keeps such rows above the joined column headings; the preview and Copy
      show them spanning (colspan), Excel merges them
- [x] The same menu in Read and Book modes (0.8.4): right-click a table there (tables show as pictures of the page)
      for Copy table, Save as Excel/CSV, Check and adjust columns; "Read as a table…" on other pictures; "Read aloud
      from here" on a paragraph; Copy for selected text
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
- [x] A moved or replaced picture keeps its place in the drawing order (0.8.4): the content filter leaves a
      one-off marker picture where it was drawn, whose resource then becomes a small form drawing the picture at
      its new place, so what was drawn over it still is. Paginaut's own forms are marked (whole page, or unbounded)
      so later moves aren't cut off; a picture inside someone else's form, or clipped where its new place would
      be cut off, still goes on top, seen whole (edit_test: under red stamped text after a move and a replace)
- [x] Bold and italic within a paragraph (0.8.5): the editor is rich text now, showing the paragraph's bold and
      italic words as found (paragraphs report their runs of one style); Ctrl+B and Ctrl+I change the selection,
      Enter makes a line break, paste comes in plain. The new text is laid out in the paragraph's own regular, bold,
      italic and bold-italic faces where the page has them, and the matching built-in face where not (with one
      face only, the HTML engine set bold words regular). A plain edit sends plain text as before
- [x] Table cells edit one by one (0.8.5): a block whose lines sit side by side on a baseline (a table row the
      text finder took for one block) is split into cells, lines stacked over each other staying together; a lone
      figure is set right, as its column is. Before, editing one figure re-flowed the whole row as a paragraph
      (edit_test: a prospectus row, one figure changed, the others kept)

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
- [x] This document to PDF/A-2b or -3b (0.9.9): see the PDF/A section
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

## Phase 7.2 (0.7.20): Markdown (done)
- [x] Open .md / .markdown files (MuPDF's own Markdown reader: GitHub tables, code, quotes, lists; laid out on A4),
      and make PDFs of them (Save as PDF, or Convert › Files to PDF)
- [x] Edit a Markdown file: Tools › Edit (for a .md tab) opens the Markdown pane on the right with the source; the pages
      follow as you type (made again from the text after a short pause, the page you're on kept); Save / Ctrl+S writes
      the .md file (Save as still makes a PDF); closing asks about unsaved changes. One pane on the right at a time
- [x] PDF → Markdown (Convert pane): headings from text clearly larger than the body, bold/italic paragraphs, bullet
      and numbered lists, tables as pipe tables, pictures saved in "<name>_files" next to it and linked; formatting
      characters escaped. Running headers and footers (the same text near the top or bottom of most pages) and page
      numbers are left out, in the Word export too
- [x] Fixes: MuPDF's Markdown reader didn't ask for the file's folder, so linked pictures showed as "[image]" (fourth
      MuPDF patch in native/build-mupdf.ps1); paths reach MuPDF with forward slashes, since it finds a file's folder by
      its last "/" (pictures of Markdown files and web pages opened by full path weren't found)
- Tests: convert_test.rs (PDF → Markdown, Markdown → PDF, Markdown shown from edited text); unit tests for escaping

## Phase 7.5 (0.7.50): Compare (done)
- [x] Compare tab (Tools › Compare, File › Compare files…, the command palette): pick the original and the changed file
      (or an open one); the two side by side, scrolling together (lined up by matching words, page by page past pages
      without any), what differs marked on both (red on the left, green on the right, purple for moved, dashed blue
      for drawings), and the list of differences on the right: changed / added / removed / moved, where (page, slide,
      sheet and cell), filters, F7 / Shift+F7 or Alt+↓/↑ to step through; click a mark on a page to find it in the list
- [x] Any two files it opens: PDF with PDF, and a PDF with the Word, PowerPoint or Excel file it was made from (their
      words read straight from the file: .docx/.pptx/.xlsx and OpenDocument; older .doc/.ppt/.xls through the PDF
      Office or LibreOffice makes of them), Markdown with its PDF, Word with Word…
- [x] Words lined up with patience diff (Myers for spreadsheets), stretches one word apart joined into one change,
      the same words taken out in one place and put back in another shown as moved. Typography doesn't count (curly
      quotes, dashes, ligatures, words hyphenated over two lines); running headers, footers and page numbers left out
      (option); capitals count (option). Against Word files: list numbers and footnote numbers (not in Word's text)
      left out. Letters MuPDF's own PDFs lose in Indian scripts (U+FFFD) match anything
- [x] Excel: cells shown as Excel shows them (number formats, dates with Windows' short date, percentages, General's
      10 digits) and matched cell by cell wherever the printout put them (wide sheets print column by column);
      numbers that round to each other match; currency signs and hidden rows/columns left out. Shown as a sheet
      with its column letters and row numbers
- [x] Drawings and scans (two PDFs, pages of the same size): the pages drawn at 50 dpi, text left out (compared as
      words), pixels differing where the other page has no similar shade nearby (a line a pixel off isn't a change),
      grouped into boxes; slivers and specks dropped, a page mostly different shown as one box
- [x] Engine: words with their boxes from MuPDF's text (a change of size inside a word splits it; footnote numbers
      flagged); zip parts read through MuPDF; a small XML reader. Fix: marking redactions on a page not yet
      measured deadlocked (the page's size was read under the document's own lock)
- Speed: 668-page prospectus against itself (425,000 words a side, 668 page pairs of drawings) in 2.1 s; 300 scanned
  pages in 3.4 s; Word/Excel/PowerPoint samples against their PDFs in 3–45 ms. Drawings are drawn by threads with
  their own copies of the documents (through the shared ones they waited on each other's locks: 9.4 s)
- Tests: compare_test.rs (Office samples against Office's PDFs, Word's and LibreOffice's PDFs of one file, Markdown
  and its PDF, a changed Markdown file, a box drawn on a copy, two scanned agreements, a long document and a scan
  against themselves); unit tests (number formats, word keys, list numbers, XML)

## Phase 7.6 (0.7.60): Easier to use (done)
- [x] Toolbar tabs in the order work goes: File · Home · View · Comment · Tools · Sign · Help (read, review, change,
      finish); each shows its own buttons below. Read and Book stay where they were (View tab, status bar)
- [x] Menus vs panes: a tool with a short list keeps its menu (Insert, Forms, OCR, Stamp, Certificate, Theme); a long
      one becomes a pane on the right: Organize (pages, marks on every page, document, getting content out). Tools tab
      without the buttons it repeated from other tabs (rotate, delete, Comment, Sign); Stamp lists the stamps (Fill &
      Sign marks are on the Sign tab)
- [x] Panes on the right share one place (one open at a time): Organize, Sign, Convert, Markdown (for .md files)
- [x] Side bar at the right edge: thin, a button per pane (icon and its name turned on its side). View › Side bar
      (or right-click it, or its ⚙ button) ticks which buttons it shows and hides it; Settings › Side bar also puts
      them in order. Remembered
- [x] Settings (Help › Settings, File › Settings…, Ctrl+,, the command palette): one dialog with tabs: General (theme,
      dark pages, cover page, field highlights, saving rotations), Read and Book (text size, font, spacing, margins,
      alignment, paper), Side bar, Signing (a digital signature's defaults), Text recognition (languages, quality),
      Office files (which program opens them, suite choices), Compare (its options). File menu keeps files only
- [x] 0.7.61: dialogs that were settings merged into Settings: Office suite choices (with getting and removing
      LibreOffice) → Office files; OCR's downloaded languages → Text recognition (per language: built in, download or
      remove the standard and best models, get another language); Keyboard shortcuts and About → tabs of their own
      (F1 still opens the shortcuts). Choices in Settings stay on one line; "Default order" for the side bar

## Phase 7.7 (0.7.70): Themes and the Reading pane (done)
- [x] Reading pane (side bar, View › Text): Read and Book modes' text size, font, spacing, margins, alignment and paper,
      next to the text (was in Settings); Book mode, full screen, keeps its Aa panel; "Fix the reading layout" there too
- [x] Themes (Settings › Themes, View › Theme): 16 presets — Standard, Steampunk, Cyberpunk, Nord, Solarized, Forest,
      Ocean, Paper, Library, Midnight, Terminal, Blueprint, Sakura, Rosé, Desert, High contrast — light, dark or both;
      each sets the colours, a font for headings, tab names and buttons, and Read/Book's Auto paper (dark pages use
      the theme's dark paper). High contrast: pure colours, strong outlines, larger text
- [x] Six base colours per mode (title bar, toolbar, menus, behind the pages, text, accent); borders, secondary text,
      hovers, selection and button text worked out from them
- [x] Your own themes: start from any theme or from a picture (its main colour for the background, its liveliest for
      the accent, light and dark versions made); edit colours and paper, previewed on the whole window; low contrast
      marked ("may be hard to read"), never blocked
- [x] Fonts: any font installed in Windows or a font file added (kept in Paginaut's data folder). Scaled to the usual
      font's x-height, and smaller still when wide (font-size-adjust), so labels keep their room; labels are one line,
      cut with "…"; a check lists labels that still don't fit and offers "headings only". All 16 presets fit
- Backend: fonts.rs (installed families from the registry, user font files); commands fonts_installed, fonts_user,
  font_add, font_bytes, font_remove
- [x] 0.7.71: skins — a photo behind the title bar, toolbar, side bars, status bar and Home (tick "Also behind the
      pages" for the desk too), seen through the theme's colours; how much shows (10–70%), blur, on/off in View ›
      Theme; low contrast over the photo marked. Menus, panes, dialogs and the pages stay solid. The photo is kept
      shrunk to 1920 px; its blurred copy is made once per change. Backend: skin_set, skin_bytes, skin_clear
- [x] 0.7.71: Steampunk redone (brass plate, copper, oiled walnut; Bookman / Rockwell / Georgia type; brass sheen on
      the title bar and side bar, copper rule, worn desk edge); Library's dark version oxblood leather, so the two
      differ; Cyberpunk neon rules, Blueprint grid, Terminal scan lines. All presets checked: text readable on every
      colour it sits on; a note when text behind the pages is low contrast
- [x] 0.7.71: side bar easier to read (wider, larger, full-strength labels, always the plain UI font); "Make your own
      palette…"; Home's recent files on a card (readable on dark desks); fixed the picture button doing nothing after
      a test run in development builds
- [x] 0.7.72: review fixes — photos picked one after another become the skin in order (the newest wins; its file is
      never deleted by an older one); the Themes tab shows the skin's photo while it's off; a new skin photo is
      written before the old one is removed; "My palette N" never repeats a name; nothing stays decoded between
      blur changes; the skin's contrast check is memoised and numeric; a picture used for both a palette and the
      skin is read once
- [x] 0.7.72: resources checked (668-page PDF, 1280×860): scrolling a steady 60 fps with no long tasks, skin off,
      on the bars, or behind the pages too; the skin costs ~14 MB of GPU memory and no measurable CPU. The status
      bar's memory figure cost ~8 ms every 3 s (a list of every process on the PC): now ~1 ms (Paginaut's own
      processes asked directly, the full list every 30 s), and nothing while minimized. Reading positions are
      only saved when they moved

## Phase 8 (0.8.0): Packaging, updates and signing
- [ ] GitHub repository hosting releases and update files — releases go to the source repo itself
      (update.rs `RELEASES` = adityapahurkar9/paginaut, from 0.9.12; earlier builds looked at an unused
      paginaut-releases), which must be public for the app to read them (and the AGPL wants the source out
      anyway). tools/release.ps1 -Publish and tools/android.sh --publish upload; release/ keeps the newest 5.
      Published 2026-11-22: v0.9.12 (latest), archives v0.9.7–v0.9.11 and android-v0.3.0–v0.6.7 (never "latest").
      Waiting on the repo being made public; until then the app's update check can't read it
- [x] Help › Check for updates and Settings › About › Updates (check, "Update and restart", progress, stop),
      plus a quiet check once a day (on by default, can be turned off; only github.com is asked). Our own updater
      on WinHTTP, no second HTTP stack: reads latest.json (Tauri's format), accepts only installers from the
      releases repo, checks the minisign signature against the key built in (update key in
      %USERPROFILE%\.paginaut, never in the repo), runs the installer as Tauri's updater would (/P /UPDATE /R)
      and quits; unsaved documents must be saved first
- [x] Signed add-on list (addons.json + .sig, published with each release; `-AddonsOnly` re-signs an edited one):
      the OCR languages on offer (Tesseract tag, sizes, hashes) and optionally a LibreOffice release pinned by
      version and SHA-256. Checked lists are kept for offline use; a list can only name files from the places
      Paginaut already downloads from. (No voices yet: there are no voice add-ons to list)
- [x] Dependency routine: tools/deps.ps1 (MuPDF and tessdata releases, cargo update --dry-run, cargo audit if
      installed, npm outdated/audit; -Apply updates within compatible versions behind the test gate and puts the
      lock files back if anything fails), tools/test-all.ps1 (TypeScript, unit tests, 10 engine suites, -Office
      for the Office ones), .github/dependabot.yml (monthly grouped PRs + security alerts once on GitHub)
- [ ] Installer code signing — prepared: bundle.windows.signCommand runs tools/sign.ps1 on every binary and the
      installer, a no-op until PAGINAUT_SIGN_THUMBPRINT (certificate in the store) or PAGINAUT_SIGN_DLIB/METADATA
      (Azure Trusted Signing) is set. Waiting on a certificate (options in the Phase 8 notes)
- [x] Settings › Default app: tick the kinds Paginaut is offered for (PDF, XPS, e-books, comics, Markdown;
      pictures and Office files unticked unless picked), "Make Paginaut the default…" opens Windows' Default apps
      at Paginaut (IApplicationAssociationRegistrationUI, the ms-settings link as a fallback), each kind shows
      whether Paginaut opens it now. Everything under HKCU (ProgIDs Paginaut.<kind>, OpenWithProgids,
      Capabilities, RegisteredApplications); never takes a default. The installer runs `paginaut
      --register-types` (keeps the user's choice, first install offers the core kinds) and a real uninstall
      `--unregister-types`. ProgIDs that may be someone's default are never deleted while installed (Windows
      resets a default whose ProgID disappears); a kind Paginaut is the default for can't be unticked
- Replaces the installer's own file associations (Tauri's, which set each type's class): earlier versions'
  ProgIDs ("PDF document" …) are kept and pointed at the new copy, and removed on uninstall
- [x] 0.8.1: review fixes — an upgrade that uninstalls first keeps what was offered (the uninstall leaves a note,
      read within the hour) and recreates any ProgID a default still names (so 0.7's "PDF document" default
      survives its old uninstaller); only an installed copy offers itself (a dev build or copied folder shows the
      tab read-only), and an old ProgID is only re-pointed when its program is gone; a check forgets an update
      it no longer finds; earlier update installers are cleared before a download; the add-on list's OCR
      catalogue is shared, not copied per lookup; release notes reach GitHub through a file; the tab's grouping
      comes from the app

### 0.8.2: read aloud, better signatures, OCR and small tools
- [x] Read aloud with the voices installed in Windows (WebView2's own speech has none, so the text is spoken
      natively through Windows.Media.SpeechSynthesis, with each word's time). View › Read aloud, the Reading pane,
      the command palette, Ctrl+Shift+U; starts at the selected text or where you are. Read and Book modes tint
      the paragraph and mark the word, keeping it in view (not for a few seconds after you move the view
      yourself); Book mode turns its pages; the page view follows the pages. A bar with pause (Ctrl+Shift+Space),
      previous/next paragraph, stop, voice and speed; a click on a paragraph carries on from there. With
      Windows' voice chosen, a paragraph in another script (Hindi, Arabic…) gets an installed voice for it.
      The next paragraph is made while one plays; a closed tab, new text or layout corrections stop it
- [x] Signatures: .pfx files, trusted timestamps, revocation check (see Phase 4)
- [x] OCR: sideways pages, Recognize again (see Phase 5)
- [x] Bookmarks from headings; Excel percentages (see Phase 6)
- [x] Fix: MuPDF writes 0.5 as ".5" (compact PDF numbers), which isn't JSON: Read mode never finished on a
      document with text under 1 pt, and search matches, links, bookmarks, fields and tables could fail the same
      way near a page's edge. JSON goes through json_printf now; Read mode also leaves out a page it can't read
      rather than waiting forever
- [x] Review fixes: speech ends COM on the shared worker threads after each call; a click on Book mode's
      page-turn edges doesn't move the reading; turning a page doesn't stop it; a removed voice falls back to the
      script match; voice/speed changes while paused stay paused; the Recognize dialog counts recognized pages
      from the page objects (no page loads under the document lock); Excel keeps "12.0%"'s decimals

## Android viewer (first build)
A quiet reader for phones and tablets from the same engine: no editing, no accounts, no ads, and no
internet permission at all (release builds). Kept clearly apart from Adobe's reader in looks and words
(no "Liquid Mode": the reflowed text is Read and Book, as on Windows).
- [x] Toolchain: NDK 29, all four ABIs (arm64, armv7, x86, x86_64), Android 7.0 (API 24) and later.
      MuPDF built for Android by native/build-mupdf-android.sh (no OCR, barcodes or .docx writer);
      native/ocr_none.c stands in for the Tesseract bridge
- [x] Rust split: app.rs holds the viewing commands both apps use, desk.rs the desktop's; everything
      Windows-only is cfg(desktop). mobile.rs is the Android entry point and the bridge to the Kotlin plugin
      (gen/android/.../PaginautPlugin.kt)
- [x] Opens PDF, XPS, EPUB, MOBI, FB2, CBZ, Markdown and pictures: the file picker, "Open with" and Share
      from other apps (copied into the cache; Recent reopens from the source, or the copy). E-books open in Read
- [x] Reader: continuous, one by one (snapping) or two pages (wide screens); pinch and double-tap zoom
      (Viewer.ts touch pinch); Read and Book (one page on narrow screens, turned with a swipe); night mode;
      search (pages or reflowed text); contents, comments list, document info; password prompt
- [x] Share (Android's sheet), Print (Android's print service; e-books and XPS written as a PDF first),
      read aloud with Android's own installed voices (by the paragraph's script), with pause and speed
- [x] Bars a tap shows or hides, system bars hidden while reading; the back button closes sheets, search,
      then the document; positions saved as you read
- [x] Works back to WebView 79 (CSS and JS lowered by esbuild, replaceChildren polyfilled); a WebView older
      than that gets a message to update it. Tauri's Jackson pinned to 2.13 (2.15 crashed Android 7)
- [x] Logo #4 redrawn as assets/icon.svg (also the Windows icon); Android adaptive and themed icons
      (assets/android-icons.py)
- [x] Release signing key (0.3.0, 2026-10-08): an upload key (RSA 4096) in %USERPROFILE%\.paginaut\paginaut-upload.jks,
      its passwords in android-signing.properties beside it (both must be backed up, like updater.key); Gradle signs release builds
      with it when the file is there. `bash tools/android.sh all --release` makes the Play bundle (.aab, all ABIs)
- [ ] Store listing, AAB upload (a personal Play account must run a closed test with 12 testers for 14 days
      first); source made public first (AGPL); 1.0 for the store release
      Listing drafted 2026-11-22: docs/PLAY-LISTING.md (name, short and full description, category, App content
      answers), assets/play/ (512 px icon, feature graphic; render.mjs remakes them), PRIVACY.md. Screenshots to take
- [x] 0.6.5 (6005) uploaded to Play (2026-10-27), internal testing draft. Automatic protection (App integrity) is
      turned off and the bundle lists three permissions again: its installer check added
      com.android.vending.CHECK_LICENSE, which MainActivity.extraPermissions() doesn't allow, so every Play copy
      would have shown "Paginaut stopped". Keep it off (it also clashes with the AGPL's right to share copies)
- [ ] Read aloud, as far as the desktop's (0.8.2) and further. Today: Android's installed voices picked by the
      paragraph's script, pause and speed. Wanted, and not only these:
      - the word being read marked (and its paragraph tinted) in Read and Book modes, kept in view; the page view
        follows the pages. UtteranceProgressListener.onRangeStart gives word ranges from Android 8 (API 26);
        Android 7 has none, so time words by length there, as the desktop does with Piper
      - choosing the voice: every installed voice for the language, a sample before picking, the script match
        kept for paragraphs in other scripts; a way to Android's own settings to install more (no Piper
        download: the app has no internet)
      - faster and slower in finer steps, and pitch; changes while paused stay paused
      - back and forward by sentence and by paragraph; a tap on a paragraph carries on from there
      - worth considering: reading on with the screen off or from the notification (a media session; a
        foreground service adds permissions, so the manifest check in build.gradle.kts and
        MainActivity.extraPermissions() must allow them), and stopping after a set time
- [x] Native debug symbols for Play (Android 0.6.8): the Play bundle carries the symbol tables itself
      (build.gradle.kts debugSymbolLevel SYMBOL_TABLE; tools/android.sh builds the release library unstripped and
      Gradle strips what it packs), so no zip to upload by hand. See "Error logs" below
- [ ] README.md: the Play app signing certificate's SHA-256 (App integrity › App signing) in place of
      PLAY_APP_SIGNING_FINGERPRINT
- [x] LICENSE file (the AGPL 3.0 text) at the repo root before the repo goes public; package.json already says
      AGPL-3.0-or-later
- [x] CBR comics: unarr (RAR 1.5–4, solid ones too) and libarchive (RAR 5) behind native/cbr.c; RarLab's unRAR left out (its licence clashes with the AGPL)
- [x] 0.3.1, 0.4.0 (2026-10-13): Paginaut's Standard theme, then the desktop's 16 colour themes (src/themedata.ts shared by
      both apps; Settings: theme previews, light or dark); Read and Book's plain paper takes the theme's; the shelf's
      folder picker starts in Download (Android 11 and later refuse Download itself: a tip says so)
- [x] 0.5.0: "Search and pick books" (Android's picker, several at once; picked books join the shelf); the whole-phone
      scan looks in Android/media (WhatsApp's documents) but never another app's data. mobile.rs checks every path
      handed to the engine (the cache, or a readable document on shared storage; writes only to the cache). Builds stop
      if the manifest gains a permission, an exported component or wider sharing; a copy that asks for more shows
      "Paginaut stopped". docs/PLAY-DATA-SAFETY.md: the Data safety answers and a privacy policy draft
- [x] 0.5.1: Hide from screenshots (View sheet): Android's secure window, on before the first frame after a restart
- [x] 0.6.0: MuPDF in its own isolated process (EngineService, android:isolatedProcess: no permissions, files or
      network). The app opens each document read-only and passes the open file over a socket (engine_link.rs, handles
      by SCM_RIGHTS); temporary files are made by the app and handed over (engine/sandbox.rs). If the engine stops, a new
      one reopens the documents (passwords too) and searches and Read mode carry on; a watchdog lets go of one that
      hangs; a document that stops it twice in two minutes is refused, and the reader says so. Message sizes capped
- [x] 0.6.1–0.6.3: the engine's temporary files capped (512, none when the phone is nearly full); printing files over
      128 MB read in place; review fixes (printing off the main thread, close and cancel watched, quiet searches count
      as a stuck engine). tools/android-engine-test.sh: 23 checks
- [x] 0.6.4, 0.6.5 (2026-10-26): password-protected PDFs show their pages (engine fix shared with desktop 0.9.8); a
      wrong password after unlocking leaves the pages drawing; errors that aren't a wrong password say what happened;
      after an engine restart, a password that no longer opens the file closes it with a message. The engine test
      checks locked pages draw (5 checks; 25 pass with the test files in place)
- [x] 0.6.6 (6006, 2026-11-10, internal testing): a file sent from another app while Paginaut is closed opens (it said
      the file wasn't there; see BUGS.md). Checked on the Pixel 9 emulator, debug and release; still to check on a
      phone with Gmail and WhatsApp. Carries the shared engine's fixes since 0.6.5 (desktop 0.9.10 and 0.9.11).
      tools/android-engine-test.sh: 20 of 25 pass; the 5 that don't need businessplans.epub (a 206-page book that
      needs repairing), which isn't on this PC, and ran on a stand-in
- [x] Desktop 0.9.10 changed native code the Android build compiles in; built and tested in 0.6.6. What changed: paginaut.c's PDF/A code
      (fo_pdfa_*, unused on Android) now includes MuPDF's FreeType headers (build.rs adds the include path for
      Android too) and calls FreeType under fz_ft_lock; fo_sign_page took a new last argument (pdfa; signing is
      desktop-only); Document::pdfa_part and Page::sign's new argument are in the shared engine

## PDF/A: archival PDFs (-2b and -3b in 0.9.9; -1b, Office files, signing and saving in 0.9.10)
PDF/A (ISO 19005) is the PDF that still opens and looks the same decades later: what courts, government portals and
archives ask for. First PDF/A-2b (looks preserved; allows transparency, JPEG 2000 and layers), then -1b (older
portals) and -3b (any attachment, as e-invoices need). The "a" levels need tags, so they wait for tagging (below);
"u" (every letter maps to Unicode) comes with -2b's checks. MuPDF has no PDF/A support: checks and fixes are ours
(paginaut.c fo_pdfa_*, engine/pdfa.rs).
- [x] Check (-2b, -3b): what a file claims (XMP pdfaid) and which rules it breaks, in plain words and grouped, with
      pages: fonts not in the file (and font entries without their type), XMP with properties PDF/A doesn't know (the
      document's, or on pages and pictures), device colours no output intent or default colour space covers,
      JavaScript and forbidden actions, encryption, annotations and fields that don't print or have no (or extra) appearances, XFA,
      NeedAppearances, attachments (-2: PDF/A files only; -3: described), layers, LZW, external and PostScript content,
      picture alternates, transfer functions, PDF 2.0, missing XMP or details that disagree with it. Objects are walked
      from the catalog; fonts and colours come from a content processor over every page and appearance (what's used).
      Done in C over MuPDF's objects (a Rust walk would have needed a large FFI), worded in Rust
- [x] Check, 0.9.10: XMP read properly (its own small XML parser: MuPDF's drops namespace prefixes; UTF-8, -16 and -32)
      and held to veraPDF's lists of XMP 2005 (and, for -1, 2004) properties and value types, extension schemas
      included; glyph widths against the font programs, characters a font hasn't got (.notdef), non-symbolic
      TrueType encodings, CID fonts' character collections and CID to glyph maps; output intent profiles (class,
      colour space, ICC version, one profile for every intent); transparency groups' blending spaces, Type 3 glyphs'
      colours, inline pictures' abbreviated colour spaces; actions on an allow-list (set-state and no-op ones out);
      rendering intents, blend modes, halftones; content using resources it doesn't hold; JPEG 2000 headers;
      size limits (numbers, strings, names, q nesting, page boundaries); attachments associated by any object's AF.
      Document info need only match the XMP for -1 (-2 and -3 only recommend it)
- [ ] Check: letters without Unicode ("u"); operators PDF doesn't define and broken hex strings in content (MuPDF's
      interpreter skips them without telling)
- [x] Save as PDF/A-2b or -3b (new file, "… (PDF-A).pdf"): fonts embedded from the system's (Windows registry,
      fontconfig; licences that forbid embedding respected; each stand-in named with why), MuPDF's copies for the base 14, widths
      taken from the program; sRGB output intent (MuPDF's built-in profile) and default colour spaces where device CMYK
      is used; XMP written from the document info, keeping the old packet's standard properties (rights, subjects,
      document IDs) when PDF/A could take it; symbol fonts' codes spelled out by glyph name; JavaScript, encryption (needs the permissions password) and forbidden
      actions removed; appearances drawn; hidden annotations and fields removed; LZW recompressed; damaged page trees
      rebuilt as Repair does. Checked again after saving; what still fails is listed
- [x] 0.9.10 fixes to match: widths taken from the font programs (simple, CID and Type 3 fonts), PDF/X intents with
      their own profile dropped, groups' blending spaces made ICC-based, page resources put on the page, halftones and
      rendering intents put right, form fields' own actions removed, buttons' looks under their state names, an empty
      look for annotations MuPDF can't draw, signatures taken out of copies (they can't stay valid), JPEG 2000 with
      bad headers stored again without loss, document dates written in full, info entries that aren't text dropped;
      pictures' XMP kept only when every property can be checked
- [x] Save as PDF/A-1b (0.9.10): PDF 1.4 (no layers, attached files or JPEG 2000; 16-bit pictures as 8-bit), CharSet
      and CIDSet glyph lists for font subsets, comments of kinds PDF 1.4 hasn't got kept as stamps (same look), a CMYK
      output intent for CMYK documents, ICC profiles below version 3. Transparency groups with nothing see-through
      are taken off; real transparency is listed as something PDF/A-1 can't hold (PDF/A-2b can)
- [ ] PDF/A-1b: transparency flattened (a page rendered to a picture as the last resort, said so)
- [x] In the app: PDF/A in the Convert pane's targets (checked as soon as it's chosen, with a warning for signed
      files); Properties (Ctrl+D) shows a file's PDF/A claim, a Check button, and a note that saving says if a change
      would break it. 0.9.10: PDF/A-1b among the kinds
- [x] Saving changes to a file that says it's PDF/A (0.9.10): the problems the changes bring (not the file's own) are
      listed first, with Keep it PDF/A (the Save as PDF/A fixes on the document itself, one undo step, signatures
      kept, saved as an update), Save anyway or Cancel; checked again after saving
- [x] Office files to PDF/A (0.9.10): Convert › Files to PDF makes PDF, PDF/A-1b, -2b or -3b; the Office conversion
      (cached) is saved as PDF/A, with what's left and the fonts that stood in shown by each file
- [x] Signing a PDF/A file keeps it PDF/A (0.9.10): the signature's look prints, has its normal look only, gets MuPDF's
      base 14 fonts and default colour spaces, all inside the signing's revision
- [x] Tests: examples/pdfa_test.rs (every testfiles PDF conforms after saving, looks the same, keeps its pages;
      attachments for -2 and -3; protection; unsaved changes; XMP kept and dropped; untyped fonts: samples from
      tools/make-pdfa-samples.py); unit tests for font names, licences and the stand-in messages
- [x] veraPDF (the reference validator, Java; not shipped) passes every saved file: the 16 from testfiles and the 20
      Office-made PDFs in testfiles/office (tools/pdfa-validate.ps1). It found what our check missed, now fixed:
      symbolic fonts read through the encoding they name (ZapfDingbats check marks), wrong CIDSet glyph lists in
      font subsets, XMP with custom properties on pictures, font entries without their type
- [x] veraPDF's test corpus (2,906 files; tools: examples/pdfa_corpus.rs, then tools/pdfa-validate.ps1 or veraPDF on
      the folder). Our check on the files veraPDF expects to fail and to pass, and veraPDF on what Save as PDF/A
      makes of each:
      | | fail files caught | pass files we flag | our copies veraPDF passes |
      |---|---|---|---|
      | PDF/A-1b | 278 of 306 | 0 of 263 | 547 of 569 |
      | PDF/A-2b | 537 of 609 (161 before) | 0 of 377 (5 before) | 936 of 986 (840 before) |
      | PDF/A-3b | 5 of 5 | 0 of 7 | 12 of 12 |
      What veraPDF still fails is mostly what a copy can't change without rewriting page content (sizes beyond
      PDF/A's limits, broken hex strings, inline pictures, characters fonts haven't got), which the check lists as
      left
- [x] examples/pdfa_keep_test.rs: a comment added to a PDF/A file is said to break it, kept, saved as an update and
      still PDF/A; a signed PDF/A file still PDF/A with a valid signature (-1b, -2b, -3b; veraPDF passes all).
      examples/pdfa_office_test.rs: Word, Excel and PowerPoint files to PDF/A (veraPDF passes 14 of 15: Excel's
      see-through chart can't be PDF/A-1b)
- [ ] Tests: the user's portal files

## PDF/UA: checking accessible PDFs (planned, after PDF/A)
PDF/UA (ISO 14289) is the accessible PDF: tagged, so a screen reader reads it in order, with pictures described and
tables explained. Accessibility laws (government, education, banks) ask for it. PDF/UA-1 first (PDF 1.7, what checkers
and portals use), PDF/UA-2 (PDF 2.0) later. MuPDF reads tag trees (structured text with structure, its tree check) and
keeps marked content through its content filter; the checks are ours.
- [ ] Check: the Matterhorn Protocol's failure conditions a program can decide (87 of 136), in plain words, grouped and
      tied to the page and the thing at fault: content neither tagged nor marked as decoration, pictures without
      alternative text, headings that skip levels, tables without header cells, broken lists, language or title
      missing (or the title not shown in the window), letters without Unicode, links, comments and fields without
      tags or descriptions, tab order not following the tags, no PDF/UA identifier in the XMP. The checks only a person
      can make (the alternative text says what the picture shows, the order makes sense, colour isn't the only signal)
      as a list to tick
- [ ] Tags panel (a side bar tab): the file's tag tree; picking a tag shows its content on the page; the reading order
      as numbered boxes on the pages (view only here; editing comes with tagging below)
- [ ] Tags kept through Paginaut's own tools: every tool that rewrites pages or the catalog checked on a tagged file
      (page delete, insert, reorder, merge, split: MuPDF's rearrange rebuilds the catalog, so restore_catalog must carry
      StructTreeRoot, ParentTree, MarkInfo, RoleMap, Lang; redaction, Edit mode, stamps, OCR text, flatten, Sanitize,
      Reduce size, Save as PDF/A). Each keeps the tree consistent (content moved or removed updates the tree) or says
      the file will lose its tags. Engine test: one tagged file through every tool, then the checker
- [ ] Tests: the PDF Association's PDF/UA reference files; the free PAC checker's results on the same files to
      compare against

## Accessibility tagging and repair (planned, after PDF/UA checking)
Tags added to a file that has none (most PDFs), then put right by hand. The hardest of the three: the big apps tag
with machine learning trained on huge numbers of documents. Ours starts from Read mode's analysis (headings,
paragraphs, lists, columns, running headers and footers, footnotes) and the table finder (engine/table.rs): good on
plain documents (letters, reports, books), hand-fixing expected on complex layouts. UI wording our own (no other
app's feature names).
- [ ] Add tags: the tag tree written from the analysis (H1–H6, P, L/LI, Table/TR/TH/TD, Figure, Caption, Note), page
      content marked to match through the content filter (as Edit mode and Redact rewrite pages), running headers,
      footers and page numbers marked as decoration; ParentTree, RoleMap, MarkInfo, language, title shown in the
      window, PDF/UA identifier. Scans after OCR (its text tagged)
- [ ] Put right by hand: reading order as numbered boxes (drag to reorder, draw a box for a new block, change a
      block's kind); the tag tree editable (move, change kind, merge, split, mark as decoration); alternative text
      per picture (asked for every picture without it); tables: header cells and their scope, spans, from the grid
      Read mode's table menu shows; descriptions for links and fields; tab order set to follow the tags
- [ ] Checked again after each change (the PDF/UA check above, its list updating); saving keeps the tags
- [ ] With tags: PDF/A-2a and -3a offered in Save as PDF/A; Word and LibreOffice exports in Convert keep their tags
      (their accessibility tag options switched on)
- [ ] Files that have tags read better: Read mode and Read aloud follow the file's own tags and alternative text
      instead of guessing
- [ ] Tests: untagged documents of rising difficulty (letter, report, two columns, tables, forms, a scanned book):
      add tags, then the checker; compared with Acrobat's results on the same files (installed on the user's PC)

## Error logs (Windows, Linux and Android; first part in 0.9.13 and Android 0.6.8)
Before 0.9.13 a failure was only what the app showed at the moment: the engine's eprintln! output goes nowhere in a
release build (no console on Windows, none on Android), and nothing was kept for a bug report. Now every version writes
a log of its own on the device and lets the user hand it over. Nothing is ever sent anywhere by the app: the log leaves
the device only when the user copies, saves or shares it, or opens a GitHub issue in the browser and posts it
themselves (docs/PLAY-DATA-SAFETY.md stays "no data collected").
- [x] One log for all three (src-tauri/src/errlog.rs, std only, no log crate): `logs/paginaut.log` in the app's local
      data folder (Windows %LOCALAPPDATA%\app.paginaut.reader\logs, Linux ~/.local/share/app.paginaut.reader/logs,
      Android the app's private data folder), rolled over to paginaut.1.log at 256 KB, so never more than 512 KB.
      Lines are `time [kind] message`, UTC; a message repeated within a minute is written once, with a count
- [x] A panic hook writes the panic, where in the code, and its thread before the process ends (release builds abort
      on panic: the hook runs first). Desktop: a "running" marker; found at the next start, the log says the last
      session ended without closing (a MuPDF crash, which no panic hook sees, or the app being stopped)
- [x] Android: the engine stopping, being started again, not starting, being stopped for hanging or filling the phone,
      and a document refused for stopping it twice (engine_link.rs; the engine's own process is isolated and can't
      write files). Uncaught Kotlin/Java exceptions (MainActivity.catchCrashes) are written raw to
      logs/crash-pending.txt and cleaned into the log at the next start, so there's one way of cleaning
- [x] Front-end errors: window.onerror and unhandled promise rejections (src/errors.ts, both pages), and the commands
      whose failures the user sees (opening, saving, converting, printing, OCR, downloads: api.ts LOGGED), at most 60
      lines a window
- [x] What it never holds: the home folder becomes ~, a path or a document's name becomes `<file>.pdf` (the extension
      kept), quoted names too; web addresses and source-code file names (a panic's place) stay. Unit tests in errlog.rs
      cover the cleaning, the roll-over and the dates
- [ ] Cleaning: a document's name with spaces, written without its folder and without quotes ("couldn't open Tax
      Return.pdf"), loses only its last word ("Tax <file>.pdf"). Messages from the app carry full paths or quote
      names, so none is known to do this; a message that does should quote the name
- [x] Report a problem, the same on all three: the user picks what to include (system details, the error log: each a
      switch, both on to start), the page says what each holds and what's never in it, and shows exactly the text
      that would be shared. Nothing leaves the device unless the user sends it; Paginaut itself sends nothing.
      Desktop (Settings › About › Report a problem…, the command palette; a tool in the right pane,
      src/ui/ReportPane.tsx): "Open a GitHub issue with this report", "Copy this report", "Save this report as a
      text file…", "Clear the error log". Android (Settings › Report a problem, src/mobile/ReportSheet.tsx): "Share
      this report as a file…" (share sheet), "Open a GitHub issue with this report" (the browser: no internet
      permission is needed, and none was added), "Clear the error log"
- [x] GitHub: an issue form, .github/ISSUE_TEMPLATE/problem.yml ("Report a problem", labels bug and from-app). The app
      fills its `platform`, `system` and `log` fields from the address (src/errors.ts issueUrl, as much of the log's
      end as fits in 7,500 characters); the user writes what happened and posts it. Needs the repository public
      (a private one takes issues only from its collaborators) and the from-app label made
- [x] MuPDF's errors into the log (paginaut.c `logged`, mupdf.rs `mupdf_error`): at most 20 lines a minute, the rest
      counted; "aborted" (a page drawing stopped) left out. Its warnings stay off: a damaged file gives hundreds. On
      Android the engine's own process can't write, so its MuPDF errors aren't logged; its stops are. The two
      eprintln! calls (appwindows.rs) are log lines now
- [x] After a crash: the next start offers Report a problem once (desktop: a question, "Report a problem…" or "Not
      now"; Android: the sheet opens with "Paginaut closed unexpectedly last time"). Rust's `crash_notice`
- [x] Android's Play bundle carries the native libraries' symbol tables (build.gradle.kts debugSymbolLevel
      SYMBOL_TABLE, ndkVersion set; tools/android.sh builds the release library unstripped, Gradle strips what it
      packs), so Play Console stops asking for native debug symbols and its crash reports name functions
- [ ] More detail in the log when turned on in Settings
- [ ] On Linux, tried in the running app (WebKitGTK): the pane, the crash offer, the issue form filled in

## 0.9.14 (Windows, Linux): updates for Linux, update versions checked, Tauri 2.12.7 and rustls 0.23.46
Version set 2026-11-22; not built for release yet.
- [x] Linux is offered updates: tools/release.ps1 signs the .deb and the AppImage with the update key and lists them in
      latest.json (linux-x86_64-deb, linux-x86_64-appimage), which update.rs has looked for since 0.8.10 but no
      release ever listed, so Linux was never offered one. Publishing stops if they're missing. Linux users on
      0.9.12 and 0.9.13 (the builds that read this repository) are offered 0.9.14; older ones read an unused repo
- [x] An update's version is checked (BUGS.md, security, low): the signed file name in the installer's signature
      (`file:Paginaut_0.9.14_x64-setup.exe`, signed by the release key with the file) must be the version latest.json
      offers, or nothing is installed (update.rs signed_version). An old signed installer can't be passed off as a
      newer one; the release process is unchanged. Unit tests; staged_release (--ignored) checks all three packages
      of a staged release. Tried on the 0.9.13 files with the Linux ones signed by the new release.ps1 step
- [ ] Tried in the running app: a Linux 0.9.13 offered 0.9.14 once it's published (the .deb and the AppImage)

Dependencies, 2026-11-22, before the repo goes public: trivy flagged rustls 0.23.45
(four advisories, low/medium; used only by ureq for update checks and timestamps), so it moved to 0.23.46.
Tauri moved within 2.x: crates 2.12.1 → 2.12.7 (tauri-build/codegen/macros 2.7.6, runtime-wry 2.12.5), plugins
opener 2.7.3, dialog 2.8.4 (rfd 0.16 → 0.17), fs 2.7.1, single-instance 2.5.5; npm CLI 2.12.6, API 2.12.5.
glib 0.18.5 (one medium advisory, Linux only) stays: Tauri 2 pins gtk 0.18.
- [x] Windows: tools/test-all.ps1 passed (TypeScript, Rust unit tests, all engine suites)
- [x] Android: debug x86_64 APK builds (tools/android.sh x86_64 --no-install); not run on an emulator
- [x] Linux: tools/linux.sh test passed (59 tests, the SoftHSM token one included). WSL first had no network:
      mirrored networking left in ~/.wslconfig from a lancache setup failed (0x8007054f); back on NAT now
- Once, tauri was found back at 2.12.1 in Cargo.lock after the update (plugins kept theirs); not reproduced
  through cargo check, test-all, the Android build or the Linux tests. Check Cargo.lock before a release
- [ ] File dialogs (open, save, pick folder) tried by hand on Windows and Linux: rfd 0.17 draws them, and the
      tests answer dialogs through window.paginautTest instead of opening them

## 0.9.13 (Windows, Linux) and Android 0.6.8: error log and Report a problem
Versions set 2026-11-22. Built and published 2026-11-22 as the latest GitHub release (v0.9.13: Windows installer +
.sig, latest.json, addons.json, the Linux .deb and AppImage; tools/test-all.ps1 passed first). Android 0.6.8 published
the same day as android-v0.6.8 (the Play bundle, with native symbols, and the four per-ABI APKs; not latest);
not uploaded to Play yet. What's in it: "Error logs" above (the
ticked items). Tested: the TypeScript check, both pages built, the Rust unit tests on Windows and Linux (WSL).
In the running Windows app (dev build): Report a problem opens from Settings › About with the right details; a
thrown error, an unhandled rejection, a repeated line, a failed open with a full path and a MuPDF error on a
broken PDF all reached the log cleaned; unticking a part takes it out of the text; a forced stop made the next start
ask "Paginaut closed unexpectedly last time", and its button opened the pane; a clean quit removed the marker.
On Android (debug build, Small_Tablet emulator, Android 11): `am crash` on the app's process was written by the
Kotlin handler and cleaned into the log at the next start, which opened the report sheet; the switches change the
text; "Share this report as a file…" handed paginaut-report.txt to the share sheet. On the engine's process it
crashed only the engine, which the app started again. The GitHub issue address was checked apart from the app,
not opened: the form isn't on GitHub until it's pushed. Not run on Linux.

## 0.9.12 (Windows, Linux) and Android 0.6.7: the 2026-11-13 code review fixed
Versions set 2026-11-21. Built and published 2026-11-22 as the latest GitHub release (v0.9.12: Windows installer +
.sig, latest.json, addons.json, Linux .deb and AppImage), the first build whose update check reads adityapahurkar9/paginaut.
Android 0.6.7 built 2026-11-21: release/android/Paginaut_0.6.7.aab and the four per-ABI APKs (upload-key signed,
version code 6007); its build caught the Roman numeral check living in desktop-only code (moved into the engine).
Tested: the
TypeScript check, the Rust unit tests on Windows and Linux (WSL), and every engine suite with the Office ones
(tools/test-all.ps1 -Office), plus pdfa_test and dsc_test. Not tried in the running app, and the Android changes
weren't built. Every entry of the review in BUGS.md is ticked with what was done, except the update's version
signature, left open by choice (it needs latest.json signed at every release). Most important:
- [x] Attachments open only when they're documents, pictures, text, sound or video, read from the name as Windows
      stores it ("invoice.exe " was opened as a program)
- [x] Signing writes beside its target and replaces it only when it worked; the replace waits for readers, as saving
      does; a signature's byte range must cover the file from its start with only the signature left out
- [x] An update looks for unsaved changes again after its download, before anything quits
- [x] Saving: one save of a tab at a time; Ctrl+S takes in the field or text box being typed in; the file is written
      afresh after Sanitize or anything deleted (removed content no longer stays in its earlier bytes; signed files
      are still only added to)
- [x] Signatures: one that certifies the document or locks fields is judged by the changes saved after it; an
      expired certificate is valid only with a trusted timestamp; Linux checks the timestamp answers this request
- [x] Web links on pages open (http, https, mailto only); Markdown tabs keep what was typed when moved between
      windows; a Markdown file that isn't UTF-8 opens read-only with "Save a UTF-8 copy"
- [x] Smaller ones: per-tab timers for Find and saving rotations; Piper, OCR models and LibreOffice set up once at a
      time; hung Office programs ended; decimal commas stay text in Excel; real Roman numerals only as page numbers;
      Book mode splits Chinese, Japanese and Thai; page label ranges kept apart; OCR layers survive redaction;
      inserted pages don't drag their whole file along; Save as Word streams its pictures (Zip64 past 4 GB)
- [x] Settings › Read aloud (Windows) points to the English (India) speech pack (Heera, Ravi): Piper still has no
      Indian English voice
- [ ] Android 0.6.7: a file sent from another app on a cold start still failed at random on a phone (reported
      2026-11-21): now asked for up to about 8 seconds, once more when the page takes it, with "Opening…" shown.
      Opens on a cold start on the Pixel 9 emulator (6 of 6); to check on a phone with Gmail and WhatsApp (BUGS.md,
      open). The engine-crash suite (tools/android-engine-test.sh) needs a debug build and wasn't run

## 0.9.11 (Windows, Linux) and Android 0.6.6: the fixes in BUGS.md, tools in the right pane
Built 2026-11-09: release/v0.9.11 has the Windows installer (+ .sig, latest.json, addons.json), the Linux .deb and
AppImage; not published. Android 0.6.6 built 2026-11-10: release/android/Paginaut_0.6.6.aab and the four per-ABI
APKs (upload-key signed); android-engine-test passes on the Pixel 9 emulator (the e-book checks need
businessplans.epub, not on this PC), and a file sent while Paginaut is closed opens (emulator; to check on a phone).
Every bug from the 2026-11-05 code review and the three Redact reports, fixed 2026-11-09 (details in BUGS.md):
- [x] Unsaved redactions, text and picture edits and Sanitize reach search, Read and Book mode and Compare
      (Doc::content_changed, one place for every edit of page content)
- [x] A comment that fails half-way leaves nothing behind; "Keep it PDF/A" is a step in the tab's history
- [x] "Save a copy" stays on the original (the buttons said "Save as"; they say "Save a copy" now)
- [x] Linux: paths compared with case (two files that differ only in case are two files); default author from USER
- [x] Attachments that can run programs on Linux, and more Windows kinds, are refused to open
- [x] A tab closed while its file loads closes the document; signing an Office file's PDF into a new file makes it a
      PDF of its own; idle workers let go of closed documents at once; a fault in one page ends a search with an
      error instead of spinning; saved signatures' ids unique; font names can't leave the fonts folder; Redact's
      Phone pattern doesn't take the end of longer numbers
- [x] Page labels from the catalog alone (no page loaded); Aadhaar checks read a page's text once; updates checked
      in pieces; one "write beside, then rename" for merge, split and flattened copies
- [x] Redact is a pane on the right (with a side bar button) that makes room for the page; the box shows as it's
      dragged; dragging over text that's part of the picture says why a box was marked
- [x] Android: a file sent from another app while Paginaut is closed is read as soon as Android hands it over, and
      a "not found" from the sending app is asked again once (to check on a phone with Gmail and WhatsApp)
- [x] A save that fails keeps every unsaved change and its undo steps (it used to go back to the file on disk)
- [x] Tools that were dialogs over the page open in the pane on the right, the page in view beside them: crop and
      resize, page numbers / headers / footers, watermark, page labels, insert pages, merge, split, recognize text,
      tables, and Read mode's Fix layout (src/toolpanes.ts, ui/ToolFrame.tsx). One tool at a time; opened from
      Organize (or Reading) it has a "‹ Organize" link back and returns there when done; Esc does the same, ✕ closes
      the pane. Every right pane is one width (360 px), so nothing moves when a tool opens. Still dialogs: Document
      properties, Protect / Reduce size / Repair, Save all pictures, Compare's file choice, Settings
- [x] Read aloud: on the pages (not only in Read mode) the paragraph being read is tinted and the word being said
      marked (the page's words from the engine, page_words, matched to the text being read); "Read aloud from
      here" on a page's right-click menu starts at the word clicked; the button moved from View to Home
- [x] Page numbers, headers and footers, and watermarks show on the pages while they're set up, as Add will draw
      them (stamps.ts StampPreview: the same boxes and matrices, drawn as a layer over the pages); the page number
      styles are a row of buttons with the one in use marked
- [x] Text watermarks come out as faint as set (they were drawn solid: MuPDF's PDF writer drops a group's opacity,
      so it now goes with the text itself; stamp_test checks it)
- [x] Split starts on a few files: by bookmarks when the document has two or more, otherwise in two halves (it was a
      file for every page)
- [x] Every window starts on the ribbon's Home tab, and Home on Start, whichever were in use last

## 0.9.10 (Windows, Linux): PDF/A-1b, Office files to PDF/A, signing and saving PDF/A files
- [x] PDF/A-1b; Office files straight to PDF/A; signing keeps a PDF/A file PDF/A; saving changes to one warns and can
      keep it PDF/A; the check and Save as PDF/A held to veraPDF's test corpus (see the PDF/A section)
- [x] A crash in Check for PDF/A on PDF/A-1 font subsets (FreeType called without MuPDF's lock)

## 0.9.9 (Windows, Linux) and Android 0.6.5: PDF/A, and fixes
- [x] PDF/A-2b and -3b: check a document, save a copy as PDF/A (Convert pane), its claim in Properties (see the PDF/A
      section above). Checked with veraPDF
- [x] Attach file works on PDFs with no attachments yet; Sanitize works on files without a Names dictionary
- [x] A plain copy (Extract pages, Compare, PDF/A with unsaved changes) no longer marks the open document as saved
- [x] The toolbar's » button goes once the buttons fit again (it stayed with room to spare)
- [x] Passwords (both apps): a wrong password tried on an unlocked file leaves its pages drawing; errors that aren't a
      wrong password say what happened (desktop password dialog, Android); Android: after an engine restart, a
      password that no longer opens the file closes it with a message

## 0.9.8 (Windows, Linux): password-protected PDFs show their pages
- [x] A PDF opened with its password no longer shows blank pages. MuPDF's needs-password check tries the empty
      password, which replaces the keys the real one unlocked, and the engine asked it again right after unlocking.
      Documents now remember that a password opened them (mupdf.rs). Same fix as Android 0.6.4
- [x] Linux: storage.json is the user's alone (0600). The temp file is made new with that mode, and one written by
      0.9.7 (readable by other accounts) is fixed when a window opens. It lists the files you've opened

## 0.9.7 (Windows, Linux): everything the app remembers survives a reinstall
- [x] Not just settings: everything kept in the WebView's storage under "paginaut." (recent files and reading positions,
      the shelf, layout fixes for Read and Book modes, search options, the Home page shown, the scan offers turned
      down) is mirrored in storage.json in the app's data folder (prefs.rs). Every write goes through store.ts
      `storeItem`, which stamps the storage and writes the file half a second later (at once when the window closes).
      index.html puts the file back before anything reads the storage when the file is newer than the stamp
- [x] 0.9.6's settings.json is read as a copy of the settings until the first storage.json is written, then removed

## 0.9.6 (Windows, Linux): settings survive a reinstall
- [x] Settings are also kept in settings.json in the app's data folder (prefs.rs). After an uninstall and reinstall the
      WebView had started its storage over, so the theme and every other choice were back to the defaults. Each copy
      says when it was saved; a window starts from the newer and fixes the other. The first start copies the WebView's
      settings into the file. Each window gets the file in its page before it runs (no wait at start)

## 0.9.5 (Windows, Linux): fixes from the review of 0.9.3–0.9.4
- [x] An Office file opened from Explorer used Microsoft Office even with LibreOffice (or Built-in) chosen: its open
      raced the page telling the app the preference. Conversions with no converter named now wait for it (3 s at most;
      only the app turns the wait on, so the examples don't wait)
- [x] Moves on a viewer without a size (a background tab, Linux before the window shows) change the view it will show:
      a jump, zoom or fit made while hidden is no longer undone when it's shown. A hidden viewer reports its last view
      (positions saved for background tabs)
- [x] Launch files open once per window: a reload (development, or WebView2 reloading the page) doesn't reopen files
      that were closed or moved to another window
- [x] A size that changes every frame (sidebar or a pane sliding, a window dragged bigger) re-fits each frame but
      renders sharp pages once it settles (was one render per frame: 7–8 for an F4 toggle, now 1)
- [x] The viewer keeps unchanged pages, and renders already on their way, when the page list is replaced (real sizes
      arriving, page edits); a mixed-size document opened at a saved page rendered its pages twice
- [x] A pass asked for at once stays at once when a scroll or resize asks again in the same frame

## 0.9.4 (Windows, Linux): the faster start on Linux
- [x] The viewer waits for a size before showing the view it's given (fit, page, place on it). On Linux, WebKitGTK lays out
      nothing until the window is shown, and since 0.9.3 a launched document gets its viewer before that: it came out at the
      smallest zoom for a moment (the sidebar marked page 2) and a saved reading position was lost (opened at page 1).
      Tabs opened in the background (hidden, so no size either) keep their reading position the same way.
- Linux launch on Debian 13 (Xvfb, screenshots every ~50 ms): first page sharp ~1000 ms (0.9.2) → ~845 ms, with the final
  layout in the first frame. The 0.9.3 Linux packages have the bug above: don't hand them out.

## 0.9.3 (Windows, Linux): faster start with a file (opened from Explorer while Paginaut is closed)
Measured on the dev PC, five cold starts each: the first page sharp ~700 ms after launch → ~490 ms. WebView2 itself
takes ~250 ms of that and doesn't get faster with browser switches (tried: component updates, background networking off).
- [x] The main window is made in setup (`create: false` in tauri.conf.json, appwindows.rs `open_main`) with the launch
      files put in its page; main.tsx opens them before the first frame. Before, the page drew Home (9 book covers,
      a library scan) and asked for the files after 5 listen calls, all waiting on the start-up script
- [x] Launch files skip the "open in another window?" check (no other window yet)
- [x] The sidebar no longer slides open beside the first document: each frame re-fitted the page (7–8 renders of page 1)
- [x] The viewer keeps its pages when the real sizes arrive and equal the first page's (most documents): the first
      sharp render was cancelled and started again
- [x] Files from a second launch while the first window is still being made were dropped; they open with the launch files

## 0.9.1 (Windows, Linux): fixes from the 0.9.0 review
- [x] Linux DSC tokens: cryptoki 0.12 no longer finishes a token driver (C_Finalize) when it's let go, so a driver
      that stays loaded refused to start a second time: listing the certificates worked, signing after it failed
      ("The token driver didn't start"). pkcs11.rs finishes each driver after use again, one driver use at a time
- [x] The token tests run for real: `tools/linux.sh test` sets up a SoftHSM token (~/softhsm) and runs the Rust tests with
      it; before, they passed without running (and the SoftHSM test passed anyway through p11-kit's proxy, which
      hides the problem above). New test: a driver started three times in a row
- [x] Linux saved signatures: the desktop's keyring is connected again after a failure (not yet up after logging in,
      or restarted), not only after Paginaut restarts; a lost connection is remade at once, so signatures don't
      fall back to the key file. Tried with GNOME Keyring restarted under a running test
- [x] Android release builds stop when the upload key isn't set up (they came out unsigned without a word);
      `tools/android.sh all` builds the page once for the four ABIs, not four times
- [x] Inline-style px check remembered per property name; the password fields in Protect typed without a cast
- [x] Release builds no longer carry a development build's source map: build.mjs --prod clears old ones from dist/
      (the 0.9.0 installer had one packed in, unused), and `tools/linux.sh release` refuses a development page (the
      0.8.11/0.8.12 Linux packages shipped one: the release notes said `npm run build`, which is the development build)

## 0.9.0 (Windows, Linux) and Android 0.3.0: libraries brought up to date, signed Android release
- [x] The app's libraries on their newest versions that Tauri allows (not Android's test-only ones, unused): Tauri 2.12.1 and its plugins, sysinfo 0.39, similar 3
      (with its `std` feature: without it Compare's time limit is silently off), minisign-verify 0.3; Linux:
      keyring 3 → keyring-core + its D-Bus store (finds the saved-signature key keyring 3 stored), chacha20poly1305
      0.11, sha1/sha2 0.11, getrandom 0.4, cryptoki 0.12; Preact 11 (no more px for numbers in inline styles:
      src/preact-px.ts adds them, as Preact's compat does), TypeScript 7; Android: AGP 9.4.1, Kotlin 2.4.21,
      Gradle 9.8.1, AndroidX webkit 1.17.1, appcompat 1.8, activity 1.13, material 1.14, lifecycle 2.11. Held by
      Tauri: windows-rs 0.62 (webview2-com), gtk 0.18, foreign-types 0.3 (openssl)
- [x] cargo-audit: no vulnerabilities; three warnings inside Tauri's own Linux and build dependencies
- [x] Android release builds signed (upload key above); 0.3.0 tested on Android 16 and on Android 11's WebView 83
      (the oldest it supports); Android 7's stock WebView still gets the "update WebView" page
- [x] Checked by the user: a tab dragged onto another window's tab strip joins it

### 0.8.17: tabs slide like Chrome's
- [x] A tab dragged along the strip slides with the pointer and the other tabs move aside (animated) as it passes
      their middles; it settles into its place when let go. The label under the pointer only shows once the tab is
      pulled off the strip (before, the tab jumped between places under the label and looked stuck)

### 0.8.15: drag labels
- [x] The dragged tab's label is a small one-line pill whose icon says what letting go does: over another window's
      tabs it turns accent-coloured with a +, and that window's strip lights up with a bar where the tab lands; off
      the tabs it shows a new-window icon
- [x] 0.8.16: dragging the file itself out to other apps (0.8.15: Windows' SHDoDragDrop from the status bar and
      Home's recent files) didn't work in use and was taken out; tabs keep Chrome's tab drag only
- [x] A file asked for again while open (in any window) switches to it, as Office does; the window says so

### 0.8.14: review fixes for windows and tabs
- [x] Alt+` works where Windows' input methods take the key press for themselves (only its release reaches the
      page: the release then counts); the chosen tab is checked to still be open
- [x] A dragged tab shows under the pointer everywhere, outside the window too: a small label in the app's colours
      that says where it will go (along the tabs, into another window by name, or a new window); the tab itself
      lifts in the strip. Windows draws it natively (dragghost.rs); elsewhere the page draws it inside the window
- [x] A file already open in another window isn't opened twice (the two copies could overwrite each other's saves):
      that window comes forward at its tab; a tab isn't moved into a window that has the same file
- [x] A Book-mode tab moved to another window leaves full screen behind; files sent to a window still starting up
      wait for it; Stop for Convert and Compare by job, not by window; the last window closes without closing each
      document first; the switcher's preview uses the renders the pages already have

### 0.8.13: several windows, tab switcher
- [x] Several windows in one Paginaut (src-tauri/src/appwindows.rs, src/windows.ts): Ctrl+N or File › New window;
      starting Paginaut again without a file opens a window, a file opened from Explorer goes to the window used
      last. File › Close window (Alt+F4) and Exit Paginaut (all windows, each asking about its unsaved changes)
- [x] Tabs move between windows with their open document: unsaved changes, undo history, Read mode and the
      place in the document go along (the windows share the engine, nothing is saved or opened again). Drag a tab
      along the strip to reorder it, off the strip for a new window where it's dropped, or onto another window's
      strip to join it; right-click a tab for Move tab to new window / to window, Close other tabs. A window
      whose last tab leaves closes. Not while the tab is saving, recognizing text or in an editing panel
- [x] Alt+` tab switcher (Alt+~ back), most recently used first: a list that drops from the tab strip with the
      chosen tab's page beside it; let go of Alt to switch, Esc to stay; a quick Alt+` flips to the tab before
- Per window: Snap Layouts overlay, Stop for Convert and Compare, title (the document, for the taskbar), request
  ids (each window counts in its own range of the engine's cancel ids). Settings and recent files written by one
  window are taken up by the others (the side bar and ribbon tab stay each window's own). An update waits for
  unsaved changes in every window

### 0.8.6: natural voices and printing
- [x] Natural offline voices (Piper) for read aloud, downloaded in Settings › Read aloud (Phase 2's list)
- [x] Print (File › Print, Home, Ctrl+P; the command palette moved to Ctrl+K only): Windows' own print dialog
      (Windows 11's, shown from the window's thread: called from another thread it never appears), then each
      page drawn by MuPDF in 256-row bands at up to 300 dpi and handed to the printer. Pages print as shown: in the
      order and turned as arranged, with comments and form entries; turned to suit the paper (a page only turned
      sideways goes back upright), fitted to the printable area and centred. All pages, the current page or
      ranges; copies and collation by the printer driver. A bar shows the progress with Stop; cancelling a
      printer's own save prompt (Microsoft Print to PDF) is a quiet cancel
- [x] Page size: fill new margins with the page's own paper

### 0.8.5: editing and the window
- [x] Bold and italic within a paragraph; table cells edit one by one (Phase 6.3)
- [x] Windows 11 Snap Layouts on the maximize button (Deferred, at the top); Paginaut's own Windows manifest

### 0.8.4: tables and pictures
- [x] Table rows adjustable, group headings over their columns, the table menu in Read and Book modes (Phase 6.2's
      tables); a moved or replaced picture keeps its place in the drawing order (Phase 6.3)
- [x] Review fixes: a cell spanning rows no longer hid the cell under it in the preview and Copy (that row shifted
      left; Excel's merges were right); only a real drag of a table line counts, so a click leaves the found rows

### 0.8.3
- [x] Copy and search of stamped and edited Devanagari text (see Phase 6.2)
- [x] Fix: 0.8.2's review change ended COM's multithreaded apartment after each spoken paragraph; as the last
      thread in it, that tore it down under the speech engine's own threads and crashed (the unit tests caught
      it one run in three). The apartment is now kept for the life of the process (CoIncrementMTAUsage), and
      each call still starts and ends COM on its thread. The 0.8.2 build was never published
