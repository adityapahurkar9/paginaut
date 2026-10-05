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
- The `.deb` depends on `libwebkit2gtk-4.1-0`, `libgtk-3-0` and `xdg-utils`; its desktop entry
  (`src-tauri/linux/paginaut.desktop`) lists every type Paginaut opens.

## Building

- `bash native/build-mupdf-linux.sh` builds MuPDF (with Tesseract, barcodes and the .docx writer, as on
  Windows) once; `build.rs` links it.
- `tools/linux.sh` builds in WSL from this checkout (`sync`, `mupdf`, `check`, `build`, `run`, `bundle`). In WSL
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
| Saved signatures | DPAPI (per user) | ChaCha20-Poly1305 with a key in the desktop's keyring (GNOME Keyring, KWallet: Secret Service); without a keyring, a key file only this user can read | Always a key file (weaker); asking for a password each time |
| Printing | Windows' print dialog, pages drawn by GDI | GTK's print dialog; the chosen pages written as a PDF and handed to CUPS, which prints PDF itself | GtkPrintOperation drawing each page as a picture (slower, holds up the window) |
| Default app | Offered in Windows, the user confirms in Default apps | Desktop entry offers every type; ticking one makes Paginaut its default with `xdg-mime` (Linux lets apps do this). Giving a type back is done in the file manager | Only offering, never setting (as on Windows) |
| Office files | Microsoft Office (COM) or LibreOffice (downloaded by Paginaut if wanted) | LibreOffice from the distribution, its own .debs (/opt), a snap or a flatpak; Paginaut doesn't download it — it says how to install it | Downloading LibreOffice's AppImage (≈300 MB, outside the package manager's updates) |
| Read aloud | Windows' voices and Piper's | Piper's natural voices only (Linux has no system voice service with word timings for the follow-along highlight) | espeak-ng (robotic) or speech-dispatcher (no word timings) |
| Installed fonts (themes) | Registry | fontconfig (`fc-list`) | — |
| Memory figure | Process commit | Private memory from `/proc/<pid>/smaps_rollup` | RSS (counts shared libraries) |
| Snap Layouts on the maximize button | snap.rs | Not needed: window managers have their own | — |
| Window | Hidden until the first frame | Shown after the first frame or 300 ms, whichever first (WebKitGTK paints nothing for a hidden window) | — |
| Rendering | GPU | WebKitGTK's DMA-BUF renderer off unless `WEBKIT_DISABLE_DMABUF_RENDERER` is set (blank windows in VMs, WSL and with some NVIDIA drivers) | Leaving it on (faster on some machines, blank on others) |

## Still to do

- Signing with certificates (digital signatures, DSC tokens): Windows uses its certificate store. Linux:
  `.p12`/`.pfx` files, and PKCS#11 for tokens and smart cards (OpenSC); verification against the system's CA
  certificates.
- Updates: the AppImage can replace itself (download, check the signature, swap); the `.deb` shows that a new
  version is out and opens its page. An apt repository would make the system update it.
- Testing on real Debian 12 / Ubuntu 22.04 machines (GNOME, KDE, Xfce).

## Testing in WSL

WSLg shows Linux windows on Windows. On Ubuntu 26.04 in WSL its Wayland socket isn't linked into the user's
runtime folder, so start the app with `XDG_RUNTIME_DIR=/mnt/wslg/runtime-dir WAYLAND_DISPLAY=wayland-0`. For
automated checks, Xvfb (a virtual display) with `import` (screenshots) and `xdotool` (clicks and keys) works
without any display at all.
