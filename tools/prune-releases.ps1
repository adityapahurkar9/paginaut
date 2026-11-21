# Keeps the newest few releases in release/ and deletes older ones, so the folder doesn't keep growing.
#   powershell -File tools/prune-releases.ps1 [-Keep 5] [-WhatIf]
# Desktop: the release/v<version> folders. Android: the release/android/Paginaut_<version>* files.
# Versions are compared as numbers (0.9.10 is newer than 0.9.9). Older releases stay on GitHub
# (release.ps1 -Publish, android.sh --publish); this only frees the local copies.
param([int]$Keep = 5, [switch]$WhatIf)

$ErrorActionPreference = "Stop"
$release = Join-Path (Split-Path $PSScriptRoot -Parent) "release"
if (-not (Test-Path $release)) { exit 0 }

function Remove-Old($items, $versionOf) {
  $byVersion = $items | Group-Object { & $versionOf $_ } | Sort-Object { [version]$_.Name } -Descending
  foreach ($group in ($byVersion | Select-Object -Skip $Keep)) {
    foreach ($item in $group.Group) {
      if ($WhatIf) { Write-Host "  would delete $($item.FullName)" }
      else { Remove-Item $item.FullName -Recurse -Force; Write-Host "  deleted $($item.Name)" }
    }
  }
}

# Desktop: v0.9.11 -> 0.9.11
$desktop = Get-ChildItem $release -Directory | Where-Object { $_.Name -match '^v\d+\.\d+\.\d+$' }
Remove-Old $desktop { param($d) $d.Name.Substring(1) }

# Android: Paginaut_0.6.7_arm64.apk, Paginaut_0.6.7.aab -> 0.6.7
$android = Join-Path $release "android"
if (Test-Path $android) {
  $files = Get-ChildItem $android -File | Where-Object { $_.Name -match '^Paginaut_\d+\.\d+\.\d+[._]' }
  Remove-Old $files { param($f) [regex]::Match($f.Name, '^Paginaut_(\d+\.\d+\.\d+)').Groups[1].Value }
}
