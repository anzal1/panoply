# Windows desktop smoke test. Installs the release .msi, starts the installed app on the runner's
# interactive desktop, and records screenshots and facts for light and dark mode.
param([Parameter(Mandatory)][string]$Dir, [Parameter(Mandatory)][string]$Out)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path

Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class W {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  public static List<IntPtr> ForPid(uint pid) {
    var l = new List<IntPtr>();
    EnumWindows((h, _) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid) l.Add(h); return true; }, IntPtr.Zero);
    return l;
  }
}
"@
[void][W]::SetProcessDPIAware()

Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Tray {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct NID { public uint cbSize; public IntPtr hWnd; public uint uID; public Guid guidItem; }
  [DllImport("shell32.dll")] public static extern int Shell_NotifyIconGetRect(ref NID id, out RECT r);
  // The shell's rectangle for a notify icon owned by hwnd with this id, or null when it has none.
  public static int[] Rect(IntPtr hwnd, uint uid) {
    var n = new NID(); n.cbSize = (uint)Marshal.SizeOf(typeof(NID)); n.hWnd = hwnd; n.uID = uid;
    RECT r; if (Shell_NotifyIconGetRect(ref n, out r) != 0) return null;
    return new int[] { r.Left, r.Top, r.Right, r.Bottom };
  }
}
"@

Add-Type @"
using System; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class Disp {
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Ansi)]
  public struct DEVMODE {
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst=32)] public string dmDeviceName;
    public short dmSpecVersion, dmDriverVersion, dmSize, dmDriverExtra; public int dmFields;
    public int dmPositionX, dmPositionY, dmDisplayOrientation, dmDisplayFixedOutput;
    public short dmColor, dmDuplex, dmYResolution, dmTTOption, dmCollate;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst=32)] public string dmFormName;
    public short dmLogPixels; public int dmBitsPerPel, dmPelsWidth, dmPelsHeight, dmDisplayFlags, dmDisplayFrequency;
    public int dmICMMethod, dmICMIntent, dmMediaType, dmDitherType, dmReserved1, dmReserved2, dmPanningWidth, dmPanningHeight;
  }
  [DllImport("user32.dll", CharSet=CharSet.Ansi)] public static extern bool EnumDisplaySettings(string dev, int mode, ref DEVMODE dm);
  [DllImport("user32.dll", CharSet=CharSet.Ansi)] public static extern int ChangeDisplaySettings(ref DEVMODE dm, int flags);
  public static List<string> Modes() {
    var l = new List<string>(); var dm = new DEVMODE(); dm.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE));
    for (int i = 0; EnumDisplaySettings(null, i, ref dm); i++) l.Add(dm.dmPelsWidth + "x" + dm.dmPelsHeight);
    return l;
  }
  public static int Set(int w, int h) {
    var dm = new DEVMODE(); dm.dmSize = (short)Marshal.SizeOf(typeof(DEVMODE));
    EnumDisplaySettings(null, -1, ref dm); dm.dmPelsWidth = w; dm.dmPelsHeight = h; dm.dmFields = 0x80000 | 0x100000;
    return ChangeDisplaySettings(ref dm, 0);
  }
}
"@
$modes = @([Disp]::Modes() | Sort-Object -Unique)
$want = if ($env:SMOKE_RES) { $env:SMOKE_RES } else { '1920x1080' }
$resResult = $null
if ($modes -contains $want) { $wh = $want.Split('x'); $resResult = [Disp]::Set([int]$wh[0], [int]$wh[1]); Start-Sleep -Seconds 3 }
# Get the runner's own console windows out of the way of the screenshots.
(New-Object -ComObject Shell.Application).MinimizeAll(); Start-Sleep -Seconds 1

