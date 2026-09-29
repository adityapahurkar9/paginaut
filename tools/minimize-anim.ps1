# Checks whether minimizing a window animates: captures the window's screen area
# rapidly while minimizing and counts frames that are neither "window" nor "desktop".
#   powershell -File tools/minimize-anim.ps1 -Exe <path> [-Title Paginaut] [-Via showwindow|syscommand]
param([string]$Exe, [string]$Title = "", [string]$Via = "showwindow")
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class M {
 public delegate bool CB(IntPtr h, IntPtr l);
 [DllImport("user32.dll")] public static extern bool EnumWindows(CB cb, IntPtr l);
 [DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out int pid);
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern IntPtr PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 public static IntPtr Find(int pid, string title) { IntPtr found = IntPtr.Zero;
  EnumWindows((h, l) => { int p; GetWindowThreadProcessId(h, out p); var t = new StringBuilder(256); GetWindowText(h, t, 256);
   if (p == pid && IsWindowVisible(h) && (title == "" || t.ToString() == title) && t.Length > 0) { found = h; return false; } return true; }, IntPtr.Zero); return found; } }
"@
$p = Start-Process $Exe -PassThru
$h = [IntPtr]::Zero
for ($i = 0; $i -lt 100 -and $h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 100; $h = [M]::Find($p.Id, $Title) }
if ($h -eq [IntPtr]::Zero) { "window not found"; exit 1 }
[M]::ShowWindow($h, 9) | Out-Null; [M]::SetForegroundWindow($h) | Out-Null
Start-Sleep -Seconds 2
$r = New-Object M+RECT; [M]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.R - $r.L; $ht = $r.B - $r.T
$bmp = New-Object System.Drawing.Bitmap $w, $ht
$g = [System.Drawing.Graphics]::FromImage($bmp)
$sig = { param($b) $s = 0; for ($y = 20; $y -lt $b.Height; $y += 40) { for ($x = 20; $x -lt $b.Width; $x += 40) { $c = $b.GetPixel($x, $y); $s = $s * 31 + $c.R + 7 * $c.G + 13 * $c.B } }; $s }
$g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size); $before = & $sig $bmp
if ($Via -eq "syscommand") { [M]::PostMessage($h, 0x0112, [IntPtr]0xF020, [IntPtr]0) | Out-Null } else { [M]::ShowWindow($h, 6) | Out-Null }
$sigs = @(); $sw = [Diagnostics.Stopwatch]::StartNew()
while ($sw.ElapsedMilliseconds -lt 700) { $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size); $sigs += (& $sig $bmp) }
$after = $sigs[-1]
$between = @($sigs | Where-Object { $_ -ne $before -and $_ -ne $after } | Select-Object -Unique).Count
"frames captured: $($sigs.Count), distinct in-between frames: $between  (0 = no animation)"
Stop-Process -Id $p.Id -Force
