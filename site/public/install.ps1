# Panoply installer for Windows (PowerShell 5.1 and 7).
#
#   irm https://panoply.anzalabidi.dev/install.ps1 | iex
#
# What it does: picks the right build for this computer (x64 or ARM64), downloads the zip over 4 parallel
# connections, checks it against the release's SHA256SUMS, and installs it for your user only (no admin
# prompt): Panoply goes to %LOCALAPPDATA%\Programs\Panoply, with a Start Menu shortcut, and the folder is
# added to your user PATH so `panoply` works in a new terminal. No telemetry: the only servers it talks to
# are api.github.com and github.com (release files).
#
# Environment overrides (all optional, set them before running):
#   $env:PANOPLY_VERSION = 'v0.3.0-beta'   install this release tag instead of the newest one
#   $env:PANOPLY_APP_DIR = 'C:\some\dir'   where Panoply is installed
#   $env:PANOPLY_CLI_ONLY = '1'            install only the command line tool (panoply.exe)
#   $env:PANOPLY_NO_INTEGRATION = '1'      do not touch the Start Menu or the user PATH
#   $env:GITHUB_TOKEN = '...'              only sent to api.github.com, to avoid its anonymous rate limit
#
# Everything sits inside one function that runs on the last line, so a connection that drops half way
# through the download never runs half a script.