function Shot([string]$Name, $Rect) {
  $sw = [W]::GetSystemMetrics(0); $sh = [W]::GetSystemMetrics(1)
  $bmp = New-Object System.Drawing.Bitmap $sw, $sh
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen(0, 0, 0, 0, $bmp.Size)
  $g.Dispose()
  $bmp.Save((Join-Path $Out "$Name-screen.png"), [System.Drawing.Imaging.ImageFormat]::Png)
  if ($Rect -and $Rect.w -gt 0) {
    $x = [Math]::Max(0, $Rect.x); $y = [Math]::Max(0, $Rect.y)
    $w = [Math]::Min($Rect.w, $sw - $x); $h = [Math]::Min($Rect.h, $sh - $y)
    if ($w -gt 0 -and $h -gt 0) {
      $bmp.Clone((New-Object System.Drawing.Rectangle $x, $y, $w, $h), $bmp.PixelFormat).Save((Join-Path $Out "$Name-window.png"), [System.Drawing.Imaging.ImageFormat]::Png)
      $th = [Math]::Min(56, $h)
      $bmp.Clone((New-Object System.Drawing.Rectangle $x, $y, $w, $th), $bmp.PixelFormat).Save((Join-Path $Out "$Name-titlebar.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    }
  }
  $bmp.Dispose()
}

function Windows-Of([int]$ProcId) {
  $out = @()
  foreach ($h in [W]::ForPid([uint32]$ProcId)) {
    $t = New-Object System.Text.StringBuilder 256; [void][W]::GetWindowText($h, $t, 256)
    $c = New-Object System.Text.StringBuilder 256; [void][W]::GetClassName($h, $c, 256)
    $r = New-Object W+RECT; [void][W]::GetWindowRect($h, [ref]$r)
    $style = [W]::GetWindowLong($h, -16)
    $out += [pscustomobject]@{
      hwnd = $h.ToInt64(); title = $t.ToString(); class = $c.ToString(); visible = [W]::IsWindowVisible($h)
      x = $r.Left; y = $r.Top; w = $r.Right - $r.Left; h = $r.Bottom - $r.Top
      has_system_caption = (($style -band 0xC00000) -eq 0xC00000)
      dpi = [W]::GetDpiForWindow($h)
    }
  }
  $out
}

# The notify icon this process registered: its hidden message window (class tray_icon_app) and the
# rectangle the shell reports for it. Also whether the shell lists the exe under NotifyIconSettings.
function Tray-Of([int]$ProcId, [string]$Exe) {
  $win = [W]::ForPid([uint32]$ProcId) | Where-Object {
    $c = New-Object System.Text.StringBuilder 256; [void][W]::GetClassName($_, $c, 256); $c.ToString() -eq 'tray_icon_app'
  } | Select-Object -First 1
  $rect = $null; $uid = 0
  if ($win) { foreach ($i in 1..8) { $r = [Tray]::Rect($win, [uint32]$i); if ($r) { $rect = $r; $uid = $i; break } } }
  $listed = $false
  $root = 'HKCU:\Control Panel\NotifyIconSettings'
  if (Test-Path $root) {
    foreach ($k in Get-ChildItem $root) {
      $path = (Get-ItemProperty $k.PSPath -ErrorAction SilentlyContinue).ExecutablePath
      if ($path -and $path -like "*Panoply*panoply-desktop.exe") { $listed = $true }
    }
  }
  [pscustomobject]@{ hwnd = $(if ($win) { $win.ToInt64() } else { 0 }); uid = $uid; rect = $(if ($rect) { $rect -join ',' } else { $null }); settings_listed = $listed }
}

function Press-Key([byte]$vk) {
  [W]::keybd_event($vk, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60
  [W]::keybd_event($vk, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 120
}

function Press-Hotkey {
  # Ctrl+Alt+Space, as injected keyboard input (RegisterHotKey sees it like a real keypress).
  foreach ($vk in 0x11, 0x12, 0x20) { [W]::keybd_event([byte]$vk, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 40 }
  foreach ($vk in 0x20, 0x12, 0x11) { [W]::keybd_event([byte]$vk, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 40 }
}

# ---- install ---------------------------------------------------------------------------------------
$msi = (Get-ChildItem $Dir -Filter *.msi | Select-Object -First 1).FullName
$zip = (Get-ChildItem $Dir -Filter *.zip | Select-Object -First 1).FullName
Write-Host "msi: $msi"
$p = Start-Process msiexec.exe -ArgumentList "/i `"$msi`" /qn /norestart /l*v `"$Out\msi-install.log`"" -Wait -PassThru
if ($p.ExitCode -ne 0) { throw "msiexec failed with exit code $($p.ExitCode)" }
$exe = 'C:\Program Files\Panoply\panoply-desktop.exe'
if (-not (Test-Path $exe)) { throw "installed app not found at $exe" }
$installed = (Get-FileHash $exe -Algorithm SHA256).Hash
Expand-Archive $zip -DestinationPath "$Out\..\zip-extract" -Force
$zexe = Get-ChildItem "$Out\..\zip-extract" -Recurse -Filter panoply-desktop.exe | Select-Object -First 1
$zipHash = if ($zexe) { (Get-FileHash $zexe.FullName -Algorithm SHA256).Hash } else { $null }

$screen = @{ width = [W]::GetSystemMetrics(0); height = [W]::GetSystemMetrics(1) }
$report = [ordered]@{
  display_modes = $modes; resolution_change_result = $resResult
  os = (Get-CimInstance Win32_OperatingSystem).Caption; screen = $screen
  msi = (Split-Path $msi -Leaf); exe_sha256 = $installed; zip_exe_matches_msi_exe = ($installed -eq $zipHash)
  runs = @()
}
$failed = @()

foreach ($theme in 'light', 'dark') {
  $light = if ($theme -eq 'light') { 1 } else { 0 }
  $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
  New-Item -Path $key -Force | Out-Null
  Set-ItemProperty -Path $key -Name AppsUseLightTheme -Value $light -Type DWord
  Set-ItemProperty -Path $key -Name SystemUsesLightTheme -Value $light -Type DWord
  Remove-Item "$env:APPDATA\Panoply\*window*" -ErrorAction SilentlyContinue

  $run = [ordered]@{ theme = $theme; alive = $false; window_found = $false }
  $proc = Start-Process $exe -PassThru -RedirectStandardError "$Out\stderr-$theme.txt"
  Start-Sleep -Seconds 10
  $proc.Refresh()
  $run.alive = -not $proc.HasExited
  if ($proc.HasExited) { $run.exit_code_early = $proc.ExitCode; $failed += "${theme}: app exited early with code $($proc.ExitCode)" }
  else {
    $wins = @(Windows-Of $proc.Id)
    $run.windows = $wins
    $main = $wins | Where-Object { $_.visible -and $_.w -gt 300 } | Select-Object -First 1
    $run.window_found = [bool]$main
    if ($main) { $run.title = $main.title; $run.window_rect = "$($main.x),$($main.y) $($main.w)x$($main.h)" }
    else { $failed += "${theme}: no visible window" }
    Shot "windows-$theme-app" $main
    $proc.Refresh()
    $run.memory_mb = [Math]::Round($proc.WorkingSet64 / 1MB, 1)
    $run.private_mb = [Math]::Round($proc.PrivateMemorySize64 / 1MB, 1)
    $run.threads = $proc.Threads.Count
    $run.cpu_seconds = [Math]::Round($proc.TotalProcessorTime.TotalSeconds, 2)

    # The command bar, through the global hotkey.
    Press-Hotkey; Start-Sleep -Seconds 2
    $after = @(Windows-Of $proc.Id)
    $run.windows_after_hotkey = $after
    $bar = $after | Where-Object { $_.visible -and $_.w -gt 300 -and ($_.hwnd -ne $(if ($main) { $main.hwnd } else { 0 })) } | Select-Object -First 1
    $run.bar_visible = [bool]$bar
    if ($bar) {
      $sw = [W]::GetSystemMetrics(0); $sh = [W]::GetSystemMetrics(1)
      $run.bar_rect = "$($bar.x),$($bar.y) $($bar.w)x$($bar.h) on $($sw)x$($sh)"
      $run.bar_centered = ([Math]::Abs(($bar.x + $bar.w / 2) - $sw / 2) -le 4)
      $run.bar_top_ratio = [Math]::Round($bar.y / $sh, 3)
      if (-not $run.bar_centered) { $failed += "${theme}: command bar not horizontally centred ($($run.bar_rect))" }
      if ($bar.y -lt $sh * 0.1 -or $bar.y -gt $sh * 0.35) { $failed += "${theme}: command bar not in the upper third ($($run.bar_rect))" }
    } else { $failed += "${theme}: command bar did not appear on the hotkey" }
    Shot "windows-$theme-bar" $bar
    $run.alive_after_hotkey = -not $proc.HasExited
    Press-Hotkey; Start-Sleep -Seconds 1

    # Close the app window the way the caption button does. The process must stay alive, because the
    # command bar is still bound to the hotkey, and the notification-area icon must say so.
    if ($main) { [void][W]::PostMessage([IntPtr]$main.hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) }
    Start-Sleep -Seconds 4
    $proc.Refresh()
    $run.alive_after_close = -not $proc.HasExited
    if (-not $run.alive_after_close) { $failed += "${theme}: the app exited when its window closed (expected it to stay in the tray)" }
    else {
      $tray = $null
      foreach ($try in 1..10) { $tray = Tray-Of $proc.Id $exe; if ($tray.rect -or $tray.settings_listed) { break }; Start-Sleep -Seconds 1 }
      $run.tray = $tray
      $run.windows_after_close = @(Windows-Of $proc.Id | Where-Object { $_.visible })
      $run.tray_icon_registered = [bool]($tray.rect -or $tray.settings_listed)
      if (-not $run.tray_icon_registered) { $failed += "${theme}: no notification-area icon after the window closed" }
      Shot "windows-$theme-closed" $null
      # The hotkey still brings the bar back.
      Press-Hotkey; Start-Sleep -Seconds 2
      $run.bar_after_close = [bool](@(Windows-Of $proc.Id) | Where-Object { $_.visible -and $_.w -gt 300 } | Select-Object -First 1)
      Press-Hotkey; Start-Sleep -Seconds 1
      # Quit from the tray menu: right click on the icon (the message the shell sends), then the last
      # item (Up wraps to it) and Enter.
      if ($tray.hwnd) {
        [void][W]::PostMessage([IntPtr]$tray.hwnd, 6002, [IntPtr]$tray.uid, [IntPtr]0x0205)
        Start-Sleep -Seconds 2
        Shot "windows-$theme-traymenu" $null
        Press-Key 0x26; Press-Key 0x0D
      }
      $run.exited_after_tray_quit = $proc.WaitForExit(15000)
      if ($run.exited_after_tray_quit) { $run.exit_code = $proc.ExitCode; if ($run.exit_code -ne 0) { $failed += "${theme}: exit code $($run.exit_code) after tray Quit" } }
      else {
        $failed += "${theme}: tray Quit did not end the process"
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
      }
    }
  }
  if (Test-Path "$Out\stderr-$theme.txt") { $run.stderr = (Get-Content "$Out\stderr-$theme.txt" -Raw) }
  $report.runs += [pscustomobject]$run
  Start-Sleep -Seconds 2
}

$report.failed = $failed
$report | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 "$Out\report.json"
Get-Content "$Out\report.json"
if ($failed.Count) { Write-Host "::error::$($failed -join '; ')"; exit 1 }
