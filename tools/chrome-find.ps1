# Benchmark helper: opens a PDF in Chrome (throwaway profile), then types a search into
# Chrome's find bar with simulated keystrokes. Leaves Chrome running for measurement.
param([string]$File, [string]$Query = "Company", [switch]$OpenOnly, [switch]$FindOnly)
$prof = "$env:TEMP\paginaut-chrome-bench"
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class W { [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c); }
"@
Add-Type -AssemblyName System.Windows.Forms

if (-not $FindOnly) {
  Remove-Item -Recurse -Force $prof -ErrorAction SilentlyContinue
  $url = "file:///" + ($File -replace '\\', '/' -replace ' ', '%20')
  Start-Process "C:\Program Files\Google\Chrome\Application\chrome.exe" -ArgumentList "--user-data-dir=`"$prof`"", "--no-first-run", "--no-default-browser-check", "--window-size=1280,860", "`"$url`""
  "opened"
}
if (-not $OpenOnly) {
  $win = Get-CimInstance Win32_Process -Filter "Name='chrome.exe'" | Where-Object { $_.CommandLine -like "*paginaut-chrome-bench*" } |
    ForEach-Object { Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue } | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
  if (-not $win) { "no chrome window"; exit 1 }
  [W]::ShowWindow($win.MainWindowHandle, 9) | Out-Null
  [W]::SetForegroundWindow($win.MainWindowHandle) | Out-Null
  Start-Sleep -Milliseconds 500
  [System.Windows.Forms.SendKeys]::SendWait("^f")
  Start-Sleep -Milliseconds 400
  [System.Windows.Forms.SendKeys]::SendWait($Query)
  [System.Windows.Forms.SendKeys]::SendWait("{ENTER}")
  "search typed"
}