function Install-Panoply {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    $repo = 'anzal1/panoply'
    $streams = 4
    $watch = [System.Diagnostics.Stopwatch]::StartNew()

    # Windows PowerShell 5.1 may default to old TLS versions that GitHub refuses.
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    } catch { }
    # .NET Framework (Windows PowerShell 5.1) allows only 2 connections per host by default. With 4 range
    # requests waiting for headers while 2 hold the connections, that would never finish.
    try { [Net.ServicePointManager]::DefaultConnectionLimit = 16 } catch { }
    Add-Type -AssemblyName System.Net.Http
    Add-Type -AssemblyName System.IO.Compression.FileSystem

    # Which chip. RuntimeInformation reports the real machine even when this shell runs under emulation.
    $arch = $null
    try {
        $osArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
        if ($osArch -eq 'Arm64') { $arch = 'arm64' } elseif ($osArch -eq 'X64') { $arch = 'x64' }
    } catch { }
    if (-not $arch) {
        $raw = $env:PROCESSOR_ARCHITEW6432
        if (-not $raw) { $raw = $env:PROCESSOR_ARCHITECTURE }
        if ($raw -eq 'ARM64') { $arch = 'arm64' } elseif ($raw -eq 'AMD64') { $arch = 'x64' }
    }
    if (-not $arch) { throw 'panoply install: unsupported chip. Panoply for Windows runs on x64 and ARM64.' }

    $cliOnly = ($env:PANOPLY_CLI_ONLY -eq '1')
    $integrate = ($env:PANOPLY_NO_INTEGRATION -ne '1')
    if ($env:PANOPLY_APP_DIR) {
        $dest = $env:PANOPLY_APP_DIR
    } elseif ($cliOnly) {
        $dest = Join-Path $env:LOCALAPPDATA 'Programs\Panoply-cli'
    } else {
        $dest = Join-Path $env:LOCALAPPDATA 'Programs\Panoply'
    }

    # The install folder is moved aside and deleted when it is replaced, so only touch one that is missing,
    # empty, or already a Panoply install (it holds panoply.exe). Anything else could be somebody's own files.
    if (Test-Path -LiteralPath $dest) {
        $destItem = Get-Item -LiteralPath $dest -Force
        $isLink = [bool]($destItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint)
        $isInstall = $destItem.PSIsContainer -and (Test-Path -LiteralPath (Join-Path $dest 'panoply.exe'))
        $isEmpty = $destItem.PSIsContainer -and -not (Get-ChildItem -LiteralPath $dest -Force | Select-Object -First 1)
        if ($isLink -or -not ($isInstall -or $isEmpty)) {
            throw "panoply install: $dest exists and is not a Panoply install, pick another folder"
        }
    }

    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.AllowAutoRedirect = $true
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromMinutes(10)
    $client.DefaultRequestHeaders.UserAgent.ParseAdd('panoply-installer')

    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ('panoply-install-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    $stage = $null
    try {
        # Step 1: which release. Every release is a prerelease for now, so /releases/latest does not work.
        $listing = $null
        if ($env:PANOPLY_VERSION) {
            $tag = $env:PANOPLY_VERSION
            if (-not $tag.StartsWith('v')) { $tag = 'v' + $tag }
        } else {
            $apiReq = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Get, "https://api.github.com/repos/$repo/releases?per_page=5")
            $apiReq.Headers.Accept.ParseAdd('application/vnd.github+json')
            if ($env:GITHUB_TOKEN) { $apiReq.Headers.Add('Authorization', "Bearer $($env:GITHUB_TOKEN)") }
            $apiResp = $client.SendAsync($apiReq).Result
            if (-not $apiResp.IsSuccessStatusCode) {
                throw 'panoply install: could not read the release list from GitHub (rate limit? set $env:PANOPLY_VERSION = ''v0.3.0-beta'' or $env:GITHUB_TOKEN).'
            }
            $json = $apiResp.Content.ReadAsStringAsync().Result | ConvertFrom-Json
            # The newest release that is not a draft (a token can see drafts, and their downloads answer 404).
            $release = @($json) | Where-Object { -not $_.draft } | Select-Object -First 1
            if (-not $release) { throw "panoply install: no release found for $repo" }
            $tag = $release.tag_name
            $listing = @($release.assets | ForEach-Object { $_.name })
        }
        $ver = $tag.TrimStart('v')
        $base = "https://github.com/$repo/releases/download/$tag"
        Write-Host "Panoply $ver for windows $arch"

        # Step 2: which asset. The zip, not the msi: nothing to elevate, nothing to uninstall by hand.
        if ($cliOnly) { $asset = "panoply-cli-$ver-windows-$arch.zip" } else { $asset = "Panoply-$ver-windows-$arch.zip" }
        if ($listing -and ($listing -notcontains $asset)) { throw "panoply install: no windows $arch download in release $tag" }

        # Fetch the checksums and ask about the archive (size, range support, final address) at the same time.
        $sumsTask = $client.GetStringAsync("$base/SHA256SUMS")
        $probeReq = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Get, "$base/$asset")
        $probeReq.Headers.Range = [System.Net.Http.Headers.RangeHeaderValue]::new([long]0, [long]0)
        $probeResp = $client.SendAsync($probeReq, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead).Result
        $code = [int]$probeResp.StatusCode
        if ($code -ne 206 -and $code -ne 200) { throw "panoply install: $asset was not found in release $tag (HTTP $code)" }
        $finalUrl = $probeResp.RequestMessage.RequestUri.AbsoluteUri
        $ranges = ($code -eq 206)
        $total = [long]0
        if ($ranges -and $probeResp.Content.Headers.ContentRange -and $probeResp.Content.Headers.ContentRange.Length) {
            $total = [long]$probeResp.Content.Headers.ContentRange.Length
        } else {
            $ranges = $false
            if ($probeResp.Content.Headers.ContentLength) { $total = [long]$probeResp.Content.Headers.ContentLength }
        }
        $probeResp.Dispose()

        # Step 3: download.
        $archive = Join-Path $tmp $asset
        $mb = [math]::Round($total / 1MB, 1)
        Write-Host "Downloading $asset ($mb MB)"
        $done = $false
        $files = @()
        $responses = @()
        if ($ranges -and $total -ge 1MB) {
            try {
                $fs = [System.IO.File]::Open($archive, 'Create', 'Write', 'ReadWrite')
                $fs.SetLength($total)
                $fs.Dispose()
                $chunk = [long][math]::Ceiling($total / $streams)
                $headTasks = @()
                for ($i = 0; $i -lt $streams; $i++) {
                    $start = [long]($i * $chunk)
                    $end = [long][math]::Min($start + $chunk - 1, $total - 1)
                    $req = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Get, $finalUrl)
                    $req.Headers.Range = [System.Net.Http.Headers.RangeHeaderValue]::new($start, $end)
                    $headTasks += $client.SendAsync($req, [System.Net.Http.HttpCompletionOption]::ResponseHeadersRead)
                }
                if (-not [System.Threading.Tasks.Task]::WaitAll([System.Threading.Tasks.Task[]]$headTasks, 60000)) { throw 'range requests timed out' }
                $copyTasks = @()
                $partEnds = @()
                for ($i = 0; $i -lt $streams; $i++) {
                    $resp = $headTasks[$i].Result
                    if ([int]$resp.StatusCode -ne 206) { throw "range request $i answered HTTP $([int]$resp.StatusCode)" }
                    $responses += $resp
                    $src = $resp.Content.ReadAsStreamAsync().Result
                    $dst = [System.IO.FileStream]::new($archive, 'Open', 'Write', 'ReadWrite')
                    [void]$dst.Seek([long]($i * $chunk), [System.IO.SeekOrigin]::Begin)
                    $files += $dst
                    $partEnds += [long]([math]::Min(($i + 1) * $chunk, $total))
                    $copyTasks += $src.CopyToAsync($dst)
                }
                if (-not [System.Threading.Tasks.Task]::WaitAll([System.Threading.Tasks.Task[]]$copyTasks, 300000)) { throw 'download timed out' }
                # The file was pre-sized, so its length proves nothing. A part is complete only when its stream
                # wrote exactly its own number of bytes (the position ends at the part's end).
                $allParts = $true
                for ($i = 0; $i -lt $streams; $i++) {
                    if ($files[$i].Position -ne $partEnds[$i]) { $allParts = $false }
                }
                foreach ($f in $files) { $f.Dispose() }
                foreach ($r in $responses) { $r.Dispose() }
                if ($allParts) { $done = $true }
            } catch {
                foreach ($f in $files) { try { $f.Dispose() } catch { } }
                foreach ($r in $responses) { try { $r.Dispose() } catch { } }
                Write-Host '  parallel download failed, retrying on one connection'
            }
        }
        if (-not $done) {
            if (Test-Path $archive) { Remove-Item $archive -Force }
            Invoke-WebRequest -Uri "$base/$asset" -OutFile $archive -UseBasicParsing
        }

        # Step 4: verify against SHA256SUMS.
        $want = $null
        foreach ($line in ($sumsTask.Result -split "`n")) {
            $parts = $line.Trim() -split '\s+', 2
            if ($parts.Count -eq 2 -and $parts[1].TrimStart('*') -eq $asset) { $want = $parts[0].ToLower(); break }
        }
        if (-not $want) { throw "panoply install: $asset is not listed in SHA256SUMS, refusing to install it" }
        $got = (Get-FileHash -Algorithm SHA256 -Path $archive).Hash.ToLower()
        if ($got -ne $want) {
            throw "panoply install: checksum mismatch for $asset (expected $want, got $got). Nothing was installed."
        }
        Write-Host 'Checksum ok'

        # Step 5: install. Unpack beside the destination, then swap by rename, so the old copy only goes
        # away after the new one is ready.
        Write-Host "Installing to $dest"
        $parent = Split-Path -Parent $dest
        if (-not (Test-Path $parent)) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
        $stage = Join-Path $parent ('.panoply-stage-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
        New-Item -ItemType Directory -Path $stage | Out-Null
        try {
            [System.IO.Compression.ZipFile]::ExtractToDirectory($archive, $stage)
        } catch {
            Expand-Archive -Path $archive -DestinationPath $stage -Force
        }
        $payload = $stage
        $inner = @(Get-ChildItem -Path $stage -Force)
        if ($inner.Count -eq 1 -and $inner[0].PSIsContainer) { $payload = $inner[0].FullName }
        $exe = Join-Path $payload 'panoply.exe'
        if (-not (Test-Path $exe)) { throw 'panoply install: the archive has no panoply.exe' }

        $old = $null
        if (Test-Path $dest) {
            $old = $dest + '.old-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
            try {
                Move-Item -Path $dest -Destination $old
            } catch {
                throw "panoply install: could not replace $dest. Close Panoply and run the installer again."
            }
        }
        try {
            Move-Item -Path $payload -Destination $dest
        } catch {
            if ($old) { Move-Item -Path $old -Destination $dest }
            throw "panoply install: could not put the new copy in place at $dest"
        }
        if ($old) {
            try { Remove-Item -Path $old -Recurse -Force } catch { Write-Host "  could not remove the old copy at $old, you can delete it" }
        }

        if ($integrate) {
            # Start Menu shortcut (the desktop app only; the CLI-only zip has no desktop exe).
            $desktopExe = Join-Path $dest 'panoply-desktop.exe'
            if (Test-Path $desktopExe) {
                $menuDir = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
                if (-not (Test-Path $menuDir)) { New-Item -ItemType Directory -Path $menuDir -Force | Out-Null }
                $shell = New-Object -ComObject WScript.Shell
                $link = $shell.CreateShortcut((Join-Path $menuDir 'Panoply.lnk'))
                $link.TargetPath = $desktopExe
                $link.WorkingDirectory = $dest
                $icon = Join-Path $dest 'panoply.ico'
                if (Test-Path $icon) { $link.IconLocation = $icon }
                $link.Description = 'Every file tool in one fast app'
                $link.Save()
                Write-Host 'Added the Start Menu shortcut'
            }
            # User PATH, so `panoply` works in a new terminal. Read the raw value (no %VARS% expanded) and write it
            # back as an expandable string, so entries such as %USERPROFILE%\bin stay as they were.
            $envKey = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment')
            $userPath = ''
            if ($envKey) {
                $userPath = [string]$envKey.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                $envKey.Close()
            }
            $entries = @($userPath -split ';' | Where-Object { $_ })
            $already = $false
            foreach ($e in $entries) {
                $expanded = [Environment]::ExpandEnvironmentVariables($e)
                if ($expanded.TrimEnd('\') -ieq $dest.TrimEnd('\')) { $already = $true }
            }
            if (-not $already) {
                $newPath = (@($entries) + $dest) -join ';'
                Set-ItemProperty -Path 'HKCU:\Environment' -Name Path -Value $newPath -Type ExpandString
                # Tell running programs the environment changed (deleting a name that does not exist still broadcasts).
                try { [Environment]::SetEnvironmentVariable('PANOPLY_PATH_REFRESH', $null, 'User') } catch { }
                Write-Host 'Added the install folder to your user PATH (open a new terminal to use panoply)'
            }
            # This session too.
            if (-not (($env:Path -split ';') -contains $dest)) { $env:Path = "$env:Path;$dest" }
        }

        $watch.Stop()
        $secs = [math]::Round($watch.Elapsed.TotalSeconds, 1)
        Write-Host "Installed Panoply $ver in $secs s"
    }
    finally {
        $client.Dispose()
        if (Test-Path $tmp) { Remove-Item -Path $tmp -Recurse -Force -ErrorAction SilentlyContinue }
        if ($stage -and (Test-Path $stage)) { Remove-Item -Path $stage -Recurse -Force -ErrorAction SilentlyContinue }
    }
}

Install-Panoply
