# Prints private memory of Paginaut and all its child processes (WebView2 etc.).
$me = Get-Process paginaut -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $me) { "Paginaut is not running"; exit }
$all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, CommandLine
$ids = @($me.Id); $changed = $true
while ($changed) { $changed = $false; foreach ($p in $all) { if ($ids -contains $p.ParentProcessId -and -not ($ids -contains $p.ProcessId)) { $ids += $p.ProcessId; $changed = $true } } }
$rows = foreach ($id in $ids) {
  $p = Get-Process -Id $id -ErrorAction SilentlyContinue
  if (-not $p -or $p.Name -eq 'conhost') { continue }
  $cmd = ($all | Where-Object ProcessId -eq $id).CommandLine
  $type = if ($cmd -match '--type=([a-z-]+)') { $Matches[1] } elseif ($p.Name -eq 'paginaut') { 'engine' } else { 'browser' }
  [pscustomobject]@{ Process = $type; PrivateMB = [math]::Round($p.PrivateMemorySize64 / 1MB) }
}
$rows | Sort-Object PrivateMB -Descending | Format-Table -AutoSize | Out-String -Width 80
"TOTAL: $(($rows | Measure-Object PrivateMB -Sum).Sum) MB"
