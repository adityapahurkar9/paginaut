# Dependencies to keep up to date

What Paginaut is built from and ships with, where each comes from, and how to update it. Check
these every few months, and straight away when one of them announces a security fix (most of them
read files that can come from anyone: a bug in a parser is a way in).

Versions as of 2026-10-05. Licences matter: Paginaut is AGPL-3.0, so everything here must be
compatible with it (no RarLab unRAR code, for one: its licence forbids rebuilding a RAR compressor).

## Read files from anyone (update first)

| What | Version | Where it lives | How to update |
|---|---|---|---|
| **MuPDF** (PDF, XPS, EPUB, CBZ, images; and the libraries it bundles: FreeType, HarfBuzz, libjpeg, OpenJPEG, jbig2dec, lcms2, MuJS, zlib, Brotli, Gumbo, Extract, Tesseract, Leptonica, zint, zxing-cpp, cmark-gfm) | 1.28.5 | `native/mupdf` (not in git) | New source from mupdf.com/releases into `native/mupdf`, then `native/build-mupdf.ps1` (Windows) and `native/build-mupdf-android.sh` (each Android ABI). Watch for API changes in `src-tauri/native/paginaut.c`. The `mupdf` npm package (tests only) follows it. |
| **libarchive** (RAR 5 comic books; only its RAR 5 reader is used) | 3.8.9 | `src-tauri/native/libarchive` (vendored subset) | Copy the same files from a new release over these (the list is `LIBARCHIVE` in `src-tauri/build.rs`); keep `paginaut_config.h`. Test with RAR 5 comics, plain and solid. |
| **unarr** (RAR 1.5 to 4 comic books, solid ones included) | 1.2.0, commit in `src-tauri/native/unarr/COMMIT` | `src-tauri/native/unarr` (vendored subset: `common`, `rar`, PPMd from `lzmasdk`) | Copy those folders from github.com/selmf/unarr at a newer commit; regenerate `unarr.h` from `unarr.h.in` (version numbers); record the commit. Test with RAR 4 comics, plain and solid. |
| **WebView2** (Windows: draws the app) | Evergreen | Installed with Windows / by Microsoft | Updates itself. The installer makes sure it's there. |
| **Android System WebView** (draws the Android app) | Whatever the phone has (79 or later needed) | The phone | Updates through the Play Store. Old ones (83 on stock Android 11 images) miss some CSS effects. |

## Build tools and frameworks

| What | Version | Where | How to update |
|---|---|---|---|
| Tauri (Rust crates `tauri`, `tauri-build`, plugins `opener`, `dialog`, `single-instance`) and `@tauri-apps/*` npm packages | 2.x (CLI/API 2.12) | `src-tauri/Cargo.toml`, `package.json` | `cargo update` in `src-tauri`, `npm update`; keep the Rust and npm sides on the same minor version. |
| Rust | 1.98.1 | rustup | `rustup update`; Android targets: `rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android`. |
| Other Rust crates (`serde`, `serde_json`, `parking_lot`, `once_cell`, `sysinfo`, `similar` (with `std`: Compare's time limit needs it), `minisign-verify`, `cc`, Windows: `windows`, `windows-core`, `webview2-com`; Linux: `keyring-core` + `dbus-secret-service-keyring-store`, `chacha20poly1305`, `sha1`/`sha2`, `getrandom`, `cryptoki`, `openssl`, `ureq`, `gtk`) | see `Cargo.lock` | `src-tauri/Cargo.toml` | `cargo update`; `cargo audit` for advisories. `windows`/`webview2-com` and `gtk` follow the versions Tauri uses, `foreign-types` the one `openssl` uses. The keyring store finds keys keyring 3 made (same service and user name). |
| Frontend (`preact` 11, `@preact/signals`, `lucide-preact`) and build (`esbuild`, `typescript` 7, `@types/node`) | see `package-lock.json` | `package.json` | `npm update`, then `npm run typecheck` and both builds (`node build.mjs`, `node build.mjs --mobile`). Preact 11 doesn't add px to numbers in inline styles: `src/preact-px.ts` does, loaded first by both apps. |
| Node.js | 24 | system | Current LTS. |
| Android Gradle Plugin / Kotlin / Gradle | 9.4.1 / 2.4.21 / 9.8.1 | `src-tauri/gen/android/build.gradle.kts`, `buildSrc`, `gradle/wrapper` | Android Studio's upgrade assistant, or edit the versions. |
| AndroidX (`webkit` 1.17.1, `appcompat` 1.8.0, `activity-ktx` 1.13.0, `material` 1.14.0, `lifecycle-process` 2.11.0) | as listed | `src-tauri/gen/android/app/build.gradle.kts` | Edit the versions; build and test on the oldest emulator (Android 7, API 24). |
| Jackson (used by Tauri's Android side) | pinned to 2.13.5 | same file, `resolutionStrategy` | Pinned because 2.15 crashes on Android 7. Only raise it with a test on API 24. |
| Android SDK / NDK | compile and target SDK 37, min SDK 24 / NDK 29.0.14206865 | Android Studio's SDK Manager | Google Play requires a recent target SDK every year (usually by August). New NDK: rebuild MuPDF for Android. |
| JDK | Android Studio's bundled JBR | `E:/Android Studio/jbr` | Comes with Android Studio updates. |

## Data the apps ship or download

| What | Version | Where | How to update |
|---|---|---|---|
| Tesseract OCR models (English, Hindi, Marathi ship; others download on request) | tessdata_fast 4.1.0 | `native/tessdata` (fetched by `native/get-tessdata.ps1`) | Change the tag in the script; downloads are checked against hashes in the app. |
| Piper (natural voices for Read aloud, Windows) | piper 2023.11.14-2; voices from rhasspy/piper-voices at one commit | Downloaded on request (`src-tauri/src/piper.rs`, `piper_voices.rs`) | Update the release and the voice commit together with their hashes. |
| App icons | drawn in `assets/icon.svg` | `src-tauri/icons`, Android mipmaps | `node assets/render-icon.mjs`, `npx tauri icon`, `E:/Anaconda/python.exe assets/android-icons.py`. |

## After any update

1. `npm run typecheck`, then build both apps (`npx tauri build`; `bash tools/android.sh <abi>`).
2. Open a PDF with forms, a scanned PDF, an EPUB, CBZ and CBR comics (RAR 4 and 5, plain and solid), XPS and Markdown.
3. On Android, the oldest emulator too (Android 7, API 24) and a 32-bit one.
