#!/usr/bin/env bash
# Builds MuPDF's static libraries for Android with the NDK, one folder per ABI:
#   native/mupdf/build/android-<abi>/libmupdf.a, libmupdf-third.a
# Same MuPDF source (and patches) as build-mupdf.ps1: run that first after unpacking MuPDF.
# Needs GNU make (winget install ezwinports.make) and bash (Git Bash); NDK_HOME set.
#
# The viewer leaves out OCR (Tesseract), barcodes and the .docx writer (extract);
# like the Windows build, the large Chinese/Japanese/Korean fallback fonts are left out.
#
# Usage: bash native/build-mupdf-android.sh [arm64-v8a armeabi-v7a x86 x86_64]
set -euo pipefail

cd "$(dirname "$0")/mupdf"
: "${NDK_HOME:?set NDK_HOME to the Android NDK}"
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) host=windows-x86_64; exe=.exe ;;
  Darwin) host=darwin-x86_64; exe= ;;
  *) host=linux-x86_64; exe= ;;
esac
bin="$NDK_HOME/toolchains/llvm/prebuilt/$host/bin"
api=24
make=${MAKE:-make}
jobs=${JOBS:-$(nproc 2>/dev/null || echo 8)}

abis=("$@")
[ ${#abis[@]} -eq 0 ] && abis=(arm64-v8a armeabi-v7a x86 x86_64)

for abi in "${abis[@]}"; do
  case "$abi" in
    arm64-v8a) target=aarch64-linux-android; emu=aarch64linux ;;
    armeabi-v7a) target=armv7a-linux-androideabi; emu=armelf_linux_eabi ;;
    x86) target=i686-linux-android; emu=elf_i386 ;;
    x86_64) target=x86_64-linux-android; emu=elf_x86_64 ;;
    *) echo "unknown ABI $abi" >&2; exit 1 ;;
  esac
  echo "== MuPDF for $abi"
  # OS=Linux: MuPDF's Linux rules (Windows' own OS=Windows_NT would otherwise be picked up).
  # HAVE_OBJCOPY: fonts linked in as binary objects by lld rather than compiled from huge C arrays.
  "$make" -j"$jobs" libs \
    OS=Linux build=release OUT="build/android-$abi" \
    CC="$bin/clang$exe --target=$target$api" CXX="$bin/clang++$exe --target=$target$api" \
    AR="$bin/llvm-ar$exe" LD="$bin/ld.lld$exe -m $emu" RANLIB="$bin/llvm-ranlib$exe" \
    HAVE_OBJCOPY=yes HAVE_X11=no HAVE_GLUT=no HAVE_CURL=no HAVE_PTHREAD=yes \
    extract=no barcode=no tesseract=no tofu_cjk=yes \
    XCFLAGS="-fPIC -ffunction-sections -fdata-sections" \
    verbose=no
done
