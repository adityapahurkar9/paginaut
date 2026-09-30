# The dependency routine: what's out of date or has a security advisory, and (with -Apply) updating the Rust and
# JavaScript packages behind the test gate.
#   powershell -File tools/deps.ps1            report only
#   powershell -File tools/deps.ps1 -Apply     update within compatible versions, run tools/test-all.ps1, and put
#                                              the lock files back if anything fails
# Run it monthly, and at once when an advisory appears (GitHub's Dependabot alerts cover the repository too:
# .github/dependabot.yml). MuPDF (which brings Tesseract) and the OCR models are rebuilt by hand; the report
# says when they have a newer release.
param([switch]$Apply)

$ErrorActionPreference = "Continue"
$root = Split-Path $PSScriptRoot -Parent
$cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"

function Head($t) { Write-Host ""; Write-Host "== $t" -ForegroundColor Cyan }
function Latest-Tag($repo, $pattern) {
  try {
    $tags = Invoke-RestMethod "https://api.github.com/repos/$repo/tags?per_page=100" -Headers @{ "User-Agent" = "Paginaut-deps" }
    $tags.name | Where-Object { $_ -match $pattern } | Sort-Object { [version]($_ -replace '[^0-9.]', '') } -Descending | Select-Object -First 1
  } catch { "(couldn't ask GitHub: $($_.Exception.Message))" }
}

Push-Location $root

Head "MuPDF (brings Tesseract and the other native libraries)"
$mupdfNow = [regex]::Match((Get-Content native\build-mupdf.ps1 -Raw), 'MuPDF (\d+\.\d+\.\d+)').Groups[1].Value
$mupdfNew = Latest-Tag "ArtifexSoftware/mupdf" '^\d+\.\d+\.\d+$'
"  in use: $mupdfNow    newest release: $mupdfNew"
if ($mupdfNew -and $mupdfNow -and ($mupdfNew -as [version]) -gt ($mupdfNow -as [version])) {
  Write-Host "  Newer MuPDF: download its source from mupdf.com/releases, run native/build-mupdf.ps1, then tools/test-all.ps1 -Office." -ForegroundColor Yellow
}

Head "OCR models (Tesseract's tessdata)"
$tagNow = [regex]::Match((Get-Content src-tauri\src\ocr_langs.rs -Raw), 'pub const TAG: &str = "([^"]+)"').Groups[1].Value
$tagNew = Latest-Tag "tesseract-ocr/tessdata_fast" '^\d+\.\d+\.\d+$'
"  in use: $tagNow    newest tag: $tagNew"

Head "Rust packages (within compatible versions; --verbose lists newer major versions)"
Push-Location src-tauri
cmd /c "`"$cargo`" update --dry-run --verbose 2>&1" | Select-String "Updating|Unchanged .*available|Locking" | ForEach-Object { "  " + $_.Line.Trim() }
if (Get-Command cargo-audit -ErrorAction SilentlyContinue) {
  Head "Rust security advisories (cargo audit)"
  cmd /c "`"$cargo`" audit 2>&1" | Select-Object -Last 15
} else {
  "  (cargo install cargo-audit adds a check against the RustSec advisory database)"
}
Pop-Location

Head "JavaScript packages"
cmd /c "npm outdated 2>&1"
Head "JavaScript security advisories (npm audit)"
cmd /c "npm audit --omit=dev 2>&1" | Select-Object -Last 8

if (-not $Apply) { Pop-Location; exit 0 }

Head "Updating"
$dirty = git status --porcelain -- src-tauri/Cargo.lock package-lock.json
if ($dirty) { Write-Host "Cargo.lock or package-lock.json has uncommitted changes: commit or discard them first." -ForegroundColor Red; Pop-Location; exit 1 }
Push-Location src-tauri
cmd /c "`"$cargo`" update 2>&1" | Select-String "Updating|Locking" | ForEach-Object { "  " + $_.Line.Trim() }
Pop-Location
cmd /c "npm update 2>&1" | Select-Object -Last 3
& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "test-all.ps1")
if ($LASTEXITCODE -ne 0) {
  Write-Host "The tests failed with the updates: the lock files are put back." -ForegroundColor Red
  git checkout -- src-tauri/Cargo.lock package-lock.json
  cmd /c "npm ci 2>&1" | Select-Object -Last 2
  Pop-Location
  exit 1
}
Write-Host "Updated and every test passed: commit Cargo.lock and package-lock.json." -ForegroundColor Green
Pop-Location
