# The test gate: everything that must pass before a dependency bump or a release is kept.
#   powershell -File tools/test-all.ps1 [-Office] [-Quick]
# Runs the TypeScript check, the Rust unit tests and the engine suites (rendering, saving, editing, redaction,
# forms of protection, stamps, page sizes, bookmarks, inserting, extracting, Compare) on copies of testfiles/.
# -Office adds the Office conversion suites (slow; need Microsoft Office or the LibreOffice pack).
# -Quick skips the engine suites. Exits 1 if anything failed; a summary is printed at the end.
param([switch]$Office, [switch]$Quick)

$ErrorActionPreference = "Continue"
$root = Split-Path $PSScriptRoot -Parent
$cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
$files = Join-Path $root "testfiles"
$temp = Join-Path ([IO.Path]::GetTempPath()) "paginaut-tests"
$results = [ordered]@{}

function Step($name, [scriptblock]$run) {
  Write-Host "== $name" -ForegroundColor Cyan
  $t = [Diagnostics.Stopwatch]::StartNew()
  & $run
  $ok = $LASTEXITCODE -eq 0
  $results[$name] = "{0}  {1:N0} s" -f $(if ($ok) { "pass" } else { "FAIL" }), $t.Elapsed.TotalSeconds
}

Push-Location $root
Step "TypeScript" { cmd /c "npx tsc --noEmit -p . 2>&1" | Select-Object -Last 20 }
Push-Location src-tauri
Step "Rust unit tests" { cmd /c "`"$cargo`" test --lib 2>&1" | Select-String "test result|FAILED|panicked" }

if (-not $Quick) {
  if (-not (Test-Path $files)) {
    Write-Host "testfiles/ is missing: the engine suites need it (tools/make-test-files.mjs, tools/make-test-pdfs.mjs)." -ForegroundColor Yellow
    $results["Engine suites"] = "SKIPPED (no testfiles/)"
  } else {
    New-Item -ItemType Directory -Force $temp | Out-Null
    $suites = "save", "edit", "redact", "secure", "stamp", "pagesize", "meta", "insert", "extract", "compare"
    if ($Office) { $suites += "office", "convert" }
    # Build them all once, then run each.
    cmd /c "`"$cargo`" build --release --examples 2>&1" | Select-String "^error|Finished"
    foreach ($s in $suites) {
      $dir = Join-Path $temp $s
      New-Item -ItemType Directory -Force $dir | Out-Null
      Step "suite: $s" { cmd /c "`"$cargo`" run --release --quiet --example ${s}_test -- `"$files`" `"$dir`" 2>&1" | Select-Object -Last 6 }
    }
  }
}
Pop-Location
Pop-Location

Write-Host ""
Write-Host "Summary" -ForegroundColor Cyan
$failed = 0
foreach ($k in $results.Keys) {
  $v = $results[$k]
  if ($v -like "FAIL*") { $failed++ }
  "{0,-28} {1}" -f $k, $v
}
if ($failed) { Write-Host "$failed failed." -ForegroundColor Red; exit 1 }
Write-Host "All passed." -ForegroundColor Green
exit 0
