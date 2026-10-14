<#
Packages the Windows artifacts for one architecture from binaries already built into -Stage.

  package.ps1 -Version 0.1.0-beta.1 -MsiVersion 0.1.0.1 -Arch x64|arm64 -Stage DIR -Out DIR -Desktop real|stub|none [-SmokeTest]

-Stage holds panoply.exe and, with -Desktop real, panoply-desktop.exe.
  real  ships the desktop app
  stub  copies panoply.exe as panoply-desktop.exe to exercise the zip and MSI paths while the desktop
        crate is absent; outputs get a "selftest-" prefix and are never released
  none  CLI zip only
Output in -Out: panoply-cli-V-windows-ARCH.zip, Panoply-V-windows-ARCH.zip, Panoply-V-windows-ARCH.msi,
and state/windows-ARCH.env with the MSI ProductCode (the winget manifest needs it).

Needs the WiX v4 tool (`dotnet tool install --global wix --version 4.0.5`) and its UI extension
(`wix extension add -g WixToolset.UI.wixext/4.0.5`) for the MSI. Code signing slots in at the
"SIGNING HOOK" comment; see packaging/signing/SIGNPATH.md.
#>
param(
  [Parameter(Mandatory)][string]$Version,
  [Parameter(Mandatory)][string]$MsiVersion,
  [Parameter(Mandatory)][ValidateSet('x64', 'arm64')][string]$Arch,
  [Parameter(Mandatory)][string]$Stage,
  [Parameter(Mandatory)][string]$Out,
  [ValidateSet('real', 'stub', 'none')][string]$Desktop = 'none',
  [switch]$SmokeTest
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

$Pkg = Split-Path -Parent $PSScriptRoot
$Root = Split-Path -Parent $Pkg
$Stage = (Resolve-Path $Stage).Path
New-Item -ItemType Directory -Force -Path $Out, (Join-Path $Out 'state') | Out-Null
$Out = (Resolve-Path $Out).Path
$prefix = if ($Desktop -eq 'stub') { 'selftest-' } else { '' }
$mk = Join-Path $Pkg 'scripts/mkarchive.py'
$licenses = @((Join-Path $Root 'LICENSE-MIT'), (Join-Path $Root 'LICENSE-APACHE'))

# SIGNING HOOK: sign "$Stage\panoply.exe" and "$Stage\panoply-desktop.exe" here, before anything is
# archived or put in the MSI, then sign the .msi once it is built (below).

# ---- zips ----------------------------------------------------------------------------------------
$cliName = "panoply-cli-$Version-windows-$Arch"
python -I $mk "$Out/$cliName.zip" $cliName "$Stage/panoply.exe=panoply.exe" @licenses "$Root/README.md"

if ($Desktop -eq 'none') { return }
$desktopExe = Join-Path $Stage 'panoply-desktop.exe'
if ($Desktop -eq 'stub') { Copy-Item "$Stage/panoply.exe" $desktopExe -Force }
$appName = "${prefix}Panoply-$Version-windows-$Arch"
python -I $mk "$Out/$appName.zip" $appName "$Stage/panoply-desktop.exe=panoply-desktop.exe" "$Stage/panoply.exe=panoply.exe" @licenses "$Root/README.md"

# ---- MSI -----------------------------------------------------------------------------------------
$work = Join-Path ([IO.Path]::GetTempPath()) "panoply-msi-$Arch"
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $work | Out-Null

$icon = Join-Path $Root 'apps/desktop/packaging/icons/panoply.ico'
if (-not (Test-Path $icon)) { $icon = Join-Path $Pkg 'assets/panoply.ico' }
Copy-Item $icon (Join-Path $work 'panoply.ico')

# The setup dialog needs a license as RTF.
$text = (Get-Content (Join-Path $Root 'LICENSE-MIT') -Raw) -replace '\\', '\\' -replace '\{', '\{' -replace '\}', '\}'
$body = ($text -split "`r?`n" | ForEach-Object { "$_\par" }) -join "`n"
$rtf = "{\rtf1\ansi\deff0{\fonttbl{\f0 Segoe UI;}}\f0\fs20 Panoply is open source software, available under the MIT license or the Apache License 2.0, at your option.\par\par`n$body`n}"
Set-Content -Path (Join-Path $work 'license.rtf') -Value $rtf -Encoding ascii

# A fixed ProductCode per architecture and version, so a rebuild of the same tag matches.
$md5 = [Security.Cryptography.MD5]::Create()
$bytes = $md5.ComputeHash([Text.Encoding]::UTF8.GetBytes("panoply|$Arch|$MsiVersion"))
$productCode = ([guid]::new($bytes)).ToString().ToUpper()

$msi = Join-Path $Out "$appName.msi"
wix build (Join-Path $PSScriptRoot 'panoply.wxs') -arch $Arch -ext WixToolset.UI.wixext -pdbtype none `
  -d "Version=$MsiVersion" -d "ProductCode={$productCode}" -d "BinDir=$Stage" -d "AssetDir=$work" `
  -d "LicenseRtf=$(Join-Path $work 'license.rtf')" -o $msi
# SIGNING HOOK (MSI): sign $msi here.
"PRODUCT_CODE={$productCode}" | Set-Content (Join-Path $Out "state/windows-$Arch.env") -Encoding ascii
Write-Host "wrote $msi ($((Get-Item $msi).Length) bytes), ProductCode {$productCode}"

# ---- smoke test: install, check, uninstall (CI runners are disposable) -----------------------------
if ($SmokeTest) {
  $dir = Join-Path $env:ProgramFiles 'Panoply'
  $log = Join-Path $work 'msi.log'
  function Invoke-Msi([string[]]$msiArgs) {
    $p = Start-Process msiexec.exe -ArgumentList $msiArgs -Wait -PassThru
    if ($p.ExitCode -ne 0) { Get-Content $log -Tail 80; throw "msiexec $($msiArgs -join ' ') exited $($p.ExitCode)" }
  }
  function Test-MenuKey { Test-Path -LiteralPath 'HKLM:\SOFTWARE\Classes\*\shell\Panoply\command' }
  function Test-OnPath { (([Environment]::GetEnvironmentVariable('Path', 'Machine') -split ';') | ForEach-Object { $_.TrimEnd('\') }) -contains $dir }

  # Everything on (the default).
  Invoke-Msi @('/i', "`"$msi`"", '/qn', '/l*v', "`"$log`"")
  foreach ($f in 'panoply.exe', 'panoply-desktop.exe', 'panoply.ico') {
    if (-not (Test-Path (Join-Path $dir $f))) { throw "missing after install: $f" }
  }
  if (-not (Test-MenuKey)) { throw 'Explorer menu key missing after a default install' }
  if (-not (Test-OnPath)) { throw 'install folder not on the machine PATH after a default install' }
  & (Join-Path $dir 'panoply.exe') --version
  Invoke-Msi @('/x', "`"$msi`"", '/qn', '/l*v', "`"$log`"")
  if (Test-Path $dir) { throw 'install folder still there after uninstall' }
  if (Test-MenuKey) { throw 'Explorer menu key still there after uninstall' }
  if (Test-OnPath) { throw 'PATH entry still there after uninstall' }

  # Optional features off: no Explorer menu, no PATH entry.
  Invoke-Msi @('/i', "`"$msi`"", '/qn', 'ADDLOCAL=Main', '/l*v', "`"$log`"")
  if (Test-MenuKey) { throw 'Explorer menu key present although the feature was not selected' }
  if (Test-OnPath) { throw 'PATH entry present although the feature was not selected' }
  if (-not (Test-Path (Join-Path $dir 'panoply-desktop.exe'))) { throw 'main feature files missing' }
  Invoke-Msi @('/x', "`"$msi`"", '/qn', '/l*v', "`"$log`"")
  Write-Host 'MSI smoke test passed: install, optional features, uninstall'
}
