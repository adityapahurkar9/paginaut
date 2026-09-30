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
if ($src -match "\{NAN, NAN\}") { Set-Content $geometry ($src -replace "NAN", "(0.0/0.0)") -NoNewline }

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

$env:CL = "/MP /DTOFU_CJK"
$target = if ($Clean) { "libmupdf:Rebuild" } else { "libmupdf" }
& $msbuild (Join-Path $root "platform\win32\mupdf.sln") "/target:$target" `
  /p:Configuration=Release /p:Platform=x64 /p:PlatformToolset=v143 `
  /p:WholeProgramOptimization=false /m /nologo /verbosity:minimal /nr:false
if ($LASTEXITCODE -ne 0) { throw "MuPDF build failed ($LASTEXITCODE)" }
