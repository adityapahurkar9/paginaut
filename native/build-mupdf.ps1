# Builds MuPDF's static libraries (Release, x64) with MSBuild.
# Source: MuPDF 1.28.5 (mupdf-1.28.5-source from mupdf.com/releases) unpacked into
# native/mupdf; it isn't kept in the repository.
# Output: native/mupdf/platform/win32/x64/Release/*.lib
#
# - TOFU_CJK leaves out the large Chinese/Japanese/Korean fallback fonts
#   (they become an optional download); Noto fonts for Indic and other
#   scripts stay in.
# - Whole-program optimisation is off so linking the app stays fast.
param([switch]$Clean)

$ErrorActionPreference = "Stop"
$root = Join-Path $PSScriptRoot "mupdf"
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$msbuild = & $vswhere -latest -products * -requires Microsoft.Component.MSBuild -find "MSBuild\**\Bin\amd64\MSBuild.exe" | Select-Object -First 1
if (-not $msbuild) { throw "MSBuild not found. Install Visual Studio Build Tools with the C++ workload." }

# MSVC 17.10+ no longer treats NAN as a compile-time constant in C.
$geometry = Join-Path $root "source\fitz\geometry.c"
$src = Get-Content $geometry -Raw
# Only the constant NAN, in capitals: -replace ignores case, and would turn isnan( into is(0.0/0.0)( as well.
if ($src -cmatch "\{NAN, NAN\}") { Set-Content $geometry ($src -creplace "NAN", "(0.0/0.0)") -NoNewline }

# Saving a gray or RGB picture with an ICC profile as JPEG converted it to RGB, while the
# image rewriter (Reduce file size) kept the gray colour space: scans came out garbled.
# Only convert what JPEG can't hold, as fz_write_pixmap_as_jpeg itself checks.
$jpeg = Join-Path $root "source\fitz\output-jpeg.c"
$src = Get-Content $jpeg -Raw
$old = "pix->colorspace != fz_device_gray(ctx) && pix->colorspace != fz_device_rgb(ctx) && pix->colorspace != fz_device_cmyk(ctx)"
$new = "!fz_colorspace_is_gray(ctx, pix->colorspace) && !fz_colorspace_is_rgb(ctx, pix->colorspace) && !fz_colorspace_is_cmyk(ctx, pix->colorspace)"
if ($src.Contains($old)) { Set-Content $jpeg $src.Replace($old, $new) -NoNewline }

# pdf_add_embedded_file begins a journal operation and, when it succeeds, never ends it:
# after attaching a file, undo refused to work ("Can't undo/redo during an operation").
$link = Join-Path $root "source\pdf\pdf-link.c"
$src = Get-Content $link -Raw
$old = "		filespec = pdf_add_filespec(ctx, doc, filename, file);`n	}`n	fz_always(ctx)`n		pdf_drop_obj(ctx, file);"
$new = "		filespec = pdf_add_filespec(ctx, doc, filename, file);`n		pdf_end_operation(ctx, doc);`n	}`n	fz_always(ctx)`n		pdf_drop_obj(ctx, file);"
if ($src.Contains($old)) { Set-Content $link $src.Replace($old, $new) -NoNewline }
elseif (-not $src.Contains($new)) { Write-Warning "pdf-link.c changed: check pdf_add_embedded_file still ends its operation" }

# The Markdown reader doesn't ask for the file's folder (the HTML one does), so pictures a .md file links to
# never load ("[image]" instead).
$md = Join-Path $root "source\html\md.c"
$src = Get-Content $md -Raw
$old = "	md_mimetypes,`n	md_recognize_doc_content`n};"
$new = "	md_mimetypes,`n	md_recognize_doc_content,`n	1`n};"
if ($src.Contains($old)) { Set-Content $md $src.Replace($old, $new) -NoNewline }
elseif (-not $src.Contains($new)) { Write-Warning "md.c changed: check the Markdown handler still asks for its folder (wants_dir)" }

