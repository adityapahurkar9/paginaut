# Dev loop: close Paginaut, rebuild frontend + app, relaunch with DevTools on port 9333.
#   powershell -File tools/dev.ps1 [file ...] [-Release] [-NoBuild] [-Fresh]
param([switch]$Release, [switch]$NoBuild, [switch]$Fresh, [Parameter(ValueFromRemainingArguments)] [string[]]$Files)

# cargo and node print progress on stderr; don't let PowerShell treat that as failure.
$ErrorActionPreference = "Continue"
$root = Split-Path $PSScriptRoot -Parent
Get-Process paginaut -ErrorAction SilentlyContinue | Stop-Process -Force -Confirm:$false
Start-Sleep -Milliseconds 300

if (-not $NoBuild) {
  Push-Location $root
  $out = cmd /c "node build.mjs $(if ($Release) { '--prod' }) 2>&1"
  if ($LASTEXITCODE -ne 0) { $out; Pop-Location; exit 1 }
  Push-Location src-tauri
  $cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
  $mode = if ($Release) { "--release" } else { "" }
  $out = cmd /c "`"$cargo`" build $mode 2>&1"
  $code = $LASTEXITCODE
  $out | Select-String -Pattern "^error|^warning: unused|error\[|-->|Finished" | ForEach-Object { $_.Line }
  Pop-Location; Pop-Location
  if ($code -ne 0) { "cargo build failed"; exit 1 }
}

if ($Fresh) {
  Remove-Item -Recurse -Force "$env:LOCALAPPDATA\app.paginaut.reader\EBWebView\Default\Local Storage" -ErrorAction SilentlyContinue
}

$exe = Join-Path $root ("src-tauri\target\" + $(if ($Release) { "release" } else { "debug" }) + "\paginaut.exe")
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9333 $env:PAGINAUT_EXTRA_ARGS"
$argList = @($Files | Where-Object { $_ } | ForEach-Object { '"' + (Resolve-Path $_).Path + '"' })
if ($argList.Count) { Start-Process -FilePath $exe -ArgumentList $argList | Out-Null } else { Start-Process -FilePath $exe | Out-Null }
"launched $exe"
