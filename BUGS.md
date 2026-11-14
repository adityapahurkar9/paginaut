# Paginaut bugs

Open bugs, newest first. Features and plans stay in ROADMAP.md; a fixed bug is ticked here and its fix is noted under
the release that carries it in ROADMAP.md.

## Open

### Code review, whole codebase (found 2026-11-13; read from the code, none checked by running yet)

After five passes everything has been read, native/paginaut.c included (all 14,000 lines, in the fifth pass). Only
glanced at: HelpDialogs.tsx, Logo.tsx, ToolFrame.tsx, SortToggle.tsx. In paginaut.c the PDF/A checker (the last 4,500
lines) was read for memory safety and plain logic, not against the PDF/A standard itself.

Most serious first. Read in full for this: lib.rs, app.rs, desk.rs, engine/mod.rs, engine/pool.rs, net.rs, update.rs,
piper.rs, ocr.rs, fonts.rs, addons.rs, shelf.rs, prefs.rs, sigstore.rs, appwindows.rs, office/libre.rs, mobile.rs,
store.ts, pages.ts, speech.ts, search.ts, RenderManager.ts, update.ts; parts of paginaut.c, dsc.rs and
PaginautPlugin.kt.

- [x] Security: opening an attachment can run a program. attachment_open compares the name's extension with the RISKY
      list as it is, and free_path cleans the name but not the extension: an attachment called "invoice.exe " (a
      space at the end) has the extension "exe ", passes, is written as a file Windows stores as invoice.exe, and is
      opened. "x.exe::$DATA" passes the same way. Old Office files that can carry macros (doc, xls, ppt, dot, xlt)
      aren't on the list although docm and xlsm are. A list of what may be opened (documents, pictures, text, as
      open_path has) on a tidied name would close the whole kind. (src-tauri/src/desk.rs, attachment_open and RISKY;
      src-tauri/src/engine/mod.rs, free_path)
      Fixed: a list of what may be opened (documents, pictures, text, sound and video: desk.rs OPENABLE) on the name
      tidied as Windows stores it; everything else is only saved. Old Office formats (doc, xls, ppt) are among what
      isn't opened. Unit test desk::tests
- [x] Signing into a new file that already exists, then failing (PIN cancelled, timestamp service unreachable),
      deletes that file. sign copies the document straight over the target before signing and removes the target on
      failure; nothing is written beside it first. (src-tauri/src/engine/mod.rs, sign)
      Fixed: signing always writes beside the target (<name>.signing.tmp) and takes its place only when it worked.
      dsc_test checks a file at the new name survives a cancelled signing
- [x] Installing an update can throw away changes made while it downloads. Unsaved changes are looked for before the
      download (update.ts installUpdate, appwindows::others_dirty), but the app quits (app.exit) after the download
      and the installer's start, minutes later, without looking again or asking. (src-tauri/src/desk.rs,
      update_install; src/update.ts)
      Fixed: update_install only downloads and checks; the page then looks at its tabs again, and update_run looks at
      the other windows again, before the installer starts. With changes found, the downloaded installer is kept for
      the next try
- [x] Security, to confirm: a signature is checked over whatever its /ByteRange names. Nothing checks that the range
      starts at 0 and that the gap in it is exactly the signature's /Contents, so a made-up PDF could show a valid
      signature over a document whose visible content isn't in the signed bytes. fo_sigs_json's comment still lists
      a "whole" field it doesn't write. Not checked: whether MuPDF's pdf_validate_signature already refuses this.
      (src-tauri/native/paginaut.c, fo_sigs_json and fo_sig_data; src-tauri/src/desk.rs, signatures)
      Fixed: confirmed (MuPDF hashes whatever the range names). fo_sig_data now refuses a signature unless its range
      is [0 a b c] within the file and the bytes left out are exactly the hex string holding that signature
      (check_byte_range); such a signature shows as one that can't be relied on. Read from the code and tried on
      Paginaut's own signatures (dsc_test: one, two, in place); no forged file was made to try it
