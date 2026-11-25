# Makes a release: tests, builds the installer, signs it and the add-on list with the update key, and writes
# latest.json (the file the app's update check reads), all into release/v<version>/.
#   powershell -File tools/release.ps1 [-Notes "What's new"] [-SkipTests] [-NoBuild] [-FromCI] [-Publish]
#   powershell -File tools/release.ps1 -AddonsOnly [-Publish]   re-sign release/v<version>/addons.json after editing
#                                                               it, to change add-ons without a new Paginaut
# -FromCI takes the installer from the "Windows release" workflow (.github/workflows/release-windows.yml) instead of
# building it here: built on GitHub from the commit pushed, and code-signed by SignPath. Run the workflow for this
# version first (gh workflow run release-windows.yml --ref main) and approve its signing requests; this fetches its
# signed installer, checks the signature, and goes on as usual (update key, add-on list, latest.json).
# -Publish uploads the files as a GitHub release of the releases repository (update.rs: RELEASES) with gh,
# Linux files included (it stops if tools/linux.sh release hasn't put them in release/v<version>/ yet; run
# again with -SkipTests -NoBuild -Publish once it has). Nothing is published without it. Every run keeps
# only the newest 5 releases in release/ (tools/prune-releases.ps1); older ones live on GitHub. The update key is %USERPROFILE%\.paginaut\updater.key unless
# TAURI_SIGNING_PRIVATE_KEY_PATH says otherwise (and TAURI_SIGNING_PRIVATE_KEY_PASSWORD if it has one).
param([string]$Notes = "", [switch]$SkipTests, [switch]$NoBuild, [switch]$FromCI, [switch]$Publish, [switch]$AddonsOnly)

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
    $builtFromCI = $null
    if ($FromCI) {
      # The newest successful run that kept this version's signed installer.
      $name = "windows-signed-v$version"
      $runs = gh run list --repo $repo --workflow release-windows.yml --status success --limit 20 --json databaseId,headSha | ConvertFrom-Json
      if ($LASTEXITCODE -ne 0) { throw "Couldn't list the workflow's runs (is gh signed in?)." }
      $run = $null
      foreach ($r in $runs) {
        $arts = gh api "repos/$repo/actions/runs/$($r.databaseId)/artifacts" --jq ".artifacts[] | select(.name == `"$name`" and (.expired | not)) | .name"
        if ($arts) { $run = $r; break }
      }
      if (-not $run) { throw "No successful Windows release run has ${name}: run the workflow (gh workflow run release-windows.yml --ref main) and approve its signing in SignPath." }
      $head = (git rev-parse HEAD).Trim()
      if ($run.headSha -ne $head) { Write-Warning "The installer was built from $($run.headSha); this checkout is at $head." }
      $ci = Join-Path ([IO.Path]::GetTempPath()) "paginaut-ci-$version"
      if (Test-Path $ci) { Remove-Item $ci -Recurse -Force }
      gh run download $run.databaseId --repo $repo --name $name --dir $ci
      if ($LASTEXITCODE -ne 0) { throw "Couldn't download $name from run $($run.databaseId)." }
      $builtFromCI = Join-Path $ci $setupName
      $code = Get-AuthenticodeSignature $builtFromCI
      if ($code.Status -ne "Valid") { throw "$setupName from the workflow isn't validly code-signed: $($code.StatusMessage)" }
      Write-Host "Code-signed by $($code.SignerCertificate.Subject)"
      # Staged as if just built (an update-key signature of an earlier copy is dropped; it's signed again below).
      Remove-Item (Join-Path $stage "$setupName.sig") -ErrorAction SilentlyContinue
    } elseif (-not $NoBuild) {
      cmd /c "npx tauri build 2>&1" | Select-String "error|Finished|setup.exe|sign.ps1"
      if ($LASTEXITCODE -ne 0) { throw "The build failed." }
    }
    $built = if ($builtFromCI) { $builtFromCI } else { Join-Path $root "src-tauri\target\release\bundle\nsis\$setupName" }
    $staged = Join-Path $stage $setupName
    # -NoBuild after the build cache was cleared: use the installer already staged (and signed) here.
    $reuse = $NoBuild -and -not $FromCI -and -not (Test-Path $built) -and (Test-Path $staged) -and (Test-Path "$staged.sig")
    if (-not $reuse) {
      if (-not (Test-Path $built)) { throw "$setupName wasn't built." }
      Copy-Item $built $stage -Force
      Sign $staged
    }

    # The add-on list this version has built in (edit release\v<version>\addons.json and use -AddonsOnly to change it later).
    if (-not ($reuse -and (Test-Path (Join-Path $stage "addons.json")))) {
      Push-Location src-tauri
      $cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
      $json = cmd /c "`"$cargo`" run --release --quiet --example make_addons 2>nul"
      Pop-Location
      [IO.File]::WriteAllText((Join-Path $stage "addons.json"), ($json -join "`n"))
    }
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
  # Linux's .deb and AppImage (tools/linux.sh release puts them here), signed with the same key, so the Linux app's
  # update check finds them (update.rs Package::platforms; before 0.9.14 they were never listed, and Linux was never
  # offered an update). Not there yet on the first run: the publish run (-SkipTests -NoBuild -Publish) writes
  # latest.json again with them, and publishing stops without them.
  $linux = @(
    @{ key = "linux-x86_64-deb"; filter = "Paginaut_${version}_*.deb" },
    @{ key = "linux-x86_64-appimage"; filter = "Paginaut_${version}_*.AppImage" }
  )
  foreach ($kind in $linux) {
    $f = Get-ChildItem $stage -Filter $kind.filter | Select-Object -First 1
    if (-not $f) { continue }
    $sig = "$($f.FullName).sig"
    # A signature older than its file is of an earlier build: signed again.
    if (-not (Test-Path $sig) -or (Get-Item $sig).LastWriteTime -lt $f.LastWriteTime) { Sign $f.FullName }
    $latest.platforms[$kind.key] = [ordered]@{
      signature = (Get-Content $sig -Raw).Trim()
      url = "https://github.com/$repo/releases/download/v$version/$($f.Name)"
    }
  }
  [IO.File]::WriteAllText((Join-Path $stage "latest.json"), ($latest | ConvertTo-Json -Depth 5))
}

