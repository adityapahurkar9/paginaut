# Google Play store listing (Android viewer)

Text and choices for Play Console › Grow users › Store presence › Main store listing, and the App content pages.
Limits are Play's; the counts beside each part were checked when it was written. Change this file with the app, so
the listing never promises what the app doesn't do.

## App name (30 characters at most)

```
Paginaut: PDF & E-book Reader
```

29 characters. Play doesn't allow words like "best", "free" or "#1", emoji, or other apps' names in the title.

## Short description (80 characters at most)

```
A private reader for PDF, e-books and comics. No ads, no account, no internet.
```

78 characters.

## Full description (4,000 characters at most)

```
Paginaut is a quiet, fast reader for your documents, books and comics. No ads, no account, no tracking, and no internet access: what you read stays on your phone.

WHAT IT OPENS
• PDF, including password-protected files
• E-books: EPUB, MOBI and FB2
• Comics: CBZ, CBR and CBT
• XPS, Markdown and pictures

Open files from the app, with "Open with" from your file manager, or by sharing them from email, chat and download apps.

READ YOUR WAY
• Pages one after another, one at a time, or two side by side on a tablet
• Pinch and double-tap to zoom
• Read mode reflows a PDF's text to fit your screen, with the text size, serif or sans font, spacing and margins you like
• Book mode turns reflowed text into pages you swipe through
• Night mode and 16 colour themes, in light or dark
• Your place in every document is remembered

FIND YOUR WAY AROUND
• Search the whole document
• Contents, bookmarks and the document's comments
• Document details: pages, size, author, security
• Recent files, and a shelf of the e-books and comics on your phone (from the folders you choose)

LISTEN
• Read aloud with the voices already on your phone, with pause and speed

SHARE AND PRINT
• Share a document to any app
• Print through Android's printing, or save as PDF

PRIVATE BY DESIGN
• No internet permission: Paginaut can't send anything anywhere
• Never changes, moves or deletes your files
• Documents are read by an engine sealed off in its own process, with no access to your files or the network, so a damaged or harmful file can't reach anything
• Hide from screenshots and from the Recent apps preview, if you like
• If something goes wrong, Report a problem shows you exactly what a report would contain. You choose what to include and send it yourself; nothing is sent automatically

FREE AND OPEN SOURCE
Paginaut is free software under the GNU AGPL 3.0, built on the MuPDF engine by Artifex Software. The source code is on GitHub: github.com/adityapahurkar9/paginaut

Paginaut is a reader: it doesn't edit, sign or fill in documents on Android. The desktop app for Windows and Linux does.
```

About 2,050 characters.

## Graphics

| Asset | Play's requirement | File |
|---|---|---|
| App icon | 512 × 512 PNG, 32-bit, up to 1 MB | `assets/play/icon-512.png` (from assets/icon.svg) |
| Feature graphic | 1024 × 500 PNG or JPEG, no transparency | `assets/play/feature-graphic.png` |
| Phone screenshots | 2–8, 16:9 or 9:16, each side 320–3840 px | to take: see below |
| 7-inch and 10-inch tablet screenshots | up to 8 each; needed to be shown as made for tablets | to take: see below |

Screenshots: from the emulators, with documents that are free to show (testfiles/ or public-domain books, never
your own). Suggested set: the library with Recent and the shelf; a PDF page; Read mode with a theme; Book mode on a
tablet; a comic; search; Settings with the themes; Report a problem.

## Categorisation

- App or game: App
- Category: **Books & Reference** (Productivity also fits; Books & Reference matches e-books and comics, and
  readers there are compared with readers)
- Tags (up to 5): pick from Play's list in the console; the nearest are e-book reader, PDF, document viewer, comics

## Contact details

- Email: required, shown publicly. Use an address meant for app mail, not a personal one
- Website: https://github.com/adityapahurkar9/paginaut (optional)

## App content (Policy › App content)

- Privacy policy: https://github.com/adityapahurkar9/paginaut/blob/main/PRIVACY.md (works once the repository is
  public)
- Ads: No
- App access: All functionality is available without special access (no login)
- Content rating questionnaire: a utility/reader with no user-generated content shown to others, no sharing between
  users, no location, no purchases. Expected rating: Everyone / PEGI 3
- Target audience: 18 and over is simplest (13+ also possible); not made for children, so the Families policy
  doesn't apply
- Data safety: "No data collected, no data shared": see docs/PLAY-DATA-SAFETY.md
- All files access (MANAGE_EXTERNAL_STORAGE): the declaration in docs/PLAY-DATA-SAFETY.md
- Government app: No. Financial features: None. Health: No. News: No
