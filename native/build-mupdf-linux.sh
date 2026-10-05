#!/usr/bin/env bash
# Builds MuPDF's static libraries for the Linux app (x86_64 or aarch64, the machine's own):
#   native/mupdf/build/linux-<arch>/libmupdf.a, libmupdf-third.a
# Same MuPDF source (and patches) as build-mupdf.ps1, and the same parts as the Windows app: OCR
# (Tesseract and Leptonica), barcodes and the .docx writer (extract) built in; the large Chinese,
# Japanese and Korean fallback fonts left out (TOFU_CJK), as on Windows.
#
# Needs build-essential (gcc, g++, make) and pkg-config. Build on the oldest system the app should run
# on (Ubuntu 22.04 / Debian 12): the libraries link against that system's C library.
#
# Usage: bash native/build-mupdf-linux.sh
set -euo pipefail

cd "$(dirname "$0")/mupdf"
arch=$(uname -m)
jobs=${JOBS:-$(nproc)}
echo "== MuPDF for Linux ($arch)"
# HAVE_X11/GLUT/CURL=no: only the libraries (no viewer apps). The system's copies of zlib, libjpeg,
# freetype... aren't used: MuPDF's own (thirdparty) keep the same versions and patches as on Windows.
make -j"$jobs" libs \
  OS=Linux build=release OUT="build/linux-$arch" \
  HAVE_X11=no HAVE_GLUT=no HAVE_CURL=no HAVE_PTHREAD=yes \
  USE_SYSTEM_LIBS=no \
  tesseract=yes barcode=yes extract=yes \
  XCFLAGS="-fPIC -ffunction-sections -fdata-sections -DTOFU_CJK" \
  verbose=no
ls -la "build/linux-$arch"/libmupdf*.a
