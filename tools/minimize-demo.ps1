# Watch-it-yourself check of Paginaut's minimize animation.
# Opens Paginaut, then minimizes it twice:
#   1) the way Paginaut's own minimize button does it,
#   2) the standard Windows way (like the taskbar / Win+Down).
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class D {
 public delegate bool CB(IntPtr h, IntPtr l);
 [DllImport("user32.dll")] public static extern bool EnumWindows(CB cb, IntPtr l);
 [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out int pid);
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 public static IntPtr Find(int pid) { IntPtr f = IntPtr.Zero; EnumWindows((h, l) => { int p; GetWindowThreadProcessId(h, out p);
   var t = new StringBuilder(64); GetWindowText(h, t, 64); if (p == pid && t.ToString() == "Paginaut") { f = h; return false; } return true; }, IntPtr.Zero); return f; } }
"@
$root = Split-Path $PSScriptRoot -Parent
Get-Process paginaut -ErrorAction SilentlyContinue | Stop-Process -Force
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9333"
$p = Start-Process (Join-Path $root "src-tauri\target\release\paginaut.exe") -PassThru
$h = [IntPtr]::Zero; for ($i = 0; $i -lt 100 -and $h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 100; $h = [D]::Find($p.Id) }
Start-Sleep -Seconds 2

Write-Host "Test 1 in 3 s: Paginaut's own minimize button. Watch the window..."
Start-Sleep -Seconds 3
node (Join-Path $root "tools\cdp.mjs") eval "window.__TAURI_INTERNALS__.invoke('plugin:window|minimize', { label: 'main' }).then(() => 'ok')" | Out-Null
Start-Sleep -Seconds 2
[D]::ShowWindow($h, 9) | Out-Null; [D]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Seconds 2

Write-Host "Test 2 in 3 s: standard Windows minimize. Watch again..."
Start-Sleep -Seconds 3
[D]::ShowWindow($h, 6) | Out-Null
Start-Sleep -Seconds 2
[D]::ShowWindow($h, 9) | Out-Null
Write-Host "Done. Did test 1 and/or test 2 animate into the taskbar?"