- [x] Two saves can run at once. save looks at tab.saving before its waits (the PDF/A check and question) and sets
      it only after them: with rotations saved automatically, Ctrl+S right after a rotation and the 1.2 s timer can
      both get through, and the second applies the same rotations and page order to the file already saved.
      (src/pages.ts, save)
      Fixed: a tab is marked as saving from the first line of save (a set of tabs entered), not after its questions
- [x] Downloading two Piper voices at once on a fresh install: both download Piper itself into the same
      piper.zip.part and unpack into the same piper.part, so one or both fail ("didn't match the expected one").
      Only a second download of the same voice is refused. (src-tauri/src/piper.rs, get_runtime; desk.rs,
      piper_download)
      Fixed: one download of Piper itself at a time; the second waits (Stop still works) and then finds it there
- [x] Linux: PDF/A saving stops with a fault on a font whose name isn't plain ASCII. find_fc counts letters and uses
      the count as a byte position (raw[..i]). (src-tauri/src/fonts.rs, find_fc)
      Fixed: find_fc goes by letters. Not run on Linux yet
- [x] Signing in place fails ("Couldn't replace…") while a search or Read mode is still reading the file, after the
      PIN was entered: the signed copy is put in place with one rename, where save_with tries for 3 seconds
      (copy_with_retry) for this very reason. (src-tauri/src/engine/mod.rs, sign)
      Fixed: the signed copy takes the file's place with a rename, then copy_with_retry (3 s) as saving does
- [x] Saving a picture writes a different file than the one chosen: the extension is swapped for the picture's own
      kind after the dialog, so "scan.png" becomes "scan.jpg" and overwrites one that's there without asking;
      "report.v2" becomes "report.jpg". (src-tauri/src/desk.rs, picture_save)
      Fixed: the chosen file is the one written; only a name with no extension gets the picture's own, and then never
      over a file that's there
- [x] Closing Find within 160 ms of typing still runs the search: highlights appear with the bar closed.
      clearSearch doesn't stop runSearch's timer; the timer is also one for all tabs, so typing in another tab's Find
      drops the first tab's search. (src/search.ts)
      Fixed: clearSearch stops the waiting search; each tab has its own timer
- [ ] Security, low: an update's version isn't signed. latest.json is read as it comes and only the installer is
      checked against the key, so an old signed installer can be offered as a newer version (and apt is run with
      --allow-downgrades). Needs someone who can change the release's files but not the key. (src-tauri/src/update.rs)
- [x] Clicking a voice's sample twice while it loads plays it twice over itself; the first can't be stopped.
      (src/speech.ts, playSample; see also the open sample bug below)
      Fixed: each sample asked for is counted and only the latest plays; a second click on a loading sample stops it
      (speech.ts stopSample)
- [x] A fault while drawing a page leaves its request unanswered: the worker catches it, but the answer is never
      sent, so the page stays blank with no message and the request counts as running. Only search and the like are
      guarded per page. (src-tauri/src/engine/pool.rs, worker; engine/mod.rs, render)
      Fixed: render runs guarded, so the fault is the page's error and the request ends
- [x] Android: a file sent again from the same place opens the kept copy when its name and size match, even if its
      content changed. (PaginautPlugin.kt, open)
      Fixed: the kept copy is used only when its fingerprint (size, first and last 64 KB) matches the source's; a
      source that can't be compared is copied again. Not built or run on Android yet
- [x] "View signed version" after a save that kept its copy beside the file (couldn't replace it) cuts the wrong
      file: the offsets are the copy's, the bytes are read from the original path. (src-tauri/src/desk.rs,
      signed_version: Doc::path where Doc::file is meant)
      Fixed: reads Doc::file, and only the bytes the signature covers
- [x] Any failure of a search with Regular expression or Whole word on says "That pattern isn't valid", also when
      the file couldn't be read. (src/search.ts)
      Fixed: "That pattern isn't valid" only for MuPDF's "regcomp failure" on a pattern the user typed; a pattern that
      runs too long says so; anything else is said as it is
- [x] Saving rotations automatically has one timer for all tabs: a rotation in a second tab within 1.2 s cancels the
      first tab's save, and a tab closed within 1.2 s gets "Couldn't save: document … is not open". (src/pages.ts,
      maybeAutoSave)
      Fixed: a timer per tab, and a tab closed meanwhile isn't saved
