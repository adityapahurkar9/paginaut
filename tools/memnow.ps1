# One-shot memory reading of Paginaut's whole process tree: "<private MB> <working set MB>".
$all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name
$root = $all | Where-Object { $_.Name -eq "paginaut.exe" } | Select-Object -First 1
if (-not $root) { "0 0"; exit }
$ids = @($root.ProcessId); $changed = $true
while ($changed) { $changed = $false; foreach ($p in $all) { if ($ids -contains $p.ParentProcessId -and -not ($ids -contains $p.ProcessId)) { $ids += $p.ProcessId; $changed = $true } } }
$priv = 0; $ws = 0
foreach ($id in $ids) { $p = Get-Process -Id $id -ErrorAction SilentlyContinue; if ($p) { $priv += $p.PrivateMemorySize64; $ws += $p.WorkingSet64 } }
"{0} {1}" -f [math]::Round($priv / 1MB), [math]::Round($ws / 1MB)
