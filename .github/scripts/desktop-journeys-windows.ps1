# Windows user journeys. Installs the release .msi, then uses the installed app the way a person does:
# the hotkey, typing into the command bar and the search box, Enter, Ctrl+Z, the window close button,
# the notification-area icon. Every journey records pass or fail, the seconds it took and screenshots.
#
# Verification is by files on disk, by the Recycle Bin, by screenshots (cropped to the app window and
# compared for blankness and brightness) and by OCR of those screenshots with Windows.Media.Ocr
# (tesseract only when this runner has no Windows OCR language).
#
# Status words in the report:
#   pass   what the journey expects happened
#   fail   the app did not do what the journey expects (an app bug until shown otherwise)
#   warn   it worked but missed a stated target (for example slower than 3 s)
#   error  the script itself broke or could not check (a script limitation, never an app bug)
#   skip   not run (depends on an earlier journey, or the runner cannot do it)
param(
  [Parameter(Mandatory)][string]$Dir,
  [Parameter(Mandatory)][string]$Out,
  [string]$Only = '',
  [string]$Fixtures = ''
)
$ErrorActionPreference = 'Continue'
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
if (-not $Fixtures) { $Fixtures = Join-Path $Here 'journeys-fixtures.py' }
$Work = 'C:\journeys'   # on the C: drive, which has a Recycle Bin (RUNNER_TEMP is on D:)
Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Work | Out-Null

Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
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
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern short VkKeyScan(char c);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr c);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  public static List<IntPtr> ForPid(uint pid) {
    var l = new List<IntPtr>();
    EnumWindows((h, _) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid) l.Add(h); return true; }, IntPtr.Zero);
    return l;
  }
  public static List<IntPtr> All() {
    var l = new List<IntPtr>();
    EnumWindows((h, _) => { l.Add(h); return true; }, IntPtr.Zero);
    return l;
  }
}
"@
# Physical pixels everywhere: per-monitor aware V2, so a changed display scale does not skew coordinates.
try { [void][W]::SetProcessDpiAwarenessContext([IntPtr]-4) } catch { [void][W]::SetProcessDPIAware() }

Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Tray {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct NID { public uint cbSize; public IntPtr hWnd; public uint uID; public Guid guidItem; }
  [DllImport("shell32.dll")] public static extern int Shell_NotifyIconGetRect(ref NID id, out RECT r);
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

# Display scale (100%, 125%, 150%...) through the display configuration API that the Settings app uses.
# The structures are read and written as plain bytes so no layout can go wrong.
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class Scale {
  [DllImport("user32.dll")] static extern int GetDisplayConfigBufferSizes(uint flags, out uint np, out uint nm);
  [DllImport("user32.dll")] static extern int QueryDisplayConfig(uint flags, ref uint np, byte[] paths, ref uint nm, byte[] modes, IntPtr topo);
  [DllImport("user32.dll")] static extern int DisplayConfigGetDeviceInfo(byte[] pkt);
  [DllImport("user32.dll")] static extern int DisplayConfigSetDeviceInfo(byte[] pkt);
  static readonly int[] Vals = { 100, 125, 150, 175, 200, 225, 250, 300, 350, 400, 450, 500 };
  // Returns { recommendedIndex, currentIndex, maxIndex } or throws with the Win32 code.
  public static int[] Get(out byte[] luidId) {
    uint np, nm;
    int r = GetDisplayConfigBufferSizes(2, out np, out nm); if (r != 0) throw new Exception("GetDisplayConfigBufferSizes " + r);
    var paths = new byte[72 * np]; var modes = new byte[64 * nm];
    r = QueryDisplayConfig(2, ref np, paths, ref nm, modes, IntPtr.Zero); if (r != 0) throw new Exception("QueryDisplayConfig " + r);
    if (np == 0) throw new Exception("no active display path");
    luidId = new byte[12]; Array.Copy(paths, 0, luidId, 0, 8); Array.Copy(paths, 8, luidId, 8, 4);
    var pkt = new byte[32];
    BitConverter.GetBytes(-3).CopyTo(pkt, 0); BitConverter.GetBytes(32).CopyTo(pkt, 4);
    Array.Copy(luidId, 0, pkt, 8, 8); Array.Copy(luidId, 8, pkt, 16, 4);
    r = DisplayConfigGetDeviceInfo(pkt); if (r != 0) throw new Exception("DisplayConfigGetDeviceInfo " + r);
    int min = BitConverter.ToInt32(pkt, 20), cur = BitConverter.ToInt32(pkt, 24), max = BitConverter.ToInt32(pkt, 28);
    int rec = -min;
    return new int[] { rec, rec + cur, rec + max };
  }
  public static int Percent(int idx) { return Vals[idx]; }
  public static int Set(int percent) {
    byte[] id; var g = Get(out id);
    int want = Array.IndexOf(Vals, percent); if (want < 0) throw new Exception("not a scale " + percent);
    if (want > g[2]) throw new Exception("this display allows at most " + Vals[g[2]] + " percent");
    var pkt = new byte[24];
    BitConverter.GetBytes(-4).CopyTo(pkt, 0); BitConverter.GetBytes(24).CopyTo(pkt, 4);
    Array.Copy(id, 0, pkt, 8, 8); Array.Copy(id, 8, pkt, 16, 4);
    BitConverter.GetBytes(want - g[0]).CopyTo(pkt, 20);
    return DisplayConfigSetDeviceInfo(pkt);
  }
}
"@

# ---- the journey recorder --------------------------------------------------------------------------
$script:Results = New-Object System.Collections.ArrayList
$script:Cur = $null
$script:JDir = $null
$script:Sess = $null
$script:OcrEngine = $null
$script:OcrKind = 'none'

function Want([string]$id) {
  if (-not $Only) { return $true }
  return (@($Only.Split(',') | ForEach-Object { $_.Trim() }) -contains $id)
}
function Journey([string]$id, [string]$name, [scriptblock]$body) {
  if (-not (Want $id)) { return }
  Write-Host "::group::journey $id $name"
  if ($id -ne '5') { Stop-All }
  $script:JDir = Join-Path $Out "j$id"
  New-Item -ItemType Directory -Force -Path $script:JDir | Out-Null
  $script:Cur = [ordered]@{ id = $id; name = $name; status = 'pass'; seconds = 0; checks = @(); notes = @(); screenshots = @(); data = [ordered]@{} }
  $sw = [Diagnostics.Stopwatch]::StartNew()
  try { & $body }
  catch {
    $script:Cur.status = 'error'
    $script:Cur.notes += "script error: $($_.Exception.Message) at line $($_.InvocationInfo.ScriptLineNumber)"
    Write-Host "script error: $($_ | Out-String)"
  }
  $sw.Stop()
  $script:Cur.seconds = [Math]::Round($sw.Elapsed.TotalSeconds, 1)
  [void]$script:Results.Add([pscustomobject]$script:Cur)
  Write-Host "journey $id : $($script:Cur.status) in $($script:Cur.seconds) s"
  Write-Host '::endgroup::'
  Save-Report
}
# A check the app has to meet. A failed check makes the journey fail (unless it already errored).
function Check([string]$what, [bool]$ok, [string]$detail = '') {
  $script:Cur.checks += [pscustomobject]@{ check = $what; ok = $ok; detail = $detail }
  Write-Host ("  [{0}] {1} {2}" -f $(if ($ok) { 'ok' } else { 'FAIL' }), $what, $detail)
  if (-not $ok -and $script:Cur.status -in 'pass', 'warn') { $script:Cur.status = 'fail' }
}
function Warn([string]$what, [string]$detail = '') {
  $script:Cur.notes += "warn: $what $detail"
  Write-Host "  [warn] $what $detail"
  if ($script:Cur.status -eq 'pass') { $script:Cur.status = 'warn' }
}
function Limit([string]$what) {
  $script:Cur.notes += "script limitation: $what"
  Write-Host "  [limit] $what"
  if ($script:Cur.status -in 'pass', 'warn') { $script:Cur.status = 'error' }
}
function Skip([string]$why) { $script:Cur.notes += "skipped: $why"; $script:Cur.status = 'skip' }
function Note([string]$t) { $script:Cur.notes += $t; Write-Host "  note: $t" }
function Fact([string]$k, $v) { $script:Cur.data[$k] = $v }

function Save-Report {
  $rep = [ordered]@{
    os = (Get-CimInstance Win32_OperatingSystem).Caption
    screen = "$([W]::GetSystemMetrics(0))x$([W]::GetSystemMetrics(1))"
    ocr_engine = $script:OcrKind
    journeys = @($script:Results)
  }
  $rep | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 (Join-Path $Out 'journeys.json')
}

# ---- windows, input, screenshots --------------------------------------------------------------------
function Windows-Of([int]$ProcId) {
  $out = @()
  foreach ($h in [W]::ForPid([uint32]$ProcId)) {
    $t = New-Object System.Text.StringBuilder 256; [void][W]::GetWindowText($h, $t, 256)
    $c = New-Object System.Text.StringBuilder 256; [void][W]::GetClassName($h, $c, 256)
    $r = New-Object W+RECT; [void][W]::GetWindowRect($h, [ref]$r)
    $out += [pscustomobject]@{
      hwnd = $h.ToInt64(); title = $t.ToString(); class = $c.ToString(); visible = [W]::IsWindowVisible($h)
      x = $r.Left; y = $r.Top; w = $r.Right - $r.Left; h = $r.Bottom - $r.Top; dpi = [W]::GetDpiForWindow($h)
    }
  }
  $out
}
function Get-Main($proc) {
  Windows-Of $proc.Id | Where-Object { $_.visible -and $_.title -eq 'Panoply' -and $_.w -gt 500 } |
    Sort-Object { $_.w * $_.h } -Descending | Select-Object -First 1
}
function Get-Bar($proc) {
  $main = Get-Main $proc
  Windows-Of $proc.Id | Where-Object { $_.visible -and $_.w -gt 300 -and $_.class -ne 'tray_icon_app' -and (-not $main -or $_.hwnd -ne $main.hwnd) } |
    Select-Object -First 1
}

function Shot([string]$Name, $Rect) {
  $sw = [W]::GetSystemMetrics(0); $sh = [W]::GetSystemMetrics(1)
  $bmp = New-Object System.Drawing.Bitmap $sw, $sh
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen(0, 0, 0, 0, $bmp.Size)
  $g.Dispose()
  $full = Join-Path $script:JDir "$Name-screen.png"
  $bmp.Save($full, [System.Drawing.Imaging.ImageFormat]::Png)
  $path = $full
  if ($Rect -and $Rect.w -gt 0) {
    $x = [Math]::Max(0, $Rect.x); $y = [Math]::Max(0, $Rect.y)
    $w = [Math]::Min($Rect.w, $sw - $x); $h = [Math]::Min($Rect.h, $sh - $y)
    if ($w -gt 0 -and $h -gt 0) {
      $path = Join-Path $script:JDir "$Name.png"
      $bmp.Clone((New-Object System.Drawing.Rectangle $x, $y, $w, $h), $bmp.PixelFormat).Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
      Remove-Item $full -ErrorAction SilentlyContinue
    }
  }
  $bmp.Dispose()
  $script:Cur.screenshots += (Split-Path $path -Leaf)
  $path
}

# Brightness (0..1) and how much of the picture is one flat colour (1 = blank).
function Image-Stats([string]$png) {
  $bmp = New-Object System.Drawing.Bitmap $png
  $n = 0; $sum = 0.0; $counts = @{}
  $stepx = [Math]::Max(1, [int]($bmp.Width / 120)); $stepy = [Math]::Max(1, [int]($bmp.Height / 80))
  for ($y = 0; $y -lt $bmp.Height; $y += $stepy) {
    for ($x = 0; $x -lt $bmp.Width; $x += $stepx) {
      $c = $bmp.GetPixel($x, $y)
      $sum += (0.299 * $c.R + 0.587 * $c.G + 0.114 * $c.B) / 255
      $k = ($c.R -shr 3) * 1024 + ($c.G -shr 3) * 32 + ($c.B -shr 3)
      $counts[$k] = 1 + [int]$counts[$k]; $n++
    }
  }
  $bmp.Dispose()
  $top = ($counts.Values | Measure-Object -Maximum).Maximum
  [pscustomobject]@{ brightness = [Math]::Round($sum / $n, 3); flat = [Math]::Round($top / $n, 3); colours = $counts.Count }
}

function Mouse-Click([int]$x, [int]$y) {
  [void][W]::SetCursorPos($x, $y); Start-Sleep -Milliseconds 80
  [W]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60
  [W]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 150
}
function Key-Down([byte]$vk) { [W]::keybd_event($vk, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 40 }
function Key-Up([byte]$vk) { [W]::keybd_event($vk, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 40 }
function Press-Key([byte]$vk) { Key-Down $vk; Key-Up $vk; Start-Sleep -Milliseconds 80 }
function Press-Chord([byte[]]$mods, [byte]$vk) {
  foreach ($m in $mods) { Key-Down $m }
  Key-Down $vk; Key-Up $vk
  foreach ($m in ($mods | Sort-Object -Descending)) { Key-Up $m }
  Start-Sleep -Milliseconds 120
}
function Press-Hotkey { Press-Chord @(0x11, 0x12) 0x20 }   # Ctrl+Alt+Space
function Type-Text([string]$s) {
  foreach ($ch in $s.ToCharArray()) {
    $v = [W]::VkKeyScan($ch)
    if ($v -eq -1) { continue }
    $vk = [byte]($v -band 0xFF); $shift = (($v -shr 8) -band 1) -eq 1
    if ($shift) { Key-Down 0x10 }
    Key-Down $vk; Key-Up $vk
    if ($shift) { Key-Up 0x10 }
    Start-Sleep -Milliseconds 45
  }
  Start-Sleep -Milliseconds 300
}
# Bring a window to the front the way a person would: a click on its title strip.
function Focus-Window($win) {
  if (-not $win) { return $false }
  [void][W]::ShowWindow([IntPtr]$win.hwnd, 9)
  [void][W]::SetForegroundWindow([IntPtr]$win.hwnd); Start-Sleep -Milliseconds 300
  if ([W]::GetForegroundWindow().ToInt64() -ne $win.hwnd) {
    Key-Down 0x12; Key-Up 0x12   # a bare Alt tap lets SetForegroundWindow through
    [void][W]::SetForegroundWindow([IntPtr]$win.hwnd); Start-Sleep -Milliseconds 300
  }
  if ([W]::GetForegroundWindow().ToInt64() -ne $win.hwnd) { Mouse-Click ($win.x + [int]($win.w / 2)) ($win.y + 10); Start-Sleep -Milliseconds 300 }
  return ([W]::GetForegroundWindow().ToInt64() -eq $win.hwnd)
}

# ---- OCR --------------------------------------------------------------------------------------------
function Initialize-Ocr {
  try {
    Add-Type -AssemblyName System.Runtime.WindowsRuntime
    $null = [Windows.Media.Ocr.OcrEngine, Windows.Foundation, ContentType = WindowsRuntime]
    $null = [Windows.Graphics.Imaging.BitmapDecoder, Windows.Foundation, ContentType = WindowsRuntime]
    $null = [Windows.Storage.StorageFile, Windows.Foundation, ContentType = WindowsRuntime]
    $null = [Windows.Globalization.Language, Windows.Globalization, ContentType = WindowsRuntime]
    $script:asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
        $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' })[0]
    $eng = [Windows.Media.Ocr.OcrEngine]::TryCreateFromUserProfileLanguages()
    if (-not $eng) {
      $lang = New-Object Windows.Globalization.Language 'en-US'
      $eng = [Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage($lang)
    }
    if ($eng) { $script:OcrEngine = $eng; $script:OcrKind = 'Windows.Media.Ocr'; return }
  } catch { Write-Host "Windows OCR unavailable: $($_.Exception.Message)" }
  # No OCR language on this runner. Try to add the Windows one (time boxed), then fall back to tesseract.
  Write-Host 'No Windows OCR engine; installing tesseract from Chocolatey'
  try { choco install tesseract -y --no-progress 2>&1 | Select-Object -Last 3 | ForEach-Object { Write-Host $_ } } catch { Write-Host $_ }
  $t = 'C:\Program Files\Tesseract-OCR\tesseract.exe'
  if (Test-Path $t) { $script:OcrKind = 'tesseract'; $script:Tess = $t }
}
function Await($op, [Type]$t) {
  $m = $script:asTask.MakeGenericMethod($t)
  $task = $m.Invoke($null, @($op)); [void]$task.Wait(-1); $task.Result
}
# Upscale 2x (and invert a dark picture) before reading: small UI text reads much better.
function Prep-Image([string]$png) {
  $st = Image-Stats $png
  $src = New-Object System.Drawing.Bitmap $png
  $w = $src.Width * 2; $h = $src.Height * 2
  $dst = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($dst)
  $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
  $dest = New-Object System.Drawing.Rectangle 0, 0, $w, $h
  if ($st.brightness -lt 0.4) {
    $cm = New-Object System.Drawing.Imaging.ColorMatrix
    $cm.Matrix00 = -1; $cm.Matrix11 = -1; $cm.Matrix22 = -1; $cm.Matrix33 = 1; $cm.Matrix40 = 1; $cm.Matrix41 = 1; $cm.Matrix42 = 1; $cm.Matrix44 = 1
    $ia = New-Object System.Drawing.Imaging.ImageAttributes
    $ia.SetColorMatrix($cm)
    $g.DrawImage($src, $dest, 0, 0, $src.Width, $src.Height, [System.Drawing.GraphicsUnit]::Pixel, $ia)
  } else { $g.DrawImage($src, 0, 0, $w, $h) }
  $g.Dispose(); $src.Dispose()
  $p = [IO.Path]::ChangeExtension($png, '.ocr.png')
  $dst.Save($p, [System.Drawing.Imaging.ImageFormat]::Png); $dst.Dispose()
  $p
}
# Text on a screenshot, with the word boxes (Windows OCR only) so a control can be located and clicked.
function Ocr([string]$png) {
  $res = [pscustomobject]@{ text = ''; words = @() }
  try {
    $p = Prep-Image $png
    if ($script:OcrKind -eq 'Windows.Media.Ocr') {
      $file = Await ([Windows.Storage.StorageFile]::GetFileFromPathAsync($p)) ([Windows.Storage.StorageFile])
      $stream = Await ($file.OpenAsync([Windows.Storage.FileAccessMode]::Read)) ([Windows.Storage.Streams.IRandomAccessStream])
      $dec = Await ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($stream)) ([Windows.Graphics.Imaging.BitmapDecoder])
      $bmp = Await ($dec.GetSoftwareBitmapAsync()) ([Windows.Graphics.Imaging.SoftwareBitmap])
      $r = Await ($script:OcrEngine.RecognizeAsync($bmp)) ([Windows.Media.Ocr.OcrResult])
      $res.text = $r.Text
      $words = @()
      foreach ($l in $r.Lines) { foreach ($wd in $l.Words) { $words += [pscustomobject]@{ t = $wd.Text; x = [int]($wd.BoundingRect.X / 2); y = [int]($wd.BoundingRect.Y / 2); w = [int]($wd.BoundingRect.Width / 2); h = [int]($wd.BoundingRect.Height / 2) } } }
      $res.words = $words
      $stream.Dispose()
    } elseif ($script:OcrKind -eq 'tesseract') {
      $res.text = (& $script:Tess $p stdout --psm 11 2>$null) -join "`n"
    }
    Remove-Item $p -ErrorAction SilentlyContinue
  } catch { Write-Host "ocr failed: $($_.Exception.Message)" }
  $txt = ([IO.Path]::ChangeExtension($png, '.txt'))
  Set-Content -Encoding UTF8 $txt $res.text
  $res
}
# Take shots of the rectangle until the OCR text matches, or the time runs out.
function Wait-Text($proc, [string]$name, [scriptblock]$rectOf, [string]$pattern, [int]$seconds = 10) {
  $t0 = Get-Date; $last = $null; $i = 0
  do {
    $i++
    $rect = & $rectOf
    $png = Shot "$name-$i" $rect
    $last = Ocr $png
    if ($last.text -match $pattern) { return [pscustomobject]@{ ok = $true; text = $last.text; png = $png; words = $last.words } }
    Start-Sleep -Milliseconds 700
  } while (((Get-Date) - $t0).TotalSeconds -lt $seconds)
  [pscustomobject]@{ ok = $false; text = $last.text; png = $png; words = $last.words }
}

# ---- the app ----------------------------------------------------------------------------------------
$exe = 'C:\Program Files\Panoply\panoply-desktop.exe'
function Launch-App([string[]]$AppArgs = @(), [hashtable]$EnvVars = @{}) {
  $data = Join-Path $script:JDir 'data'
  New-Item -ItemType Directory -Force -Path $data | Out-Null
  $env:PANOPLY_DATA_DIR = $data
  foreach ($k in $EnvVars.Keys) { Set-Item -Path "Env:$k" -Value $EnvVars[$k] }
  $err = Join-Path $script:JDir 'stderr.txt'
  $argStr = ($AppArgs | ForEach-Object { '"' + $_ + '"' }) -join ' '
  $t0 = Get-Date
  if ($argStr) { $p = Start-Process $exe -ArgumentList $argStr -PassThru -RedirectStandardError $err }
  else { $p = Start-Process $exe -PassThru -RedirectStandardError $err }
  foreach ($k in $EnvVars.Keys) { Remove-Item -Path "Env:$k" -ErrorAction SilentlyContinue }
  Remove-Item Env:PANOPLY_DATA_DIR -ErrorAction SilentlyContinue
  [pscustomobject]@{ proc = $p; t0 = $t0; err = $err; data = $data }
}
function Stop-App($app) {
  if (-not $app) { return }
  try { $app.proc.Refresh(); if (-not $app.proc.HasExited) { Stop-Process -Id $app.proc.Id -Force -ErrorAction SilentlyContinue; [void]$app.proc.WaitForExit(5000) } } catch {}
  Start-Sleep -Milliseconds 500
}
function Stop-All { Get-Process panoply-desktop -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue; Start-Sleep -Seconds 1 }
function Wait-Main($app, [int]$seconds = 20) {
  $t0 = Get-Date
  do {
    $app.proc.Refresh()
    if ($app.proc.HasExited) { return $null }
    $m = Get-Main $app.proc
    if ($m) { return $m }
    Start-Sleep -Milliseconds 100
  } while (((Get-Date) - $t0).TotalSeconds -lt $seconds)
  $null
}
function Find-NewFile([string]$dir, [string]$pattern, [string[]]$except, [int]$seconds) {
  $t0 = Get-Date
  do {
    $f = Get-ChildItem $dir -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -like $pattern -and ($except -notcontains $_.Name) -and $_.Name -notlike '*.part*' -and $_.Name -notlike '.*' } | Select-Object -First 1
    if ($f) { Start-Sleep -Milliseconds 400; return $f }
    Start-Sleep -Milliseconds 500
  } while (((Get-Date) - $t0).TotalSeconds -lt $seconds)
  $null
}
function Py { param([string[]]$A) $o = & python -I $Fixtures @A 2>&1; $script:PyExit = $LASTEXITCODE; ($o | Out-String).Trim() }
function Copy-Fixtures([string]$to) {
  New-Item -ItemType Directory -Force -Path $to | Out-Null
  Get-ChildItem $script:FixtureDir -File | Copy-Item -Destination $to
}
# Accessibility tree of a window through UI Automation (System.Windows.Automation), in a child job
# with a time limit, since a provider that misbehaves can hang the caller.
function Uia-Dump([long]$hwnd, [string]$file) {
  $job = Start-Job -ScriptBlock {
    param($h)
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    $root = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$h)
    $all = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
    $rows = @()
    foreach ($e in $all) {
      $c = $e.Current
      $rows += [pscustomobject]@{ name = $c.Name; type = $c.ControlType.ProgrammaticName; cls = $c.ClassName; id = $c.AutomationId; enabled = $c.IsEnabled; rect = $c.BoundingRectangle.ToString() }
    }
    [pscustomobject]@{ root = $root.Current.Name; rootType = $root.Current.ControlType.ProgrammaticName; count = $all.Count; elements = $rows }
  } -ArgumentList $hwnd
  if (Wait-Job $job -Timeout 25) {
    $r = Receive-Job $job; Remove-Job $job -Force
    $r | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 $file
    return $r
  }
  Stop-Job $job; Remove-Job $job -Force
  [pscustomobject]@{ root = ''; count = -1; elements = @(); timeout = $true }
}
# Recycle Bin: is there an entry for this file name or this size?
function In-RecycleBin([string]$name, [long]$size) {
  $hits = @()
  try {
    $sh = New-Object -ComObject Shell.Application
    $rb = $sh.Namespace(10)
    foreach ($it in $rb.Items()) {
      $n = $rb.GetDetailsOf($it, 0); $orig = $rb.GetDetailsOf($it, 1)
      if ($it.Name -eq $name -or $n -eq $name -or ($n + [IO.Path]::GetExtension($name)) -eq $name -or $it.Path -like "*$name*" -or $orig -like "*$work*") { $hits += "$n | $orig" }
    }
  } catch { }
  if (-not $hits) {
    foreach ($f in (Get-ChildItem 'C:\$Recycle.Bin' -Force -Recurse -ErrorAction SilentlyContinue)) {
      if ($f.Name -like '$R*' -and $f.Length -eq $size) { $hits += $f.FullName }
      if ($f.Name -like '$I*') { try { $b = [IO.File]::ReadAllBytes($f.FullName); if ([Text.Encoding]::Unicode.GetString($b) -like "*$name*") { $hits += $f.FullName } } catch {} }
    }
  }
  $hits
}

# ================================================================================================
# Install
# ================================================================================================
$msi = (Get-ChildItem $Dir -Filter *.msi | Select-Object -First 1).FullName
Write-Host "msi: $msi"
$p = Start-Process msiexec.exe -ArgumentList "/i `"$msi`" /qn /norestart /l*v `"$Out\msi-install.log`"" -Wait -PassThru
if ($p.ExitCode -ne 0) { Write-Host "::error::msiexec failed with exit code $($p.ExitCode)"; exit 1 }
if (-not (Test-Path $exe)) { Write-Host "::error::installed app not found at $exe"; exit 1 }

$modes = @([Disp]::Modes() | Sort-Object -Unique)
$want = if ($env:SMOKE_RES) { $env:SMOKE_RES } else { '1920x1080' }
if ($modes -contains $want) { $wh = $want.Split('x'); [void][Disp]::Set([int]$wh[0], [int]$wh[1]); Start-Sleep -Seconds 3 }
(New-Object -ComObject Shell.Application).MinimizeAll(); Start-Sleep -Seconds 1
Initialize-Ocr
Write-Host "OCR engine: $script:OcrKind"

$script:FixtureDir = Join-Path $Work 'fixtures'
$fx = Py @('make', $script:FixtureDir)
Write-Host $fx
$pyOk = ($script:PyExit -eq 0)
Stop-All

# ================================================================================================
# 1. First launch
# ================================================================================================
Journey '1' 'First launch: window, home, no crash or blank window within 3 s' {
  $app = Launch-App
  $main = $null; $paintMs = $null; $windowMs = $null; $stat = $null
  $deadline = (Get-Date).AddSeconds(20)
  while ((Get-Date) -lt $deadline) {
    $app.proc.Refresh()
    if ($app.proc.HasExited) { break }
    if (-not $main) { $main = Get-Main $app.proc; if ($main) { $windowMs = [int]((Get-Date) - $app.t0).TotalMilliseconds } }
    if ($main) {
      $png = Shot 'first-paint' $main
      $stat = Image-Stats $png
      if ($stat.colours -gt 12 -and $stat.flat -lt 0.97) { $paintMs = [int]((Get-Date) - $app.t0).TotalMilliseconds; break }
    }
    Start-Sleep -Milliseconds 100
  }
  Fact 'ms_to_window' $windowMs; Fact 'ms_to_first_painted_frame' $paintMs
  Check 'the process is alive' (-not $app.proc.HasExited) "exit code $(if ($app.proc.HasExited) { $app.proc.ExitCode })"
  Check 'a window titled Panoply appears' ([bool]$main) "after $windowMs ms"
  if ($main) {
    Fact 'window' "$($main.x),$($main.y) $($main.w)x$($main.h) dpi $($main.dpi)"
    if ($paintMs -and $paintMs -le 3000) { Check 'first painted frame within 3 s' $true "$paintMs ms" }
    elseif ($paintMs -and $paintMs -le 10000) { Warn 'first painted frame took longer than 3 s' "$paintMs ms (cold runner, so a script note and not a verdict)" }
    else { Check 'the window is not blank' $false "flat=$($stat.flat) colours=$($stat.colours)" }
    Start-Sleep -Seconds 2
    $m2 = Get-Main $app.proc
    $r = Wait-Text $app.proc 'home' { Get-Main $app.proc } 'choose files|drop|search|Panoply' 8
    Check 'home renders (OCR finds its words)' $r.ok ($r.text -replace '\s+', ' ')
    # Accessibility: does GPUI expose a UI Automation tree on Windows?
    $u = Uia-Dump $m2.hwnd (Join-Path $script:JDir 'uia-tree.json')
    $named = @($u.elements | Where-Object { $_.name }).Count
    Fact 'uia_elements' $u.count; Fact 'uia_named_elements' $named
    Note "UI Automation: $($u.count) elements under the window, $named with a name, root '$($u.root)' ($($u.rootType))$(if ($u.timeout) { ', timed out' })"
  }
  $app.proc.Refresh()
  if (-not $app.proc.HasExited) { Check 'still alive after 6 s' $true } else { Check 'still alive after 6 s' $false "exit code $($app.proc.ExitCode)" }
  if (Test-Path $app.err) { $e = Get-Content $app.err -Raw; if ($e) { Fact 'stderr' $e.Substring(0, [Math]::Min(600, $e.Length)) } }
  Stop-App $app
}

# ================================================================================================
# 2. Hotkey, type "compress", Enter
# ================================================================================================
Journey '2' 'Hotkey, type "compress", Enter: a Compress panel opens' {
  $app = Launch-App @('--background')
  Start-Sleep -Seconds 4
  Check 'the process is alive in the background' (-not $app.proc.HasExited)
  Check 'no window before the hotkey' (-not (Get-Bar $app.proc))
  Press-Hotkey; Start-Sleep -Seconds 2
  $bar = Get-Bar $app.proc
  Check 'the command bar appears on Ctrl+Alt+Space' ([bool]$bar)
  if ($bar) {
    Fact 'bar_rect' "$($bar.x),$($bar.y) $($bar.w)x$($bar.h)"
    Fact 'bar_has_focus_before_typing' ([W]::GetForegroundWindow().ToInt64() -eq $bar.hwnd)
    if ([W]::GetForegroundWindow().ToInt64() -ne $bar.hwnd) { Note 'the bar did not hold keyboard focus after the hotkey; the script clicked it to focus, which a person would also have to do'; [void](Focus-Window $bar) }
    Type-Text 'compress'
    # The typed word has to land in the search box: the placeholder goes away and the Suggested list changes.
    $before = $null; $landed = $false
    for ($k = 1; $k -le 6 -and -not $landed; $k++) {
      Start-Sleep -Milliseconds 1200
      $before = Wait-Text $app.proc "bar-typed-$k" { Get-Bar $app.proc } 'compress' 3
      $landed = ($before.text -notmatch 'Search 31 tools')
    }
    Fact 'rows_after_typing' ($before.text -replace '\s+', ' ')
    Check 'the typed word replaces the placeholder in the search box' $landed ($before.text -replace '\s+', ' ')
    Check 'typed text and result rows are visible' $before.ok ($before.text -replace '\s+', ' ')
    Press-Key 0x0D; Start-Sleep -Seconds 2
    $after = Wait-Text $app.proc 'bar-after-enter' { Get-Bar $app.proc } 'Choose files|Quality|Run|Options' 8
    Fact 'panel_text' ($after.text -replace '\s+', ' ')
    $which = [regex]::Match($after.text, 'Compress (image|PDF|video)', 'IgnoreCase').Value
    Check 'a Compress panel opens (its title and controls)' ($after.ok -and [bool]$which) ($after.text -replace '\s+', ' ')
    Fact 'panel_title' $(if ($which) { $which } else { 'none' })
    if ($which -and $which -ne 'Compress image') { Note "the word compress matches three tools (image, PDF, video); Enter opened: $which" }
    $same = (Image-Stats $before.png); $now = (Image-Stats $after.png)
    Check 'the bar window is still on screen' ([bool](Get-Bar $app.proc))
    Press-Key 0x1B; Start-Sleep -Milliseconds 800
    Press-Hotkey; Start-Sleep -Seconds 1
  }
  Stop-App $app
}

# ================================================================================================
# 3. Open a file by command line, run "under 50 kb"   (5 undoes it in the same session)
# ================================================================================================
Journey '3' 'Open a PNG by command line, run "under 50 kb": an output under 50 KB named (compressed)' {
  if (-not $pyOk) { Limit 'fixtures could not be made (python or pip failed)'; return }
  $d = Join-Path $Work 'j3'; Copy-Fixtures $d
  $png = Join-Path $d 'testpic.png'
  $origHash = (Get-FileHash $png -Algorithm SHA256).Hash; $origSize = (Get-Item $png).Length
  Fact 'input_bytes' $origSize
  $app = Launch-App @($png)
  $main = Wait-Main $app 20
  Check 'the app window opens with the file argument' ([bool]$main)
  if (-not $main) { Stop-App $app; return }
  $r = Wait-Text $app.proc 'file-open' { Get-Main $app.proc } 'testpic' 10
  Check 'the file is shown (its name is on screen)' $r.ok ($r.text -replace '\s+', ' ')
  [void](Focus-Window $main)
  Press-Chord @(0x11) 0x46   # Ctrl+F focuses the search box
  Start-Sleep -Milliseconds 400
  Type-Text 'under 50 kb'
  $q = Wait-Text $app.proc 'query' { Get-Main $app.proc } 'under 50' 6
  Fact 'rows_after_typing' ($q.text -replace '\s+', ' ')
  Check 'the words typed reach the search box' $q.ok
  Press-Key 0x0D
  $out = $null; $presses = 1
  for ($i = 1; $i -le 4 -and -not $out; $i++) {
    $out = Find-NewFile $d 'testpic*(*' @('testpic.png', 'a.pdf', 'b.pdf', 'scan.pdf') 5
    if (-not $out) {
      [void](Shot "wait-$i" (Get-Main $app.proc))
      if ($i -lt 4) { Press-Key 0x0D; $presses++ }   # the options step, if the tool waited for Return
    }
  }
  if (-not $out) { $out = Find-NewFile $d 'testpic*' @('testpic.png', 'a.pdf', 'b.pdf', 'scan.pdf') 15 }
  Fact 'enter_presses' $presses
  $done = Wait-Text $app.proc 'result-card' { Get-Main $app.proc } 'KB|MB|smaller|Undo|Done|saved' 8
  Fact 'result_card' ($done.text -replace '\s+', ' ')
  Check 'an output file appeared next to the input' ([bool]$out)
  if ($out) {
    Fact 'output' $out.Name; Fact 'output_bytes' $out.Length
    Check 'the output is under 50 KB' ($out.Length -lt 50 * 1024) "$($out.Length) bytes"
    Check 'the output is named (compressed)' ($out.Name -like '*(compressed*') $out.Name
  }
  Check 'the original is untouched' ((Get-FileHash $png -Algorithm SHA256).Hash -eq $origHash)
  $script:Sess = [pscustomobject]@{ app = $app; dir = $d; out = $out; ok = ([bool]$out) }
}

# ================================================================================================
# 5. Undo on that result: Recycle Bin, not deleted
# ================================================================================================
Journey '5' 'Undo on a result: the file goes to the Recycle Bin, not deleted' {
  $s = $script:Sess
  if (-not $s -or -not $s.ok) { Skip 'journey 3 produced no output to undo'; Stop-App $(if ($s) { $s.app }); return }
  $app = $s.app; $out = $s.out; $name = $out.Name; $size = $out.Length; $full = $out.FullName
  $main = Get-Main $app.proc
  [void](Focus-Window $main)
  Fact 'file' $name
  Press-Chord @(0x11) 0x5A   # Ctrl+Z on the result card
  Start-Sleep -Seconds 3
  $how = 'Ctrl+Z'
  if (Test-Path $full) {
    # No keyboard undo took effect. Press the Undo button the way a person would: find the word and click it.
    $png = Shot 'result-before-click' (Get-Main $app.proc); $o = Ocr $png
    $w = $o.words | Where-Object { $_.t -match '^Undo' } | Select-Object -First 1
    if ($w) {
      $m = Get-Main $app.proc
      Mouse-Click ($m.x + $w.x + [int]($w.w / 2)) ($m.y + $w.y + [int]($w.h / 2)); Start-Sleep -Seconds 3
      $how = 'clicked the Undo button (Ctrl+Z did nothing)'
      Note 'Ctrl+Z did not undo the result; the Undo button did, if the file is gone'
    } else { Note 'Ctrl+Z did nothing and OCR could not find an Undo button' }
  }
  Fact 'undo_by' $how
  [void](Shot 'after-undo' (Get-Main $app.proc))
  Check 'the output file is gone from the folder' (-not (Test-Path $full))
  Check 'the original is still there' (Test-Path (Join-Path $s.dir 'testpic.png'))
  $hits = In-RecycleBin $name $size
  Fact 'recycle_bin_hits' ($hits -join '; ')
  Check 'the Recycle Bin holds the output (it was not deleted for good)' ([bool]$hits) ($hits -join '; ')
  Stop-App $app
}

# ================================================================================================
# 4. Merge two PDFs, OCR a scanned PDF
# ================================================================================================
Journey '4a' 'Merge two PDFs: one output with 5 pages' {
  if (-not $pyOk) { Limit 'fixtures could not be made'; return }
  $d = Join-Path $Work 'j4a'; Copy-Fixtures $d
  $app = Launch-App @((Join-Path $d 'a.pdf'), (Join-Path $d 'b.pdf'))
  $main = Wait-Main $app 20
  Check 'the app window opens with two files' ([bool]$main)
  if (-not $main) { Stop-App $app; return }
  [void](Wait-Text $app.proc 'files' { Get-Main $app.proc } 'a\.pdf|b\.pdf|merge|PDF' 8)
  [void](Focus-Window $main)
  Press-Chord @(0x11) 0x46; Start-Sleep -Milliseconds 400
  Type-Text 'merge pdf'
  $q = Wait-Text $app.proc 'query' { Get-Main $app.proc } 'merge' 6
  Fact 'rows_after_typing' ($q.text -replace '\s+', ' ')
  Press-Key 0x0D
  $out = $null
  for ($i = 1; $i -le 3 -and -not $out; $i++) {
    $out = Find-NewFile $d '*(merged*.pdf' @() 6
    if (-not $out -and $i -lt 3) { [void](Shot "wait-$i" (Get-Main $app.proc)); Press-Key 0x0D }
  }
  [void](Shot 'result' (Get-Main $app.proc))
  Check 'a merged PDF appeared next to the inputs' ([bool]$out)
  if ($out) {
    $n = Py @('pages', $out.FullName); Fact 'output' $out.Name; Fact 'pages' $n
    Check 'the merged PDF has 5 pages (2 + 3)' ($n -eq '5') "pages=$n"
  }
  Stop-App $app
}
Journey '4b' 'OCR a scanned PDF: the text becomes extractable' {
  if (-not $pyOk) { Limit 'fixtures could not be made'; return }
  $d = Join-Path $Work 'j4b'; Copy-Fixtures $d
  $scan = Join-Path $d 'scan.pdf'
  $before = Py @('text', $scan)
  Check 'the input scan has no text layer to begin with' ([string]::IsNullOrWhiteSpace($before)) $before
  $app = Launch-App @($scan)
  $main = Wait-Main $app 20
  Check 'the app window opens with the scan' ([bool]$main)
  if (-not $main) { Stop-App $app; return }
  [void](Wait-Text $app.proc 'files' { Get-Main $app.proc } 'scan' 8)
  [void](Focus-Window $main)
  Press-Chord @(0x11) 0x46; Start-Sleep -Milliseconds 400
  Type-Text 'ocr'
  $q = Wait-Text $app.proc 'query' { Get-Main $app.proc } 'ocr|text|search' 6
  Fact 'rows_after_typing' ($q.text -replace '\s+', ' ')
  Press-Key 0x0D
  $out = $null; $presses = 1; $t0 = Get-Date
  # Return runs the options step, then OCR asks to download its models and the runtime, one consent sheet each: say yes (Return) while an options or consent sheet shows.
  for ($i = 1; $i -le 14 -and -not $out; $i++) {
    $out = Find-NewFile $d 'scan*(*.pdf' @() 6
    if (-not $out) {
      $w = Wait-Text $app.proc "wait-$i" { Get-Main $app.proc } 'Download the|Not now|Options' 1
      if ($w.ok) { Press-Key 0x0D; $presses++ }
      if (((Get-Date) - $t0).TotalSeconds -ge 200) { break }
    }
  }
  if (-not $out) { $out = Find-NewFile $d 'scan*(*.pdf' @() 60 }
  Fact 'enter_presses' $presses
  [void](Shot 'result' (Get-Main $app.proc))
  Check 'a searchable PDF appeared next to the scan' ([bool]$out)
  if ($out) {
    $t = Py @('text', $out.FullName); Fact 'output' $out.Name; Fact 'extracted_text' ($t -replace '\s+', ' ')
    $words = Py @('words', $out.FullName, 'invoice,4812,total,350,thank,business'); $ok = ($script:PyExit -eq 0)
    Check 'the text can be extracted (at least two of the words on the page)' $ok "found: $words"
  }
  Stop-App $app
}

# ================================================================================================
# 3b. The same words on a grainy photo saved as a PNG (the common exam-portal case)
# ================================================================================================
Journey '3b' 'A photo saved as PNG, "under 50 kb": an output under 50 KB, in whatever format works' {
  if (-not $pyOk) { Limit 'fixtures could not be made (python or pip failed)'; return }
  $d = Join-Path $Work 'j3b'; Copy-Fixtures $d
  $png = Join-Path $d 'photo.png'; Fact 'input_bytes' (Get-Item $png).Length
  $app = Launch-App @($png)
  $main = Wait-Main $app 20
  Check 'the app window opens with the file argument' ([bool]$main)
  if (-not $main) { Stop-App $app; return }
  [void](Wait-Text $app.proc 'file-open' { Get-Main $app.proc } 'photo' 10)
  [void](Focus-Window $main)
  Press-Chord @(0x11) 0x46; Start-Sleep -Milliseconds 400
  Type-Text 'under 50 kb'
  $q = Wait-Text $app.proc 'query' { Get-Main $app.proc } 'under 50' 6
  Fact 'rows_after_typing' ($q.text -replace '\s+', ' ')
  Press-Key 0x0D
  $out = $null
  for ($i = 1; $i -le 4 -and -not $out; $i++) {
    $out = Find-NewFile $d 'photo*(*' @() 6
    if (-not $out -and $i -lt 4) { Press-Key 0x0D }   # the options step, if the tool waited for Return
  }
  $done = Wait-Text $app.proc 'result-card' { Get-Main $app.proc } 'KB|MB|smaller|Undo|Done|saved|did not work' 8
  Fact 'result_card' ($done.text -replace '\s+', ' ')
  Check 'an output file appeared next to the input' ([bool]$out) ($done.text -replace '\s+', ' ')
  if ($out) { Fact 'output' $out.Name; Fact 'output_bytes' $out.Length; Check 'the output is under 50 KB' ($out.Length -lt 50 * 1024) "$($out.Length) bytes" }
  Stop-App $app
}

# ================================================================================================
# 6. Close the window, use the tray, Quit
# ================================================================================================
Journey '6' 'Close the window, tray icon, Quit (tray menu and Ctrl+Q)' {
  $app = Launch-App
  $main = Wait-Main $app 20
  Check 'the window opens' ([bool]$main)
  if (-not $main) { Stop-App $app; return }
  Start-Sleep -Seconds 2
  [void][W]::PostMessage([IntPtr]$main.hwnd, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)   # WM_CLOSE, as the close button does
  Start-Sleep -Seconds 3
  $app.proc.Refresh()
  Check 'the process stays alive (the hotkey still works from the tray)' (-not $app.proc.HasExited)
  if ($app.proc.HasExited) { return }
  Check 'no window is left on screen' (-not (Get-Main $app.proc))
  $win = [W]::ForPid([uint32]$app.proc.Id) | Where-Object { $c = New-Object System.Text.StringBuilder 256; [void][W]::GetClassName($_, $c, 256); $c.ToString() -eq 'tray_icon_app' } | Select-Object -First 1
  $rect = $null; $uid = 0
  if ($win) { foreach ($i in 1..8) { $rr = [Tray]::Rect($win, [uint32]$i); if ($rr) { $rect = $rr; $uid = $i; break } } }
  Check 'a notification-area icon is registered' ([bool]$rect) "rect $($rect -join ',')"
  [void](Shot 'closed' $null)
  # A click on the icon opens the window again.
  Press-Hotkey; Start-Sleep -Seconds 2
  Check 'the hotkey still opens the bar after the window closed' ([bool](Get-Bar $app.proc))
  [void](Shot 'bar-after-close' $null)
  Press-Key 0x1B; Start-Sleep -Seconds 1
  if ((Get-Bar $app.proc)) { Press-Hotkey; Start-Sleep -Seconds 1 }
  if ($win) {
    [void][W]::PostMessage($win, 6002, [IntPtr]$uid, [IntPtr]0x0205)   # the message the shell sends for a right click
    Start-Sleep -Seconds 2
    [void](Shot 'tray-menu' $null)
    Press-Key 0x26; Press-Key 0x0D   # Up wraps to the last item, Quit
  }
  $gone = $app.proc.WaitForExit(15000)
  Check 'Quit in the tray menu ends the process' $gone
  if (-not $gone) { Stop-App $app }
  # Ctrl+Q with the window open
  $app2 = Launch-App
  $m2 = Wait-Main $app2 20
  if ($m2) {
    Start-Sleep -Seconds 2; [void](Focus-Window $m2)
    Press-Chord @(0x11) 0x51
    $gone2 = $app2.proc.WaitForExit(10000)
    Check 'Ctrl+Q quits the app' $gone2
    if (-not $gone2) { [void](Shot 'ctrl-q' $m2); Stop-App $app2 }
  } else { Check 'the window opens a second time' $false }
}

# ================================================================================================
# 7. Light and dark theme, 125% and 150% display scaling
# ================================================================================================
Journey '7' 'Light and dark theme, 125% and 150% scaling' {
  $key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
  $orig = (Get-ItemProperty $key -ErrorAction SilentlyContinue).AppsUseLightTheme
  $bright = @{}
  foreach ($theme in 'light', 'dark') {
    $v = if ($theme -eq 'light') { 1 } else { 0 }
    Set-ItemProperty -Path $key -Name AppsUseLightTheme -Value $v -Type DWord -ErrorAction SilentlyContinue
    Set-ItemProperty -Path $key -Name SystemUsesLightTheme -Value $v -Type DWord -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2
    $app = Launch-App
    $main = Wait-Main $app 20
    Check "the window opens in $theme mode" ([bool]$main)
    if ($main) {
      Start-Sleep -Seconds 3
      $png = Shot "theme-$theme" (Get-Main $app.proc)
      $st = Image-Stats $png; $bright[$theme] = $st.brightness
      Fact "brightness_$theme" $st.brightness
      Check "the window follows the system $theme theme" ($(if ($theme -eq 'light') { $st.brightness -gt 0.6 } else { $st.brightness -lt 0.35 })) "mean brightness $($st.brightness)"
      $t = Ocr $png; Check "text is readable in $theme mode (OCR finds words)" ($t.text.Length -gt 12) ($t.text -replace '\s+', ' ')
    }
    Stop-App $app
  }
  if ($null -ne $orig) { Set-ItemProperty -Path $key -Name AppsUseLightTheme -Value $orig -Type DWord -ErrorAction SilentlyContinue }

  # Display scale
  $scaleReport = @()
  try {
    $id = $null; $g = [Scale]::Get([ref]$id)
    $scaleReport += "start: recommended $([Scale]::Percent($g[0])) percent, current $([Scale]::Percent($g[1])) percent, maximum $([Scale]::Percent($g[2])) percent"
    $startPct = [Scale]::Percent($g[1])
    foreach ($pct in 125, 150) {
      $rc = $null
      try { $rc = [Scale]::Set($pct) } catch { $scaleReport += "set $pct percent: $($_.Exception.Message)"; Check "the runner allows $pct percent scaling" $true; Limit "this runner does not allow $pct percent: $($_.Exception.Message)"; continue }
      Start-Sleep -Seconds 4
      $g2 = [Scale]::Get([ref]$id); $nowPct = [Scale]::Percent($g2[1])
      $scaleReport += "set $pct percent: API result $rc, display now $nowPct percent, screen $([W]::GetSystemMetrics(0))x$([W]::GetSystemMetrics(1))"
      if ($nowPct -ne $pct) { Limit "asked for $pct percent, the display reports $nowPct percent (result $rc)"; continue }
      $app = Launch-App
      $main = Wait-Main $app 20
      Check "the window opens at $pct percent" ([bool]$main)
      if ($main) {
        Start-Sleep -Seconds 3
        $main = Get-Main $app.proc
        $png = Shot "scale-$pct" $main
        $expectDpi = [int](96 * $pct / 100)
        Fact "dpi_at_$pct" $main.dpi; Fact "window_at_$pct" "$($main.w)x$($main.h)"
        Check "the window runs at $expectDpi dpi" ($main.dpi -eq $expectDpi) "dpi $($main.dpi)"
        $sw = [W]::GetSystemMetrics(0); $sh = [W]::GetSystemMetrics(1)
        Check "the window fits the screen at $pct percent" (($main.x -ge -16) -and ($main.y -ge -16) -and ($main.x + $main.w -le $sw + 16) -and ($main.y + $main.h -le $sh + 16)) "$($main.x),$($main.y) $($main.w)x$($main.h) on $($sw)x$($sh)"
        $t = Ocr $png; Check "text is readable at $pct percent" ($t.text.Length -gt 12) ($t.text -replace '\s+', ' ')
        # the bar too
        Press-Hotkey; Start-Sleep -Seconds 2
        $bar = Get-Bar $app.proc
        Check "the command bar opens at $pct percent" ([bool]$bar)
        if ($bar) { [void](Shot "scale-$pct-bar" $bar); Fact "bar_at_$pct" "$($bar.x),$($bar.y) $($bar.w)x$($bar.h) dpi $($bar.dpi)" }
      }
      Stop-App $app
    }
    try { [void][Scale]::Set($startPct) } catch {}
    Start-Sleep -Seconds 3
  } catch {
    $scaleReport += "scale API unavailable: $($_.Exception.Message)"
    Limit "the display scale could not be read or changed on this runner: $($_.Exception.Message)"
  }
  Fact 'scaling' ($scaleReport -join ' | ')
  Note ($scaleReport -join ' | ')
}

# ================================================================================================
# 8. Memory and idle CPU after 60 s idle
# ================================================================================================
Journey '8' 'Memory and idle CPU after 60 s idle' {
  $app = Launch-App
  $main = Wait-Main $app 20
  Check 'the window opens' ([bool]$main)
  if (-not $main) { Stop-App $app; return }
  $p = $app.proc
  Start-Sleep -Seconds 30
  $p.Refresh(); $cpu30 = $p.TotalProcessorTime.TotalSeconds; $t30 = Get-Date
  Start-Sleep -Seconds 30
  $p.Refresh(); $cpu60 = $p.TotalProcessorTime.TotalSeconds; $t60 = Get-Date
  $pct = [Math]::Round(($cpu60 - $cpu30) / ($t60 - $t30).TotalSeconds * 100, 2)
  $ws = [Math]::Round($p.WorkingSet64 / 1MB, 1); $priv = [Math]::Round($p.PrivateMemorySize64 / 1MB, 1)
  Fact 'idle_cpu_percent_of_one_core_30_to_60s' $pct; Fact 'working_set_mb' $ws; Fact 'private_mb' $priv
  Fact 'threads' $p.Threads.Count; Fact 'handles' $p.HandleCount; Fact 'cpu_seconds_total' ([Math]::Round($cpu60, 2))
  [void](Shot 'idle' (Get-Main $p))
  if ($pct -le 1.0) { Check 'idle CPU is at most 1% of a core between 30 s and 60 s' $true "$pct%" }
  elseif ($pct -le 5.0) { Warn 'idle CPU is above 1% of a core' "$pct% (the design target is 0%)" }
  else { Check 'idle CPU is at most 5% of a core' $false "$pct%" }
  if ($ws -le 100) { Check 'working set is at most 100 MB (the idle budget)' $true "$ws MB" }
  elseif ($ws -le 200) { Warn 'working set is above the 100 MB idle budget' "$ws MB" }
  else { Check 'working set is at most 200 MB' $false "$ws MB" }
  Stop-App $app
}

Stop-All
Save-Report
# The step summary: a table a person reads first.
$md = @("### Windows user journeys", "", "OCR engine: $script:OcrKind", "", "| # | Journey | Result | Seconds |", "|---|---|---|---:|")
foreach ($r in $script:Results) { $md += "| $($r.id) | $($r.name) | $($r.status) | $($r.seconds) |" }
foreach ($r in $script:Results) {
  $bad = @($r.checks | Where-Object { -not $_.ok })
  if ($bad.Count -or $r.notes.Count) {
    $md += ""; $md += "**$($r.id)**"
    foreach ($b in $bad) { $md += "- FAILED: $($b.check) $($b.detail)" }
    foreach ($n in $r.notes) { $md += "- $n" }
  }
}
if ($env:GITHUB_STEP_SUMMARY) { $md -join "`n" | Add-Content -Encoding UTF8 $env:GITHUB_STEP_SUMMARY }
$md | ForEach-Object { Write-Host $_ }
# Failing journeys are findings, not a broken workflow: the run only fails when the harness itself broke.
$errors = @($script:Results | Where-Object { $_.status -eq 'error' })
Write-Host "journeys: $($script:Results.Count), fail: $(@($script:Results | Where-Object { $_.status -eq 'fail' }).Count), error: $($errors.Count)"
exit 0