# The PDF writer device drops marked text (ActualText, Alt): text shaped from several letters into one glyph
# (Indic conjuncts, Arabic forms) that Paginaut stamps or edits in copied and searched as blanks, since a PDF
# font's ToUnicode maps single glyphs. Written as a /Span now; paginaut.c wraps such text in ActualText.
$dev = Join-Path $root "source\pdf\pdf-device.c"
$src = Get-Content $dev -Raw
if (-not $src.Contains("pdf_dev_begin_metatext")) {
  $fns = @'
/* Paginaut: marked text as a /Span, so glyphs shaped from several letters copy and search as the letters. */
static void
pdf_dev_begin_metatext(fz_context *ctx, fz_device *dev, fz_metatext meta, const char *text)
{
	pdf_device *pdev = (pdf_device*)dev;
	gstate *gs;
	const char *key = meta == FZ_METATEXT_ACTUALTEXT ? "ActualText" : meta == FZ_METATEXT_ABBREVIATION ? "E" : "Alt";
	pdf_dev_end_text(ctx, pdev);
	gs = CURRENT_GSTATE(pdev);
	fz_append_printf(ctx, gs->buf, "/Span <</%s <FEFF", key);
	while (text && *text)
	{
		int c;
		text += fz_chartorune(&c, text);
		if (c >= 0x10000)
		{
			c -= 0x10000;
			fz_append_printf(ctx, gs->buf, "%04X%04X", 0xD800 + (c >> 10), 0xDC00 + (c & 0x3FF));
		}
		else
			fz_append_printf(ctx, gs->buf, "%04X", c);
	}
	fz_append_string(ctx, gs->buf, ">>> BDC\n");
}

static void
pdf_dev_end_metatext(fz_context *ctx, fz_device *dev)
{
	pdf_device *pdev = (pdf_device*)dev;
	pdf_dev_end_text(ctx, pdev);
	fz_append_string(ctx, CURRENT_GSTATE(pdev)->buf, "EMC\n");
}

'@
  $at = "fz_device *pdf_new_pdf_device(fz_context *ctx"
  $reg = "`tdev->super.end_tile = pdf_dev_end_tile;"
  if (-not ($src.Contains($at) -and $src.Contains($reg))) { throw "pdf-device.c changed: the ActualText patch needs a new place" }
  $src = $src.Replace($at, $fns + $at).Replace($reg, $reg + "`n`tdev->super.begin_metatext = pdf_dev_begin_metatext;`n`tdev->super.end_metatext = pdf_dev_end_metatext;")
  Set-Content $dev $src -NoNewline
}

# Text extraction counts what's left of an ActualText in bytes where it means letters: with letters beyond
# ASCII (Devanagari takes 3 bytes each) the end of the replacement came out twice ("क्रमांकमांक").
$stext = Join-Path $root "source\fitz\stext-device.c"
$src = Get-Content $stext -Raw
$old = "flush_actualtext(ctx, dev, actualtext, i, i + (int)strlen(actualtext) - (span->len - end));"
$new = "flush_actualtext(ctx, dev, actualtext, i, i + (int)fz_utflen(actualtext) - (span->len - end));"
if ($src.Contains($old)) { Set-Content $stext $src.Replace($old, $new) -NoNewline }
elseif (-not $src.Contains($new)) { Write-Warning "stext-device.c changed: check ActualText is still counted in letters" }

$env:CL = "/MP /DTOFU_CJK"
$target = if ($Clean) { "libmupdf:Rebuild" } else { "libmupdf" }
& $msbuild (Join-Path $root "platform\win32\mupdf.sln") "/target:$target" `
  /p:Configuration=Release /p:Platform=x64 /p:PlatformToolset=v143 `
  /p:WholeProgramOptimization=false /m /nologo /verbosity:minimal /nr:false
if ($LASTEXITCODE -ne 0) { throw "MuPDF build failed ($LASTEXITCODE)" }