- [x] Setting up LibreOffice, or downloading an OCR language, can be started twice (two windows, a double click):
      both write the same .part file, and for LibreOffice the second start clears a Stop just pressed. Only Piper
      voices refuse a second start. (src-tauri/src/desk.rs, office_install and ocr_download)
      Fixed: both refuse a second start while one runs
- [x] Two windows saving a signature at the same moment: one is lost (the list is read, changed and written with no
      lock; prefs.rs has one for its file). (src-tauri/src/desk.rs, sig_add and sig_delete)
      Fixed: sig_add and sig_delete hold one lock from reading to writing
- [x] Windows: one very long entry among the installed fonts ends the listing there (any error from RegEnumValueW
      stops it, also "more data"): fonts after it are missing from themes and count as not installed for PDF/A.
      (src-tauri/src/fonts.rs, reg_values and reg_pairs)
      Fixed: an entry too long for the buffers (now 16,384 and 4,096 letters) is skipped and the listing goes on;
      reg_values is gone (reg_pairs' names)
- [x] Marking for redaction that fails partway leaves the marks made so far showing on the pages (the step is taken
      back in the document, but the pages kept in memory aren't dropped, as annot_edit does).
      (src-tauri/src/engine/mod.rs, redact_mark)
      Fixed: the pages kept in memory are dropped on failure (also when removing marks fails)
- [x] Read aloud: one failed reading of a page's words is kept, so the word being read is never marked on that page
      again until the text is made anew. (src/speech.ts, pageLayout)
      Fixed: a failed reading is forgotten, so the next word asks again
- [x] A page's text read at the very moment of an edit can be kept from before the edit (text() has no check like
      list()'s edit_rev). (src-tauri/src/engine/mod.rs, Doc::text)
      Fixed: text() checks edit_rev before keeping, as list() does

Second pass, the same day. Read for it: compare.rs, convert.rs, office/mod.rs, office/com.rs, engine/mupdf.rs,
engine/ocr.rs, engine/sandbox.rs, print.rs, windows.ts, main.tsx, edits.ts, annots.ts, forms.ts, ocr.ts, redact.ts,
dsc.ts, mobile/open.ts, the first third of Viewer.ts, and the capability files. Most serious first.

- [x] Web links on PDF pages don't open, on Windows, Linux and Android. Both capability files give
      "opener:allow-open-url" and nothing that says which addresses (not "opener:default" or
      "opener:allow-default-urls"); tauri-plugin-opener 2.7.0 refuses every address when none is allowed, and the
      click swallows the error. Read from the plugin's code, not tried. When allowed, only http, https and mailto
      should be: the address comes from the PDF (file:, ms-…: and \\server links would open too).
      (src-tauri/capabilities/default.json and mobile.json; src/render/Viewer.ts, loadLinks)
      Fixed: both capability files allow https, http and mailto addresses, and the page checks the same before asking
      (Viewer.ts openableLink); another kind of link, or one that can't be opened, is said in a toast. Not tried in
      the running app yet
- [x] Privacy: what Sanitize takes out, and pages and comments deleted, are still in the saved file. An ordinary
      Save of a PDF adds the changes to the end of the file (that's what makes it quick) unless redactions were
      applied (MuPDF then insists on a full write). So after Sanitize (metadata, scripts, attached files, comments)
      or deleting a page, Save leaves the old content in the file's earlier bytes, where it can be read back. A
      save after any of these should write the file afresh. (src-tauri/src/engine/mod.rs, save_with: the
      incremental branch)
      Fixed: a save after a page was deleted, or after Sanitize, a deleted comment or attached file, an edit of text
      or pictures, removed recognised text or changed properties (Doc::took_out), writes the file afresh. Except a
      signed file: its signatures cover the earlier bytes, so it's still only added to. save_test: a deleted page
      makes the file smaller and its text is on no page
- [x] A signature that a later change made invalid still shows as valid. fo_sigs_json gives "invalidAt" (MuPDF's
      pdf_validate_signature: a change the signature doesn't allow was saved after it; -2 damaged), and nothing in
      the app reads it: verdictOf goes by the signed bytes and the certificate alone. (src/dsc.ts, verdictOf and
      describe; src/ui/SigPanel.tsx)
      Fixed: the engine also gives what a signature allows after it ("mdp": its certification level, "lock": locked
      fields). One that certifies with no changes allowed and has changes after it, or that limits changes and has
      more than form filling and signing after it (MuPDF's invalidAt), is not valid; with comments allowed, or a
      history that can't be read, it's a warning (dsc.ts laterChange). An ordinary signature isn't judged by
      invalidAt: MuPDF counts any added comment or signature field as a change, which such a signature allows. Not
      tried on a certified file (none among the test files)
- [x] Ctrl+S leaves out the form field being typed in: a field's text goes to the document when the field is left
      (or Enter), and Ctrl+S saves without that. The text still shows, so the form looks saved. The same for a text
      box (comment) still open. (src/main.tsx, the Ctrl+S key; src/forms.ts, commit; src/annots.ts, editText)
      Fixed: Save first hands over the field or text box in use and waits for edits under way (store.ts settleEdits);
      Ctrl+S in a text box reaches Save
- [x] Recognized text that fails partway (and Remove recognized text) still leaves an undo step in the document,
      while the app is told it failed and counts none: Undo is out of step from then on, and the tab isn't marked
      changed. apply_ocr and remove_ocr call end_op where annot_edit now calls abandon_op.
      (src-tauri/src/engine/ocr.rs)
      Fixed: abandon_op on failure, and the pages kept in memory are dropped
- [x] The page turns are handed to the C side without their count: fo_write_document reads rot[order[k]] whatever
      the list's length. A convert to text, or Save as PDF of a file that isn't a PDF, with a list shorter than the
      highest page reads past its end (wrong turns, or a crash). apply_page_edits passes and checks the count.
      (src-tauri/src/engine/mupdf.rs, write_document and convert_to_pdf; native/paginaut.c, fo_write_document)
      Fixed: the turns are filled out with 0 up to the highest page named before they're handed over (mupdf.rs
      order_and_turns)
- [x] Document properties: a title, subject or keywords over 1 KB is read cut short (a 1024-byte buffer), and
      Apply writes all four back, so the rest is lost. (src-tauri/src/engine/mupdf.rs, metadata; engine/mod.rs,
      set_properties)
      Fixed: a longer value is read again whole
- [x] Windows: when Word, Excel or PowerPoint stops answering during a conversion, Paginaut gives up after 3
      minutes and leaves it running, hidden, with the file open ("check for its window", but it has none to see).
      The file stays locked until the program is ended in Task Manager. (src-tauri/src/office/com.rs, run)
      Fixed: the copy of the program started for the conversion (the one new process of it) is ended when it stops
      answering, and the message says so; a copy the user had open is left. Not tried with a program that hangs
- [x] Moving a tab to a new window lets go of it before the new window has it. If that window's page never starts
      (or it's closed at once), the document stays open with its unsaved changes and no tab anywhere.
      (src/windows.ts, moveToNewWindow)
      Fixed: a window that goes before its page took the tab hands it back to the window it came from (or any left)
- [x] Signing into a new file accepts the name of a PDF open in another tab (Save refuses that).
      (src/dsc.ts, finish)
      Fixed: refused as Save refuses it
- [x] Android: a big file read in place stays held when its password is cancelled or it can't be unlocked
      (closeCurrent lets go of such files; this path doesn't). (src/mobile/open.ts, openPicked)
      Fixed: let go on every way out of openPicked that doesn't show the file. Not built or run on Android yet
- [x] Convert to pictures numbers the files by the page's place in the file, not as shown: after moving or deleting
      pages, "report-9.png, report-1.png…". Excel's sheets ("Page 9") the same. (src-tauri/src/convert.rs)
      Fixed: files and Excel's sheets are numbered as the pages are shown (the request carries each page's place)
- [x] A conversion stopped or failed partway leaves what it wrote: half a TIFF at the chosen name (over a file
      that was there), the first pictures of many. (src-tauri/src/convert.rs)
      Fixed: a TIFF is written beside its name and put there when whole; pictures written before a stop or failure are
      deleted
- [x] The pages box takes "1-5" but not "1 - 5" or "3 to 8", which its pattern was written for (the text is split
      at spaces first). (src/ocr.ts, parsePages)
      Fixed: spaces around a range's mark are taken out before the text is split
- [x] A drop-down you can type in ignores "read only", and two pages in view with a field of the same number share
      one list of choices (the list's id repeats). (src/forms.ts, create)
      Fixed: read only is set, and each list has an id of its own

Third pass, the same day. Read for it: engine_link.rs, pkcs11.rs, defaults.rs, officetext.rs, xml.rs, flow.rs,
docx.rs, markdown.rs, tiff.rs, tts.rs, snap.rs, print_linux.rs, engine/table.rs, most of engine/pdfa.rs, the first
half of engine/pdfhtml.rs, the rest of dsc.rs's Linux side and the start of its Windows side, native/cbr.c,
native/ocr.cpp, all of PaginautPlugin.kt and MainActivity.kt, the rest of Viewer.ts, markdown.ts, print.ts,
actions.ts, textedit.ts, office.ts, compare.ts, sign.ts, extract.ts, pagesize.ts, stamps.ts, shelf.ts,
reflow/model.ts, find.ts and overrides.ts, mobile/records.ts, main.tsx, speech.ts, search.ts, shelf.ts and flow.ts,
ui/SecureUI.tsx and most of ui/PagesUI.tsx. Most serious first.

- [ ] Moving a Markdown tab to another window loses what was typed in its source and not yet saved. The source is
      kept by the tab's key in the window it was opened in; the move doesn't take it along, and the old window
      forgets it when the tab leaves. The new window reads the file from disk again, still shows the tab as changed,
      and Save then offers Save as PDF. (src/markdown.ts, sources; src/windows.ts, pack and busy)
- [ ] A Markdown file that isn't UTF-8 (Windows-1252, UTF-16) is read with its other letters turned into "�", and
      saving writes that back over the file. A UTF-8 file's byte order mark is dropped on save. Such a file should
      be read in its own encoding, or opened read-only. (src-tauri/src/desk.rs, markdown_read and markdown_save)
- [x] Android: opening a file that's read where it is while it's already held drops the new one's link. Such a file
      always gets the same link (direct/<its address, hashed>/<name>): the second open deletes and remakes the
      folder and replaces the held handle without closing it, and when the first one is let go that closes the
      second's handle and deletes its link. Easiest to meet: tapping a book on the shelf while the shelf is still
      reading that book's cover (just after new books were found). The book fails to open, or opens and then Print,
      Share and coming back after the engine restarts fail. Also "Open with" from Files for the file that's open.
      (PaginautPlugin.kt, open, openDirect and release; src/mobile/shelf.ts, fillCovers)
      Fixed: every open of a file read in place gets a folder of its own (PaginautPlugin.kt directDir), so two opens
      of one file don't share a link or a handle. Not built or run on Android yet
- [ ] Android: a bad book on the shelf can get the document you're reading refused. When the engine stops, each
      document with something under way takes a strike; a cover being drawn for the shelf has no strike (and no
      refusal) of its own. So a damaged book whose cover stops the engine twice in two minutes gets the open,
      healthy document closed as "stopped the engine twice", and the bad book is tried again on every scan.
      (src-tauri/src/engine_link.rs, strike, engine_stopped and cover)
- [ ] A signature made with an expired certificate shows as valid on the signer's own word about when it signed.
      The certificate is judged at the time the signature claims (on Linux also at the time of a timestamp from a
      service that isn't trusted), and "expired" is shown green: "valid when it signed, expired since". Without a
      trusted timestamp nothing proves when it was signed; that case should read as a warning.
      (src-tauri/src/dsc.rs, verify on both systems; src/dsc.ts, verdictOf)
- [ ] A table saved as Excel gets wrong numbers when the figures use a decimal comma: every comma is taken out
      before the number is read, so "12,5" becomes 125 and "1.234,56" becomes 1.23456 (percentages the same). A
      comma should count as a thousands mark only when groups of two or three digits follow it; otherwise the cell
      should stay text. (src-tauri/src/engine/table.rs, number and percent)
- [ ] Linux: converting Office files to PDF into a chosen folder writes them next to it under a name with a
      backslash in it ("out\report.pdf"): the path is put together with "\\". Names already taken are also compared
      in lower case, where Linux keeps Report and report apart. (src/office.ts, runConvert)
- [ ] Compare: changing an option or swapping sides while a comparison runs starts another without stopping the
      first. The first keeps going in the engine and Stop no longer reaches it (its number was replaced).
      (src/compare.ts, compareAgain and swapSides)
- [ ] Android: the cache tidy can delete the copy of the document just opened. A tidy already under way is reused
      by a later call, and doesn't know the copy that call asked to keep. (src/mobile/records.ts, tidyCache)
- [ ] A short line at the top or bottom of a page made only of the letters i, v, x, l, c, d, m is taken for a
      Roman page number and left out: "CV" at the head of a résumé, "Civil", "Mid", "DVD". Read and Book modes and
      read aloud drop it, and so do Save as Word and Markdown. A real Roman numeral should be asked for (and a
      small one). (src/reflow/model.ts, PAGE_NUMBER; src-tauri/src/flow.rs, page_number)
- [ ] Linux: a timestamp is put into the signature without checking it is the one asked for (the same random
      number, the same hash of the signature). A wrong or replayed answer is embedded and signing says it worked.
      (src-tauri/src/dsc.rs, the Linux add_timestamp)
- [ ] Save as Markdown writes "\2024. A year" for a paragraph that starts with a number and a dot, and the
      backslash shows: Markdown takes a backslash only before punctuation. The dot is what needs it ("2024\.").
      The test expects the wrong form. (src-tauri/src/markdown.rs, escape_start)
- [ ] Compare reads an Excel file's text one place off after an empty entry written as <si/> in its list of
      shared text (Excel itself writes <si><t/></si>; other tools may not). (src-tauri/src/officetext.rs,
      shared_strings)
- [ ] Edit text: with a picture selected, the one-key shortcuts still run (the selection box has the focus, not a
      text field), so R turns the whole page. (src/main.tsx, the keys; src/textedit.ts)
- [ ] Protect: when checking the permissions password fails (not a wrong password: the call itself), nothing is
      said. (src/ui/SecureUI.tsx, unlock)

Fourth pass, the same day. Read for it: the rest of dsc.rs (Windows signing and checking), engine/pdfhtml.rs,
engine/pdfa.rs, officetext.rs and compare.rs; dragghost.rs, base64.rs; api.ts, skin.ts, themes.ts, modes.ts,
panes.ts, toolpanes.ts; reflow's BookView, ReadView, LayoutEditor and render; the mobile screens (Reader, Sheets,
Library, ShelfView, Storage, select, state, native, nav); every file in src/ui; EngineService.kt and
RefusedActivity.kt; and parts of paginaut.c (see the note at the top).

- [ ] Book mode: text written without spaces (Chinese, Japanese, Thai) can't be split across pages, since a
      paragraph is only cut at a space. A paragraph taller than a page is put whole on one page and its end is cut
      off, never shown. (src/reflow/BookView.ts, fit and paginate)
- [ ] Attaching a file whose name is very long (511 bytes or more, about 170 Chinese or Hindi letters) when the PDF
      already has an attachment of that name hangs the app: the numbered name ("name (2)") is cut back to the same
      name, so the search for a free one never ends. (src-tauri/native/paginaut.c, fo_pdf_attachment_add)
- [ ] Split by bookmarks: two bookmarks whose names differ only in capitals ("Summary", "SUMMARY") get the same
      file on Windows, so one part replaces the other and the message still counts both. (src/ui/PagesUI.tsx,
      SplitDialogInner, plan)
- [ ] Read and Book modes keep their place as a count of paragraphs. After pages are moved or deleted (from the
      sidebar, while reading) the text is built again and the old count points somewhere else, so the view jumps.
      (src/ui/Reflow.tsx, ReflowHost; tab.reflowBlock)
- [ ] An e-book made from a PDF with more than 512 fonts: text in the fonts after the 512th isn't drawn, on screen
      or in Save as PDF, and nothing says so. (src-tauri/native/paginaut.c, fo_pinned_pdf, MAX_FONTS)
- [ ] Save as repaired EPUB leaves out a first or last line of a page made only of the letters i, v, x, l, c
      ("Civil", a lone "I"): it's taken for a Roman page number. (src-tauri/src/engine/pdfhtml.rs, is_folio)
- [ ] The table tool asks the engine again after each change to its lines and shows whichever answer comes last,
      so after two quick changes the cells can be those of the earlier lines. (src/ui/TableUI.tsx, the effect on
      cols and rows)
- [ ] Android, Document info: Created and Modified ignore the date's time zone, so they're hours off (and near
      midnight a day off). The desktop reads the zone. (src/mobile/Sheets.tsx, pdfDate)
- [ ] Page numbers: "Start numbering at" 0 and Bates "From" 0 turn back into 1, though both boxes allow 0.
      (src/ui/StampUI.tsx, HeaderDialogInner)
- [ ] Sign pane: "Goes in the form's field … on page N" gives the page's number in the file, not where it's shown
      after pages were moved or deleted. (src/ui/CertSection.tsx)
- [ ] Attached files: a file pinned to a page that has been deleted is listed as on "page 0".
      (src/ui/ExtractUI.tsx, Attachments)
- [ ] Compare: with the list narrowed to one kind (Added, say), F7 and the arrows still stop at the differences
      that are hidden. (src/ui/CompareUI.tsx, step)
- [x] Convert: after the last page the progress reads one page too many ("Page 11 of 10…").
      (src/ui/ConvertPane.tsx)
      Fixed: never more than the total
- [ ] Book mode in a narrow window shows one page but its bar still says two ("5–6 of 200").
      (src/ui/Reflow.tsx, BookBar)
- [ ] Android, Settings and Storage: with no copies kept it says "0.1 MB of copies". (src/mobile/Storage.tsx,
      sizeText)

Fifth pass, the same day. Read for it: the rest of native/paginaut.c, start to end.

- [ ] Save as PDF/A can crash on a font whose LastChar is above 255 (or FirstChar below 0): its widths are written
      again over that range from a list of 256, read past its end. Needs an embedded simple font with one drawn
      letter whose width is wrong. (native/paginaut.c, pa_put_widths and the Type 3 loop in pa_font_use)
- [ ] After a redaction, a text edit, a picture edit or removing hidden text on a page, that page's recognised
      text (OCR layer) can no longer be removed and the page isn't counted as having one: the page's content is
      written again as one stream, without the mark the layer is found by. (native/paginaut.c, stamps_aside,
      is_ocr_stream, fo_ocr_apply)
- [ ] Inserting pages from a PDF can copy the whole of that PDF in: a form button (any comment but a link) that
      goes to a page brings that page along, and with it every page of the file. The saved file is far bigger and
      still holds the pages left out. (native/paginaut.c, insert_pdf_pages, is_internal_link)
- [ ] Sanitize, scripts: scripts on a form field that has several boxes (they sit on the field, not on its boxes)
      and on bookmarks are left in. (native/paginaut.c, fo_pdf_sanitize, drop_scripts_in)
- [ ] Page labels that start again with the same style (two reports in one file, each numbered from 1) run on
      after any page is moved or deleted: the second report becomes 50, 51, 52. (native/paginaut.c, write_labels)

Cleanup:

- [ ] Removing every watermark leaves the empty "Watermark" layer in the file, which other readers still list.
      (native/paginaut.c, fo_stamp_remove, watermark_layer)
- [ ] A photo loaded while making a signature is kept in memory until the window closes (its object URL is never
      revoked). (src/ui/SignMaker.tsx, Photo, load)
- [ ] Save as Word keeps every page's pictures in memory, then the zip writer copies them all again; its sizes and
      places are 32-bit and its file count 16-bit, unchecked, so past 4 GB (a long colour scan) it writes a file
      Word can't open. Save as Markdown also holds every picture before writing any.
      (src-tauri/src/engine/table.rs, zip; src-tauri/src/docx.rs and flow.rs, document)
- [ ] Compare counts each word's place from the start of its paragraph or cell again: slow for very long ones.
      (src-tauri/src/compare.rs, tokens)
- [x] com.rs reads "Visible" the same way in three places (convert_on_this_thread).
      Fixed: read once
- [ ] The signature panel copies everything a signature covers twice into memory, per signature (a 900 MB scan:
      about 1.8 GB each); signed_version reads the whole file to write its start. (src-tauri/native/paginaut.c,
      fo_sig_data; src-tauri/src/desk.rs)
- [x] "A progress event per 1%" is written out four times, two with `done - last` (which underflows if progress
      ever starts over) and two with saturating_sub. (src-tauri/src/desk.rs)
      Fixed: one progress_step, which also counts progress that starts over
- [ ] "Open, and give the kept password if it's asked for" is written out seven times. (src-tauri/src/engine/mod.rs)
- [ ] git_blob_sha1 is in ocr.rs and in piper.rs.
- [ ] verify and verify_file read the key and signature the same way twice (update.rs); reg_values is reg_pairs
      without the data (fonts.rs).

### Read aloud voices (reported 2026-11-10, Windows and Linux)

- [x] Listening to a voice sample in Settings › Read aloud and then closing Settings leaves the sample playing to the
      end. Nothing stops it when the Read aloud tab or the dialog goes away: playSample only stops a sample when it's
      clicked again, another sample starts or it finishes. Closing Settings (and leaving the Read aloud tab) should
      stop it. (src/speech.ts, playSample; src/ui/SettingsDialog.tsx, ReadAloud)
      Fixed: the Read aloud tab stops the sample when it goes away (leaving the tab or closing Settings)
- [ ] Very few Indian English voices. The downloadable list has none: the Piper catalogue has English only for
      Great Britain (9) and the United States (22), no en_IN, so it's American and British accents only. Windows'
      own voices on the user's PC are David, Zira and Mark (all US); Windows has Indian English voices (Heera, Ravi)
      only with the English (India) speech pack installed. To look at: other en_IN Piper voices or a newer catalogue,
      telling people how to add Windows' English (India) voices, and showing Indian English as its own entry in the
      language list. (src-tauri/src/piper_voices.rs from tools/piper-catalog.py; src/ui/SettingsDialog.tsx)

## Fixed

### Found while adding the watermark preview (2026-11-09, Windows and Linux; fixed the same day, next release 0.9.11)

- [x] A text watermark is drawn solid whatever its opacity (20% came out as full red). fo_stamp_text wrapped the
      text in a group with the opacity, and MuPDF's PDF writer leaves a group's opacity out. Pictures were right
      (their opacity goes with the picture). (src-tauri/native/paginaut.c, fo_stamp_text)
      Fixed: the opacity goes with each piece of text drawn (st_device). stamp_test checks a red watermark at 20%
      comes out pale

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

### Android (0.6.5; fixed 2026-11-09, in Android 0.6.6; checked on the emulator, not yet on a phone)

- [x] Opening a PDF from another app (Gmail, WhatsApp: Open with › Paginaut) while Paginaut isn't running: the app
      starts and says "This file isn't there any more (moved or deleted)". Opening the same file again from the other
      app, with Paginaut now running, works. Reported 2026-10-31.
      The message is the FileNotFoundException branch of copyThenResolve in
      src-tauri/gen/android/app/src/main/java/app/paginaut/reader/PaginautPlugin.kt; on a cold start the shared
      file's URI is read before it can be opened (or after the grant that came with it no longer applies)
      Fixed (in code): the sent file is read as soon as Android hands it over (PaginautPlugin.load / onNewIntent start
      the copy at once), not seconds later when the page asks; and a "not found" from the sending app's provider is
      asked again once, 0.7 s later, before it's believed. On the emulator (0.6.6, debug and release) a file sent
      while Paginaut was closed opened every time. To check on a phone: Gmail and WhatsApp, Paginaut closed

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