$addons = Join-Path $stage "addons.json"
if (-not (Test-Path $addons)) { throw "No addons.json in $stage." }
Sign $addons

Write-Host "Ready in $stage :" -ForegroundColor Green
Get-ChildItem $stage | ForEach-Object { "  {0,-40} {1,10:N0} bytes" -f $_.Name, $_.Length }

# Only this release and the four before it stay in release/ (the rest are on GitHub).
& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot "prune-releases.ps1")

if (-not $Publish) {
  Write-Host "Not published (add -Publish to upload to github.com/$repo)."
  exit 0
}
cmd /c "gh repo view $repo --json name >nul 2>&1"
if ($LASTEXITCODE -ne 0) { throw "github.com/$repo doesn't exist (or gh isn't signed in): create it as a public repository first." }
if ($AddonsOnly) {
  & gh release upload "v$version" $addons "$addons.sig" --repo $repo --clobber
} else {
  # Every Windows release ships the Linux .deb and AppImage too (tools/linux.sh release puts them here).
  if (-not (Get-ChildItem $stage -Filter "*.deb") -or -not (Get-ChildItem $stage -Filter "*.AppImage")) {
    throw "No Linux .deb/AppImage in $stage yet: run tools/linux.sh release in Ubuntu 22.04, then publish with -SkipTests -NoBuild -Publish."
  }
  # ...and lists them in latest.json, or Linux users are never offered the update.
  $listed = (Get-Content (Join-Path $stage "latest.json") -Raw | ConvertFrom-Json).platforms
  if (-not $listed."linux-x86_64-deb" -or -not $listed."linux-x86_64-appimage") {
    throw "latest.json doesn't list the Linux files: run again with -SkipTests -NoBuild -Publish (it rewrites latest.json)."
  }
  $files = Get-ChildItem $stage -Exclude "notes.txt" | ForEach-Object { $_.FullName }
  # Through cmd: Windows PowerShell stops on gh's "release not found" on stderr otherwise.
  cmd /c "gh release view v$version --repo $repo --json tagName >nul 2>&1"
  if ($LASTEXITCODE -eq 0) {
    # Already published (a second run): replace the files in place.
    & gh release upload "v$version" @files --repo $repo --clobber
  } else {
    # The notes go through a file: quotes, & and % in them reach GitHub as written.
    $notesFile = Join-Path $stage "notes.txt"
    [IO.File]::WriteAllText($notesFile, $latest.notes)
    # --latest: the app's update check reads releases/latest, which must stay the desktop release
    # (Android releases are published with --latest=false).
    & gh release create "v$version" @files --repo $repo --title "Paginaut $version" --notes-file $notesFile --latest
    Remove-Item $notesFile
  }
}
if ($LASTEXITCODE -ne 0) { throw "Publishing failed." }
Write-Host "Published: https://github.com/$repo/releases/tag/v$version" -ForegroundColor Green
