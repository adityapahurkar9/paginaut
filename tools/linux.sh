#!/usr/bin/env bash
# The Linux app, built in WSL (or on a Linux machine) from this checkout.
#
# In WSL the project is copied into the Linux file system first (~/paginaut by default): building
# straight from /mnt/e through WSL's file bridge is several times slower. On a real Linux machine,
# set PAGINAUT_LINUX_DIR to the checkout itself to skip the copy.
#
# Usage (from Windows: wsl -d Ubuntu -- bash /mnt/e/Projects/PDF-Reader-Editor/tools/linux.sh <step>):
#   sync      copy the sources over (MuPDF's own build folder and Rust's are kept)
#   mupdf     build MuPDF's libraries (native/build-mupdf-linux.sh), once
#   check     cargo check
#   build     the app, debug (target/debug/paginaut)
#   run       build and start it (WSLg shows its window on Windows)
#   bundle    release build with the .deb and AppImage (runs on this system and newer)
#   release   the .deb and AppImage for every supported system: run it in Ubuntu 22.04
#             (wsl -d Ubuntu-22.04 -u root -- bash .../tools/linux.sh release). The page (dist/) is built
#             on Windows first (npm run build): 22.04's Node is too old, and the page is the same anywhere.
set -euo pipefail

src="$(cd "$(dirname "$0")/.." && pwd)"
dir="${PAGINAUT_LINUX_DIR:-$HOME/paginaut}"
# Linux programs only: WSL adds the Windows PATH too (Windows node and npm would build for Windows).
export PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin"

sync() {
  [ "$src" = "$dir" ] && return
  mkdir -p "$dir"
  rsync -a --delete \
    --exclude '/node_modules/' --exclude '/dist/' --exclude '/src-tauri/target/' --exclude '/src-tauri/gen/android/' \
    --exclude '/native/mupdf/build/' --exclude '/native/mupdf/platform/win32/x64/' --exclude '/.git/' \
    --exclude '/assets/ChatGPT*' --exclude '/assets/Gemini*' \
    "$src/" "$dir/"
  cd "$dir"
  # Windows' node_modules hold Windows builds of esbuild and Tauri's CLI: Linux gets its own.
  if [ ! -d node_modules ] || [ package-lock.json -nt node_modules/.package-lock.json ]; then
    npm ci --no-audit --no-fund
  fi
}

step=${1:-build}
case "$step" in
  sync) sync ;;
  mupdf) sync; cd "$dir" && bash native/build-mupdf-linux.sh ;;
  check) sync; cd "$dir" && npm run build --silent && (cd src-tauri && cargo check) ;;
  build) sync; cd "$dir" && npx tauri build --debug --no-bundle ;;
  run) sync; cd "$dir" && npx tauri build --debug --no-bundle && ./src-tauri/target/debug/paginaut "${@:2}" ;;
  # AppImage tools run unpacked: no FUSE in WSL or containers.
  bundle) sync; cd "$dir" && APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=1 npx tauri build --bundles deb,appimage ;;
  release)
    [ -f "$src/dist/index.html" ] || { echo "Build the page first on Windows: npm run build" >&2; exit 1; }
    dir="${PAGINAUT_LINUX_DIR:-$HOME/paginaut-release}"
    mkdir -p "$dir"
    rsync -a --delete       --exclude '/node_modules/' --exclude '/src-tauri/target/' --exclude '/src-tauri/gen/android/'       --exclude '/native/mupdf/build/' --exclude '/native/mupdf/platform/win32/x64/' --exclude '/.git/'       --exclude '/assets/ChatGPT*' --exclude '/assets/Gemini*'       "$src/" "$dir/"
    cd "$dir"
    [ -f native/mupdf/build/linux-$(uname -m)/libmupdf.a ] || bash native/build-mupdf-linux.sh
    command -v cargo-tauri >/dev/null || cargo install tauri-cli --version "^2" --locked
    APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=1 cargo tauri build --bundles deb,appimage --config '{"build":{"beforeBuildCommand":""}}'
    ls -la src-tauri/target/release/bundle/deb/*.deb src-tauri/target/release/bundle/appimage/*.AppImage
    ;;
  *) echo "unknown step: $step" >&2; exit 1 ;;
esac
