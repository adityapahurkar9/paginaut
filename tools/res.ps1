# Resources an app uses, summed over its process tree (WebView2 or CEF helpers included):
# private memory, working set, CPU time and handles. Usage:
#   powershell -File tools/res.ps1 -Name paginaut        (or Acrobat, AcroRd32...)
#   -Json for one line of JSON.
param([string]$Name = "paginaut", [switch]$Json)
$roots = @(Get-Process $Name -ErrorAction SilentlyContinue)
if (-not $roots) { if ($Json) { '{"running":false}' } else { "$Name is not running" }; exit }
$all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name
$ids = @($roots.Id); $changed = $true
while ($changed) {
  $changed = $false
  foreach ($p in $all) { if ($ids -contains $p.ParentProcessId -and -not ($ids -contains $p.ProcessId)) { $ids += $p.ProcessId; $changed = $true } }
}
$priv = 0; $ws = 0; $cpu = 0.0; $handles = 0; $n = 0
foreach ($id in $ids) {
  $p = Get-Process -Id $id -ErrorAction SilentlyContinue
  if (-not $p -or $p.Name -eq 'conhost') { continue }
  $n++; $priv += $p.PrivateMemorySize64; $ws += $p.WorkingSet64; $handles += $p.HandleCount
  if ($p.TotalProcessorTime) { $cpu += $p.TotalProcessorTime.TotalSeconds }
}
$o = [ordered]@{ processes = $n; privateMB = [math]::Round($priv / 1MB); workingSetMB = [math]::Round($ws / 1MB); cpuSeconds = [math]::Round($cpu, 2); handles = $handles }
if ($Json) { $o | ConvertTo-Json -Compress } else { [pscustomobject]$o | Format-List }
