# Makes a release: tests, builds the installer, signs it and the add-on list with the update key, and writes
# latest.json (the file the app's update check reads), all into release/v<version>/.
#   powershell -File tools/release.ps1 [-Notes "What's new"] [-SkipTests] [-NoBuild] [-Publish]
#   powershell -File tools/release.ps1 -AddonsOnly [-Publish]   re-sign release/v<version>/addons.json after editing
#                                                               it, to change add-ons without a new Paginaut
# -Publish uploads the files as a GitHub release of the releases repository (update.rs: RELEASES) with gh.
# Nothing is published without it. The update key is %USERPROFILE%\.paginaut\updater.key unless
# TAURI_SIGNING_PRIVATE_KEY_PATH says otherwise (and TAURI_SIGNING_PRIVATE_KEY_PASSWORD if it has one).
param([string]$Notes = "", [switch]$SkipTests, [switch]$NoBuild, [switch]$Publish, [switch]$AddonsOnly)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$conf = Get-Content (Join-Path $root "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
$version = $conf.version
$repo = [regex]::Match((Get-Content (Join-Path $root "src-tauri\src\update.rs") -Raw), 'pub const RELEASES: &str = "([^"]+)"').Groups[1].Value
$key = if ($env:TAURI_SIGNING_PRIVATE_KEY_PATH) { $env:TAURI_SIGNING_PRIVATE_KEY_PATH } else { Join-Path $env:USERPROFILE ".paginaut\updater.key" }
if (-not (Test-Path $key)) { throw "The update key isn't at $key (see the header of this script)." }
$env:TAURI_SIGNING_PRIVATE_KEY_PATH = $key
if (-not $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) { $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "" }
$stage = Join-Path $root "release\v$version"
New-Item -ItemType Directory -Force $stage | Out-Null
$setupName = "Paginaut_${version}_x64-setup.exe"

function Sign($file) {
  $out = cmd /c "npx tauri signer sign `"$file`" 2>&1"
  if ($LASTEXITCODE -ne 0 -or -not (Test-Path "$file.sig")) { $out; throw "Couldn't sign $file." }
}

if (-not $AddonsOnly) {
  if (-not $SkipTests) {
    & powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "test-all.ps1")
    if ($LASTEXITCODE -ne 0) { throw "Tests failed: nothing was released." }
  }
  Push-Location $root
  try {
    if (-not $NoBuild) {
      cmd /c "npx tauri build 2>&1" | Select-String "error|Finished|setup.exe|sign.ps1"
      if ($LASTEXITCODE -ne 0) { throw "The build failed." }
    }
    $built = Join-Path $root "src-tauri\target\release\bundle\nsis\$setupName"
    if (-not (Test-Path $built)) { throw "$setupName wasn't built." }
    Copy-Item $built $stage -Force
    Sign (Join-Path $stage $setupName)

    # The add-on list this version has built in (edit release\v<version>\addons.json and use -AddonsOnly to change it later).
    Push-Location src-tauri
    $cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    $json = cmd /c "`"$cargo`" run --release --quiet --example make_addons 2>nul"
    Pop-Location
    [IO.File]::WriteAllText((Join-Path $stage "addons.json"), ($json -join "`n"))
  } finally { Pop-Location }

  $latest = [ordered]@{
    version = $version
    notes = $(if ($Notes) { $Notes } else { "Paginaut $version" })
    pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = [ordered]@{
      "windows-x86_64" = [ordered]@{
        signature = (Get-Content (Join-Path $stage "$setupName.sig") -Raw).Trim()
        url = "https://github.com/$repo/releases/download/v$version/$setupName"
      }
    }
  }
  [IO.File]::WriteAllText((Join-Path $stage "latest.json"), ($latest | ConvertTo-Json -Depth 5))
}

$addons = Join-Path $stage "addons.json"
if (-not (Test-Path $addons)) { throw "No addons.json in $stage." }
Sign $addons

Write-Host "Ready in $stage :" -ForegroundColor Green
Get-ChildItem $stage | ForEach-Object { "  {0,-40} {1,10:N0} bytes" -f $_.Name, $_.Length }

if (-not $Publish) {
  Write-Host "Not published (add -Publish to upload to github.com/$repo)."
  exit 0
}
& gh repo view $repo --json name 2>$null | Out-Null
if ($LASTEXITCODE -ne 0) { throw "github.com/$repo doesn't exist (or gh isn't signed in): create it as a public repository first." }
if ($AddonsOnly) {
  & gh release upload "v$version" $addons "$addons.sig" --repo $repo --clobber
} else {
  # The notes go through a file: quotes, & and % in them reach GitHub as written.
  $notesFile = Join-Path $stage "notes.txt"
  [IO.File]::WriteAllText($notesFile, $latest.notes)
  $files = Get-ChildItem $stage -Exclude "notes.txt" | ForEach-Object { $_.FullName }
  & gh release create "v$version" @files --repo $repo --title "Paginaut $version" --notes-file $notesFile
  Remove-Item $notesFile
}
if ($LASTEXITCODE -ne 0) { throw "Publishing failed." }
Write-Host "Published: https://github.com/$repo/releases/tag/v$version" -ForegroundColor Green
