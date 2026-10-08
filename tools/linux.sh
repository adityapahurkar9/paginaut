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
#   test      the Rust tests, the token ones included: a SoftHSM token (a token in software) is set up in
#             ~/softhsm the first time (needs the softhsm2 and opensc packages)
#   build     the app, debug (target/debug/paginaut)
#   run       build and start it (WSLg shows its window on Windows)
#   bundle    release build with the .deb and AppImage (runs on this system and newer)
#   release   the .deb and AppImage for every supported system: run it in Ubuntu 22.04
#             (wsl -d Ubuntu-22.04 -u root -- bash .../tools/linux.sh release). The page (dist/) is built
#             on Windows first (node build.mjs --prod): 22.04's Node is too old, and the page is the same anywhere.
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

# The token the PKCS#11 tests sign with (pkcs11.rs, dsc.rs): "Paginaut DSC", PIN 1234, holding a key and its
# certificate "Token Signer" under the same id.
token() {
  local home="$HOME/softhsm" so=/usr/lib/softhsm/libsofthsm2.so
  export PAGINAUT_SOFTHSM_CONF="$home/softhsm2.conf" SOFTHSM2_CONF="$home/softhsm2.conf"
  if ! command -v softhsm2-util >/dev/null || ! command -v pkcs11-tool >/dev/null; then
    echo "The token tests need SoftHSM and OpenSC: sudo apt install softhsm2 opensc" >&2
    exit 1
  fi
  softhsm2-util --show-slots 2>/dev/null | grep -q "Paginaut DSC" && return
  mkdir -p "$home/tokens"
  echo "directories.tokendir = $home/tokens" > "$SOFTHSM2_CONF"
  softhsm2-util --init-token --free --label "Paginaut DSC" --pin 1234 --so-pin 12345678 >/dev/null
  local t
  t=$(mktemp -d)
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$t/key.pem" -out "$t/cert.pem" -subj "/CN=Token Signer" -days 3650 2>/dev/null
  openssl x509 -in "$t/cert.pem" -outform der -out "$t/cert.der"
  softhsm2-util --import "$t/key.pem" --token "Paginaut DSC" --label "Token Signer" --id 01 --pin 1234 >/dev/null
  pkcs11-tool --module "$so" --token-label "Paginaut DSC" --login --pin 1234 --write-object "$t/cert.der" --type cert --id 01 --label "Token Signer" >/dev/null
  rm -rf "$t"
}

step=${1:-build}
case "$step" in
  sync) sync ;;
  mupdf) sync; cd "$dir" && bash native/build-mupdf-linux.sh ;;
  check) sync; cd "$dir" && npm run build --silent && (cd src-tauri && cargo check) ;;
  test)
    sync; token; cd "$dir" && npm run build --silent
    cd src-tauri
    if command -v gnome-keyring-daemon >/dev/null && command -v dbus-run-session >/dev/null; then
      # Saved signatures sealed in a real keyring: a throwaway one, unlocked (one started on demand would wait,
      # unseen, for its password).
      keys=$(mktemp -d)
      XDG_DATA_HOME="$keys" dbus-run-session -- bash -c 'echo -n test | gnome-keyring-daemon --unlock --components=secrets >/dev/null && cargo test --lib'
      rm -rf "$keys"
    else
      cargo test --lib
    fi
    ;;
  build) sync; cd "$dir" && npx tauri build --debug --no-bundle ;;
  run) sync; cd "$dir" && npx tauri build --debug --no-bundle && ./src-tauri/target/debug/paginaut "${@:2}" ;;
  # AppImage tools run unpacked: no FUSE in WSL or containers.
  bundle) sync; cd "$dir" && APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=1 npx tauri build --bundles deb,appimage ;;
  release)
    # The release page: minified, without the development build's source map (npm run build makes that one).
    if [ ! -f "$src/dist/index.html" ] || grep -q "sourceMappingURL" "$src/dist/app.js"; then
      echo "Build the release page first on Windows: node build.mjs --prod" >&2
      exit 1
    fi
    dir="${PAGINAUT_LINUX_DIR:-$HOME/paginaut-release}"
    mkdir -p "$dir"
    rsync -a --delete       --exclude '/node_modules/' --exclude '/src-tauri/target/' --exclude '/src-tauri/gen/android/'       --exclude '/native/mupdf/build/' --exclude '/native/mupdf/platform/win32/x64/' --exclude '/.git/'       --exclude '/assets/ChatGPT*' --exclude '/assets/Gemini*'       "$src/" "$dir/"
    cd "$dir"
    # (A development build's source map left in dist/ would be packed into the app.)
    rm -f dist/*.map
    [ -f native/mupdf/build/linux-$(uname -m)/libmupdf.a ] || bash native/build-mupdf-linux.sh
    command -v cargo-tauri >/dev/null || cargo install tauri-cli --version "^2" --locked
    APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=1 cargo tauri build --bundles deb,appimage --config '{"build":{"beforeBuildCommand":""}}'
    ls -la src-tauri/target/release/bundle/deb/*.deb src-tauri/target/release/bundle/appimage/*.AppImage
    ;;
  *) echo "unknown step: $step" >&2; exit 1 ;;
esac
