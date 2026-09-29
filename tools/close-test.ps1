# Checks that Folio exits via (1) its close button path and (2) Alt+F4 (WM_SYSCOMMAND SC_CLOSE).
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class C { public delegate bool CB(IntPtr h, IntPtr l);
 [DllImport("user32.dll")] public static extern bool EnumWindows(CB cb, IntPtr l);
 [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out int pid);
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
 [DllImport("user32.dll")] public static extern IntPtr PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
 public static IntPtr Find(int pid) { IntPtr f = IntPtr.Zero; EnumWindows((h, l) => { int p; GetWindowThreadProcessId(h, out p);
   var t = new StringBuilder(64); GetWindowText(h, t, 64); if (p == pid && t.ToString() == "Folio") { f = h; return false; } return true; }, IntPtr.Zero); return f; } }
"@
$root = Split-Path $PSScriptRoot -Parent
$exe = Join-Path $root "src-tauri\target\release\folio.exe"
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9333"
foreach ($how in "button", "alt+f4") {
  Get-Process folio -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Milliseconds 300
  $p = Start-Process $exe -PassThru
  $h = [IntPtr]::Zero; for ($i = 0; $i -lt 100 -and $h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 100; $h = [C]::Find($p.Id) }
  Start-Sleep -Seconds 2
  if ($how -eq "button") { node (Join-Path $root "tools\cdp.mjs") eval "document.querySelector('.winbtn.close').click(), 'clicked'" | Out-Null }
  else { [C]::PostMessage($h, 0x0112, [IntPtr]0xF060, [IntPtr]0) | Out-Null }
  $exited = $p.WaitForExit(4000)
  "{0,-8} -> {1}" -f $how, $(if ($exited) { "closed" } else { "STILL RUNNING" })
}
Get-Process folio -ErrorAction SilentlyContinue | Stop-Process -Force
