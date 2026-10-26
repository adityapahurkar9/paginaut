# Third-party notices

Paginaut is free software under the GNU Affero General Public License 3.0. It is built on the
works below; their licences ask for these notices to go with the app. (Which versions, and how to
update them: DEPENDENCIES.md.)

## MuPDF

Copyright (C) Artifex Software, Inc. Licensed under the GNU AGPL 3.0. MuPDF includes FreeType,
HarfBuzz, libjpeg, OpenJPEG, jbig2dec, Little CMS, MuJS, zlib, Brotli, Gumbo, Extract, zint,
zxing-cpp and cmark-gfm under their own licences (see the MuPDF source, `thirdparty/`).
https://mupdf.com

Save as PDF/A puts fonts into the files it writes. The base 14 ones (Helvetica, Times, Courier, Symbol,
ZapfDingbats and the stand-ins for fonts not on the computer) are URW's, from MuPDF, under the SIL Open Font
License 1.1, which allows embedding them in documents. Installed fonts are only put in when their own licence
allows embedding (OS/2 fsType). The sRGB colour profile it adds is MuPDF's own.

## Tesseract and Leptonica (Windows: text recognition)

Tesseract: Apache License 2.0. Leptonica: BSD 2-Clause. Models from tesseract-ocr/tessdata_fast,
Apache License 2.0.

## unarr (RAR 1.5 to 4 comic books)

Copyright the unarr project authors (see `src-tauri/native/unarr/AUTHORS`). Licensed under the GNU
Lesser General Public License 3.0 (`src-tauri/native/unarr/COPYING`); its RAR code is adapted from
The Unarchiver (LGPL). Includes the PPMd decoder from Igor Pavlov's LZMA SDK (public domain).
The full source of Paginaut, unarr included, is available under the AGPL, so it can be rebuilt with
a changed unarr. https://github.com/selmf/unarr

## libarchive (RAR 5 comic books)

The BLAKE2 files (`archive_blake2*`) are available under CC0 1.0, OpenSSL or Apache 2.0. The rest:

```
Copyright (c) 2003-2018 Tim Kientzle and the libarchive authors
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:
1. Redistributions of source code must retain the above copyright
   notice, this list of conditions and the following disclaimer
   in this position and unchanged.
2. Redistributions in binary form must reproduce the above copyright
   notice, this list of conditions and the following disclaimer in the
   documentation and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE AUTHOR(S) ``AS IS'' AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES
OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.
IN NO EVENT SHALL THE AUTHOR(S) BE LIABLE FOR ANY DIRECT, INDIRECT,
INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT
NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF
THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## Piper (Windows: natural voices, downloaded on request)

Piper: MIT licence. Voices from rhasspy/piper-voices, each under the licence given in its model
card.

## Tauri, Preact, Lucide

Tauri: MIT or Apache 2.0. Preact and @preact/signals: MIT. Lucide icons: ISC.
