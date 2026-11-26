# Paginaut on Linux

The desktop app on Linux is the same app as on Windows: the same MuPDF engine, the same interface and the
same features, with the parts Windows provides done the Linux way. This page records how, and every place
where there was more than one reasonable choice, so any of them can be revisited.

## Which systems

**Choice: Debian 12+, Ubuntu 22.04+ and the distributions built on them (Linux Mint 21+, Pop!_OS 22.04+,
elementary OS 7+, Zorin OS 17+, KDE neon, MX Linux 23+), on x86-64 (arm64 builds the same way).**

- They share WebKitGTK 4.1 (the web engine Tauri uses on Linux), GTK 3 and glibc 2.35 or newer.
- Other choices: also Fedora/openSUSE (RPM) — easy to add later (`tauri build --bundles rpm`), left out because
  the request was Debian/Ubuntu; Ubuntu 20.04 / Debian 11 — they lack WebKitGTK 4.1.
- Release builds should be made **on the oldest system supported** (Ubuntu 22.04 or Debian 12): a program
  linked on a newer system needs that system's C library. Development builds can be made anywhere.

## Packages

**Choice: a `.deb` (installed with the system's package manager, which also pulls in WebKitGTK) and an
AppImage (one file that runs without installing).**

- Others: Flatpak (sandboxed; file access through portals would need work in how files are opened and saved;
  worth doing for Flathub later), Snap (Ubuntu-only in practice), an apt repository (gives automatic updates;
  needs hosting and a signing key).
- The `.deb` depends on `libwebkit2gtk-4.1-0`, `libgtk-3-0`, `xdg-utils` and `libssl3` (`libssl3t64` on Ubuntu
  24.04+ and Debian 13); its desktop entry (`src-tauri/linux/paginaut.desktop`) lists every type Paginaut opens.
  **The .deb is the one to offer first.**
- The AppImage carries its own libraries (≈96 MB). Like every AppImage, it needs FUSE 2 to mount itself, which
  Ubuntu 22.04+ and Debian 12+ no longer install: `sudo apt install libfuse2` (`libfuse2t64` on Ubuntu 24.04+),
  or run it with `--appimage-extract-and-run`.

### Tested

| System | Package | Result |
|---|---|---|
| Ubuntu 22.04.5 (glibc 2.35) | .deb | Installs with apt; opens PDFs, comics |
| Debian 13 trixie (glibc 2.41) | .deb | Installs with apt (pulls WebKitGTK, libssl3t64); opens PDFs |
| Debian 13 trixie | AppImage | Runs with `--appimage-extract-and-run` (no libfuse2 there) |
| Ubuntu 26.04 | .deb (dev build) | Printing to a file, default apps, the repaired e-book, signing with a .pfx and a SoftHSM token |

The release binary needs at most glibc 2.35 and libstdc++ 3.4.29 (GCC 11).

## Building

- `bash native/build-mupdf-linux.sh` builds MuPDF (with Tesseract, barcodes and the .docx writer, as on
  Windows) once; `build.rs` links it.
- `tools/linux.sh` builds in WSL from this checkout (`sync`, `mupdf`, `check`, `test`, `build`, `run`, `bundle`; `test` sets up a SoftHSM token for the DSC tests). In WSL
  it copies the sources to the Linux file system first: building from `/mnt/e` is several times slower.
- Build machine packages (Debian/Ubuntu): `build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev
  librsvg2-dev libayatana-appindicator3-dev libxdo-dev libssl-dev patchelf file curl rsync nodejs npm`, and Rust
  from rustup.

## How each Windows part is done on Linux

| Part | Windows | Linux | Other choices |
|---|---|---|---|
| Web engine | WebView2 | WebKitGTK 4.1 | — |
| Downloads (OCR models, voices, updates) | WinHTTP | ureq + rustls, the system's certificates and proxy settings | libcurl (one more system library); the `curl` program (not always installed) |
| File hashes | BCrypt | RustCrypto (sha1, sha2) | OpenSSL |
| Saved signatures | DPAPI (per user) | ChaCha20-Poly1305 with a key in the desktop's keyring (GNOME Keyring, KWallet: Secret Service); without a keyring, a key file only this user can read. The file says which key sealed it, and only saving makes a key, so a keyring that is locked or not started yet never leads to a second one | Always a key file (weaker); asking for a password each time |
| Printing | Windows' print dialog, pages drawn by GDI | GTK's print dialog; the chosen pages written as a PDF and handed to CUPS, which prints PDF itself | GtkPrintOperation drawing each page as a picture (slower, holds up the window) |
| Default app | Offered in Windows, the user confirms in Default apps | Desktop entry offers every type; ticking one makes Paginaut its default with `xdg-mime` (Linux lets apps do this); what is the default now is read from the mimeapps.list files in the XDG order, not by running `xdg-mime` once per type. Giving a type back is done in the file manager | Only offering, never setting (as on Windows) |
| Office files | Microsoft Office (COM) or LibreOffice (downloaded by Paginaut if wanted) | LibreOffice from the distribution, its own .debs (/opt), a snap or a flatpak; Paginaut doesn't download it — it says how to install it | Downloading LibreOffice's AppImage (≈300 MB, outside the package manager's updates) |
| Read aloud | Windows' voices and Piper's | Piper's natural voices only (Linux has no system voice service with word timings for the follow-along highlight) | espeak-ng (robotic) or speech-dispatcher (no word timings) |
| Installed fonts (themes) | Registry | fontconfig (`fc-list`) | — |
| Fonts put into PDF/A files (Save as PDF/A) | Registry (name → file) | fontconfig (`fc-match`; a different family it offers counts as a stand-in and is named) | Its licence (OS/2 fsType) checked the same way |
| Memory figure | Process commit | Private memory from `/proc/<pid>/smaps_rollup` | RSS (counts shared libraries) |
| Window | Hidden until the first frame | Shown after the first frame or 300 ms, whichever first (WebKitGTK paints nothing for a hidden window) | — |
| Signing with certificates | Windows' certificate store (DSC drivers put certificates there), CryptoAPI | OpenSSL (libssl3). Certificate files (.pfx/.p12), and DSC tokens and smart cards through their PKCS#11 drivers: p11-kit (every registered driver, OpenSC's included), the paths the usual DSC tokens' Linux drivers use, or one chosen with *Token driver…*. The token signs; Paginaut assembles the same CMS as on Windows. The PIN is typed into Paginaut (Windows' drivers show their own window) and used for that signature only | Using libp11's OpenSSL engine (not installed everywhere); NSS's database (what Firefox uses; DSC users rarely load tokens there) |
| Updates | The installer, downloaded and checked | AppImage: the new one replaces it and starts. .deb: downloaded, checked, and installed by `apt-get` through `pkexec` (the system's own password prompt; Paginaut never holds the password), then Paginaut starts again once the old copy has quit. Without polkit, the .deb is opened in whichever app opens .debs | Opening the .deb in the software installer always (Ubuntu 22.04's store and the archive manager can't install a .deb file); an apt repository (the system updates Paginaut with everything else; needs hosting and a signing key) |
| Temporary files | The user's own temp folder | `~/.cache/paginaut/tmp` (mode 700), set as TMPDIR at start unless TMPDIR is set to something other than /tmp: /tmp is shared by every account, and print copies, rebuilt books and the update being installed have names someone else could make first | /tmp with random names (every place that makes a temporary file would need changing); `$XDG_RUNTIME_DIR` (in memory, often too small for a rebuilt book) |
| Certificate files | Windows reads every .pfx | OpenSSL 3's legacy module (in Debian's and Ubuntu's libssl3) is loaded for .pfx files encrypted with RC2 (older Windows exports, many CAs); without it, a clear message instead of "wrong password" | Asking users to export again with AES |
| Revocation (OCSP) | CryptoAPI checks the answer | The answer counts only when signed by the certificate's issuer (or a responder it appointed) and current; otherwise "unknown" | Trusting the answer as received (it comes over plain HTTP) |
| Rendering | GPU | WebKitGTK's DMA-BUF renderer off unless `WEBKIT_DISABLE_DMABUF_RENDERER` is set (blank windows in VMs, WSL and with some NVIDIA drivers) | Leaving it on (faster on some machines, blank on others) |

## Release builds

`tools/linux-setup.sh` prepares a build machine; in WSL an Ubuntu 22.04 distribution is used just for this
(`wsl --install Ubuntu-22.04`). Build the release page on Windows first (`node build.mjs --prod`; the release step refuses the development build), then
`wsl -d Ubuntu-22.04 -u root -- bash /mnt/e/Projects/PDF-Reader-Editor/tools/linux.sh release`: it copies
the sources over, builds MuPDF once, and makes the .deb and AppImage with Tauri's CLI from cargo (22.04's
Node is too old for the page's tools, and the page is the same anywhere).

The AppImage step downloads AppImage's runtime from GitHub while it packs. If the build machine can't reach
GitHub (on 2026-11-09 WSL's Ubuntu 22.04 couldn't resolve github.com), the .deb is made but Tauri stops with
"failed to run linuxdeploy". To finish by hand: download
`https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64` on any machine and put it
at `/root/.cache/tauri/runtime-x86_64` (a copy is kept there now), unpack the packing plugin once
(`cd /tmp/ldai && /root/.cache/tauri/linuxdeploy-plugin-appimage.AppImage --appimage-extract`), then in
`src-tauri/target/release/bundle/appimage/` of the build copy run
`ARCH=x86_64 /tmp/ldai/squashfs-root/appimagetool-prefix/usr/bin/appimagetool --runtime-file /root/.cache/tauri/runtime-x86_64 Paginaut.AppDir Paginaut_<version>_amd64.AppImage`.

## Still to do

- Testing on real Debian 12 / Ubuntu 22.04+ machines (GNOME, KDE, Xfce), with real printers and DSC tokens
  (tested here with CUPS's Print to File and SoftHSM).
- arm64 packages (the same steps on an arm64 machine).

## Testing in WSL

WSLg shows Linux windows on Windows. On Ubuntu 26.04 in WSL its Wayland socket isn't linked into the user's
runtime folder, so start the app with `XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir WAYLAND_DISPLAY=wayland-0`. For
automated checks, Xvfb (a virtual display) with `import` (screenshots) and `xdotool` (clicks and keys) works
without any display at all.
