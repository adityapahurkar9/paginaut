# Validates PDF/A files with veraPDF (the reference validator; Java, not shipped with Paginaut).
#   powershell -File tools/pdfa-validate.ps1 -VeraPdf <path to verapdf.bat> -Folder <folder of PDFs> [-Java <java.exe>]
# Files named "... (PDF-A-1).pdf" are checked as PDF/A-1b, "... (PDF-A-3).pdf" as PDF/A-3b, the rest as PDF/A-2b; one PASS/FAIL line per file, with the
# failed rules (ISO 19005 clause-test) under a FAIL. Make the files first: cargo run --release --example pdfa_test.
param([Parameter(Mandatory)][string]$VeraPdf, [Parameter(Mandatory)][string]$Folder, [string]$Java = "")

$ErrorActionPreference = "Stop"
if ($Java) { $env:JAVACMD = $Java }
elseif (-not $env:JAVACMD -and (Test-Path "E:\Android Studio\jbr\bin\java.exe")) { $env:JAVACMD = "E:\Android Studio\jbr\bin\java.exe" }
$failed = 0
$files = @(Get-ChildItem (Join-Path $Folder "*(PDF-A*).pdf"))
# Nothing checked isn't a pass (a wrong folder, or files not made yet).
if (-not $files) { "No PDF/A files (named ""... (PDF-A).pdf"") in $Folder"; exit 1 }
# veraPDF's Java warnings go to stderr, which Windows PowerShell would stop on.
$ErrorActionPreference = "Continue"
$files | ForEach-Object {
  $flavour = if ($_.Name -like "*PDF-A-3*") { "3b" } elseif ($_.Name -like "*PDF-A-1*") { "1b" } else { "2b" }
  $out = & $VeraPdf --flavour $flavour --format text -v $_.FullName 2>$null
  $lines = $out | Where-Object { $_ -match "^(PASS|FAIL)|^\s+FAIL" }
  if ($lines -match "^FAIL") { $failed++ }
  $lines -replace [regex]::Escape((Resolve-Path $Folder).Path + "\"), ""
}
if ($failed) { "$failed failed"; exit 1 } else { "all passed" }
