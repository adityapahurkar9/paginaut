# Samples total private memory of an app's whole process tree over time.
#   powershell -File tools/memwatch.ps1 -App paginaut  -Seconds 60 [-Interval 0.5] [-Out file.csv]
#   powershell -File tools/memwatch.ps1 -App chrome -Match paginaut-chrome-bench -Seconds 60
param([string]$App = "paginaut", [string]$Match = "", [double]$Seconds = 60, [double]$Interval = 0.5, [string]$Out = "")

function Get-TreeIds {
  $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CommandLine
  if ($App -eq "chrome") {
    return @($all | Where-Object { $_.Name -eq "chrome.exe" -and $_.CommandLine -like "*$Match*" } | ForEach-Object ProcessId)
  }
  $root = $all | Where-Object { $_.Name -eq "$App.exe" } | Select-Object -First 1
  if (-not $root) { return @() }
  $ids = @($root.ProcessId); $changed = $true
  while ($changed) {
    $changed = $false
    foreach ($p in $all) { if ($ids -contains $p.ParentProcessId -and -not ($ids -contains $p.ProcessId)) { $ids += $p.ProcessId; $changed = $true } }
  }
  return $ids
}

$ids = Get-TreeIds
$sw = [Diagnostics.Stopwatch]::StartNew()
$rows = @()
while ($sw.Elapsed.TotalSeconds -lt $Seconds) {
  if ($sw.Elapsed.TotalSeconds % 5 -lt $Interval) { $ids = Get-TreeIds }   # pick up new child processes
  $mb = 0
  foreach ($id in $ids) { $p = Get-Process -Id $id -ErrorAction SilentlyContinue; if ($p) { $mb += $p.PrivateMemorySize64 } }
  $rows += [pscustomobject]@{ t = [math]::Round($sw.Elapsed.TotalSeconds, 1); MB = [math]::Round($mb / 1MB) }
  Start-Sleep -Milliseconds ([int]($Interval * 1000))
}
if ($Out) { $rows | Export-Csv -NoTypeInformation $Out }
$rows | ForEach-Object { "$($_.t)`t$($_.MB)" }
