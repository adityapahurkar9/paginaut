# Paginaut for Android: Data safety

What to enter in Google Play Console's **Data safety** form for Paginaut (app.paginaut.reader), why each answer
is true, and what in the app backs it up. Also the **All files access** declaration and a privacy policy draft,
which Play asks for alongside it. Recheck this page whenever the app gains a permission, a library or a feature
that touches files or the network.

## The short version

Paginaut collects no data and shares no data. It has no internet permission in release builds, so nothing it
reads can leave the phone through it. It reads documents and never changes them.

## The form, question by question

Play's definitions: **collected** means sent off the device by the app; **shared** means handed to a third party.
Data the app only uses on the phone is neither.

| Question | Answer | Why |
| --- | --- | --- |
| Does your app collect or share any of the required user data types? | **No** | Release builds have no internet permission (see "Proof" below). Documents, reading positions, bookmarks, the shelf's list and settings stay on the phone. |
| Is all of the user data collected by your app encrypted in transit? | Not asked (nothing is collected) | |
| Do you provide a way for users to request that their data is deleted? | Not asked (nothing is collected) | On the phone: Settings › Storage removes copies; Settings › Clear the Recent list; uninstalling removes everything. |
| Has your app been independently validated against a global security standard? | **No** | Optional; can be revisited (e.g. an OWASP MASA review). |
| Account creation | **None** | No accounts, no sign-in. |
| Ads | **No ads** | |

Things that might look like collection or sharing, and why they aren't:

- **Share, Print, "Open with".** The user starts each one, choosing the app or printer that gets the file.
  Play counts user-initiated transfers the user expects as neither collection nor sharing.
- **Read aloud.** The text goes to Android's own text-to-speech service on the phone (the voice the user has
  installed). Paginaut sends nothing anywhere. Whether a voice engine works offline is up to that engine.
- **Android's backup.** Backup is on (the user's choice in Android settings). Android saves the app's settings,
  reading positions and the shelf's list to the user's own Google account; the developer never receives them.
  Copies of documents are in the cache, which Android doesn't back up. Play's help pages don't say plainly
  whether Android's own backup needs declaring. Our reading is that it doesn't, because the developer doesn't
  receive the data. **Confirm this with Play Console's help or policy support before submitting.**
- **Crash reports.** Paginaut has no crash reporting library. Android's own reports, which users opt into, go to
  Google under Google's terms, not the app's.

## All files access (MANAGE_EXTERNAL_STORAGE) declaration

Play asks for a separate declaration for this permission. It is optional in Paginaut, off until the user turns it
on, and the shelf works without it ("Search and pick books", "Choose a folder").

- **Core use:** "Document management / e-book library. The shelf lists every e-book and comic on the phone,
  including those other apps saved where Android's folder picker can't reach (Download itself, and
  Android/media, where WhatsApp keeps received documents)."
- **Why the alternatives aren't enough:** the folder picker refuses Download itself on Android 11 and later,
  and picking single files is capped at 512 kept files per app, with no new books showing up on their own.
- **Read-only:** Paginaut reads e-books and comics (EPUB, MOBI, FB2, CBZ, CBR, CBT) and never writes, moves or
  deletes the user's files.
- **User control:** off by default; turned on from the shelf with an explanation; turned off in
  Settings › Storage or Android's settings.

If Google refuses it, remove the permission from `src-tauri/gen/android/app/src/main/AndroidManifest.xml`, from
the allowed list in `app/build.gradle.kts` and from `MainActivity.extraPermissions()`; the shelf keeps working
through picked books and chosen folders.

## Proof: what keeps these answers true

Points that can be checked in the source:

1. **No internet in release builds.** Only `src/debug/AndroidManifest.xml` adds INTERNET (to load the page from
   the computer during development).
2. **The build stops on extra permissions.** `app/build.gradle.kts` checks the merged manifest (the app's plus
   every library's) on every build. The build fails if it finds a permission other than storage reading, a
   component open to other apps, or sharing beyond the copies of opened documents.
3. **The app stops on extra permissions.** `MainActivity` checks what the installed copy asks Android for. A
   modified copy shows "Paginaut stopped" with the permissions it found, on a plain screen in its own
   process (`RefusedActivity`), and opens nothing.
4. **Read-only.** Files are opened with read access only (`"r"`; folder and file grants are read grants). The
   engine writes only into the app's private cache (`mobile.rs`, `writable`): Print's PDF, repaired copies,
   covers.
5. **The engine opens only documents.** `mobile.rs` (`readable`) refuses any path outside the app's cache
   unless it's a document on shared storage, of a type Paginaut reads, not reached through a link, and not in
   another app's `Android/data` or `Android/obb`. Kotlin checks the same before handing over a path
   (`PaginautPlugin.sharedBook`).
6. **Other apps can't reach the app's files.** Copies and covers live in the app's private storage, which
   Android keeps apart from every other app. The only shared component is the FileProvider for Share,
   limited to the copies of opened documents and handing out one-time read grants.
7. **No scripts from documents.** JavaScript in PDFs is off on Android. Book text is shown as text, never run
   as a web page.
8. **The reading engine is sealed off.** MuPDF, the code that reads every document, runs in its own Android
   isolated process (`EngineService`, `engine_link.rs`): no permissions, no files, no network. The app opens
   each document read-only and hands the engine just that open file. A document built to break the engine
   reaches nothing. If the engine stops or stops answering, the app starts a new one and carries on; a
   document that stops it twice is closed with a message. Its temporary files are capped (512 per engine,
   none on a nearly full phone), and an engine that fills the phone is stopped, which frees them.
   `tools/android-engine-test.sh` checks this.
9. **Hide from screenshots** (the reader's View sheet): Android's secure window, so screenshots, recordings
   and shared screens show black and Recent apps no preview.

## Privacy policy (draft)

Play needs a privacy policy link even for apps that collect nothing. It has to be a public web page: host this
once the releases page or repository is public.

> **Paginaut privacy policy**
>
> Paginaut is a reader for PDF, e-books, comics, XPS and Markdown. It doesn't collect, store on servers, or share
> any information about you. It has no internet access.
>
> **What stays on your phone:** the documents you open, Paginaut's copies of documents other apps send it, your
> reading positions, bookmarks, the list of books on your shelf, and your settings. If Android's backup is on,
> Android may save Paginaut's settings and reading positions to your Google account; the developer never
> receives them.
>
> **Files:** Paginaut only reads documents you open, pick, or keep in folders you choose for its shelf. If you
> turn on "All files access", it looks for e-books and comics anywhere on the phone. It never changes, moves or
> deletes your files.
>
> **Read aloud** uses your phone's own text-to-speech voice. **Share** and **Print** send a document only where
> you choose.
>
> **Questions:** [contact email].
>
> Last updated: [date].

## Before submitting

- [ ] Confirm the backup point above with Play's help or policy support.
- [ ] Host the privacy policy and put its link in the store listing.
- [ ] Fill the All files access declaration (or remove the permission).
- [ ] Recheck after any new permission, library or network feature: the build check will stop a new
      permission, and this page should change with it.
