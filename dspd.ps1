# SPDX-License-Identifier: MIT
# Copyright (c) 2026 itznan.
<#
.SYNOPSIS
    dspd Interactive Drive Benchmark Launcher for Windows.
.DESCRIPTION
    Scans all local drives, pen drives, removable USB storage, NVMe, and SATA SSDs/HDDs,
    allows selecting a specific drive interactively, and runs dspd benchmarks.
    Automatically obtains the dspd binary if not already present.
.PARAMETER Size
    Benchmark test file size (e.g., 512MiB, 1GiB, 4GiB). Default is 1GiB.
.PARAMETER Passes
    Number of benchmark passes per profile. Default is 3.
.PARAMETER Duration
    Duration of each pass in seconds. Default is 5.
.PARAMETER Warmup
    Warmup window in seconds. Default is 1.
.PARAMETER Profile
    Optional single profile name (e.g., "RND4K Q32T1", "SEQ1M Q8T1").
.PARAMETER Json
    Output formatted JSON to the console.
.PARAMETER Yes
    Skip SSD wear warning prompt.
.EXAMPLE
    .\dspd.ps1
.EXAMPLE
    .\dspd.ps1 -Size 512MiB -Passes 1 -Yes
.EXAMPLE
    irm https://raw.githubusercontent.com/itznan/dspd/main/dspd.ps1 | iex
#>

[CmdletBinding()]
param(
    [string]$Size = "1GiB",
    [int]$Passes = 3,
    [int]$Duration = 5,
    [int]$Warmup = 1,
    [string]$Profile,
    [switch]$Json,
    [switch]$Yes,
    [switch]$Elevate
)

$ErrorActionPreference = "Stop"

# Optional self-elevation if -Elevate is explicitly requested
if ($Elevate) {
    $isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    if (-not $isAdmin) {
        Write-Host "[*] Elevating to Administrator..." -ForegroundColor Cyan
        try {
            $cmd = if ($PSCommandPath) { "& '$PSCommandPath'" } else { "irm https://raw.githubusercontent.com/itznan/dspd/main/dspd.ps1 | iex" }
            Start-Process powershell.exe -ArgumentList "-NoProfile -ExecutionPolicy Bypass -Command `"$cmd`"" -Verb RunAs
            exit
        } catch {
            Write-Warning "Elevation declined or unavailable. Continuing in standard mode..."
        }
    }
}

# 2. Locate or Download dspd.exe binary
function Find-OrDownloadDspd {
    $candidates = @(
        (Join-Path $PSScriptRoot "dspd.exe"),
        (Join-Path $PSScriptRoot "target\release\dspd.exe"),
        (Join-Path $PSScriptRoot "target\debug\dspd.exe"),
        (Join-Path (Get-Location) "dspd.exe"),
        (Join-Path $env:TEMP "dspd\dspd.exe")
    )

    foreach ($path in $candidates) {
        if ($path -and (Test-Path -Path $path -PathType Leaf)) {
            return (Resolve-Path $path).Path
        }
    }

    $inPath = Get-Command "dspd.exe" -ErrorAction SilentlyContinue
    if ($inPath) {
        return $inPath.Source
    }

    # Auto-download from GitHub releases
    $destDir = Join-Path $env:TEMP "dspd"
    $destExe = Join-Path $destDir "dspd.exe"
    $destZip = Join-Path $destDir "dspd_download.zip"

    if (-not (Test-Path $destDir)) {
        New-Item -ItemType Directory -Path $destDir -Force | Out-Null
    }

    Write-Host "[*] dspd.exe not found locally. Downloading latest release from GitHub..." -ForegroundColor Cyan
    $repo = "itznan/dspd"
    $arch = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "aarch64-pc-windows-msvc" } else { "x86_64-pc-windows-msvc" }
    $downloadUrl = "https://github.com/$repo/releases/latest/download/dspd-v0.1.0-$arch.zip"

    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13
        $apiUri = "https://api.github.com/repos/$repo/releases/latest"
        $releaseInfo = Invoke-RestMethod -Uri $apiUri -UseBasicParsing -Headers @{ "User-Agent" = "dspd" } -ErrorAction SilentlyContinue
        if ($releaseInfo -and $releaseInfo.assets) {
            $matchedAsset = $releaseInfo.assets | Where-Object { $_.name -like "*$arch.zip" -and $_.name -notlike "*.sha256" } | Select-Object -First 1
            if ($matchedAsset -and $matchedAsset.browser_download_url) {
                $downloadUrl = $matchedAsset.browser_download_url
            }
        }
    } catch {}

    try {
        Invoke-WebRequest -Uri $downloadUrl -OutFile $destZip -UseBasicParsing
        Expand-Archive -Path $destZip -DestinationPath $destDir -Force
        Remove-Item $destZip -Force -ErrorAction SilentlyContinue

        # Check subdirectories if zip contained a folder
        $extractedExe = Get-ChildItem -Path $destDir -Recurse -Filter "dspd.exe" | Select-Object -First 1
        if ($extractedExe) {
            Copy-Item -Path $extractedExe.FullName -Destination $destExe -Force
            return $destExe
        }
    } catch {
        Write-Warning "Could not download pre-built binary: $_"
    }

    throw "dspd.exe could not be found or downloaded. Please ensure dspd.exe is present in your folder or build it using 'cargo build --release'."
}

$dspdPath = Find-OrDownloadDspd

# 3. Discover All Available Drives (Fixed, Removable / Pen Drives, External Storage)
Write-Host ""
Write-Host "========================================================================================" -ForegroundColor DarkCyan
Write-Host "               dspd Disk Benchmark - Interactive Drive Selector                         " -ForegroundColor Cyan
Write-Host "========================================================================================" -ForegroundColor DarkCyan

$logicalDrives = Get-CimInstance Win32_LogicalDisk | Where-Object { $_.DriveType -in 2, 3 }

$driveList = @()
$counter = 1

foreach ($ld in $logicalDrives) {
    $letter = $ld.DeviceId.TrimEnd(':')
    
    # Resolve physical disk model & bus type if possible
    $model = "Storage Device"
    $busType = if ($ld.DriveType -eq 2) { "USB" } else { "Fixed" }
    
    try {
        $partition = Get-Partition -DriveLetter $letter -ErrorAction SilentlyContinue
        if ($partition) {
            $disk = $partition | Get-Disk -ErrorAction SilentlyContinue
            if ($disk) {
                $model = $disk.FriendlyName
                $busType = "$($disk.BusType)"
            }
        }
    } catch {}

    $freeGb = [math]::Round($ld.FreeSpace / 1GB, 1)
    $totalGb = [math]::Round($ld.Size / 1GB, 1)
    $typeLabel = if ($ld.DriveType -eq 2) { "Pen Drive / USB" } else { "Local Drive" }

    $driveList += [PSCustomObject]@{
        Index       = $counter
        Letter      = $letter
        Model       = $model
        BusType     = $busType
        TypeLabel   = $typeLabel
        FreeGB      = $freeGb
        TotalGB     = $totalGb
        FreeBytes   = $ld.FreeSpace
        TotalBytes  = $ld.Size
    }

    $formattedLine = "  [{0}] Drive {1}:  {2} [{3}, {4}] (Free: {5} GB / Total: {6} GB)" -f `
        $counter, $letter, $model, $busType, $typeLabel, $freeGb, $totalGb
    
    if ($ld.DriveType -eq 2) {
        Write-Host $formattedLine -ForegroundColor Yellow
    } else {
        Write-Host $formattedLine -ForegroundColor White
    }
    
    $counter++
}

Write-Host "========================================================================================" -ForegroundColor DarkCyan

if ($driveList.Count -eq 0) {
    Write-Error "No eligible storage drives were detected."
    exit 1
}

# 4. Interactive Drive Selection
$selectedItem = $null

while ($null -eq $selectedItem) {
    $prompt = "Select a drive to benchmark [1-$($driveList.Count) or drive letter, default: 1]: "
    Write-Host $prompt -NoNewline -ForegroundColor Green
    $selection = [Console]::ReadLine().Trim()

    if ([string]::IsNullOrWhiteSpace($selection)) {
        $selectedItem = $driveList[0]
        break
    }

    # Match by index number
    if ($selection -match '^\d+$') {
        $num = [int]$selection
        if ($num -ge 1 -and $num -le $driveList.Count) {
            $selectedItem = $driveList[$num - 1]
            break
        }
    }

    # Match by drive letter (e.g. C, D, E, F)
    $letterMatch = $driveList | Where-Object { $_.Letter -eq $selection.ToUpper() }
    if ($letterMatch) {
        $selectedItem = $letterMatch
        break
    }

    Write-Host "  Invalid selection. Please enter a valid number or drive letter." -ForegroundColor Red
}

Write-Host "`n[*] Selected Target: Drive $($selectedItem.Letter): ($($selectedItem.Model))" -ForegroundColor Cyan

# 5. Build arguments for dspd
$dspdArgs = @(
    "--drives", $selectedItem.Letter,
    "--include-removable",
    "--size", $Size,
    "--passes", $Passes,
    "--duration", $Duration,
    "--warmup", $Warmup
)

if ($Profile) {
    $dspdArgs += @("--profile", $Profile)
}

if ($Json) {
    $dspdArgs += "--json"
}

if ($Yes) {
    $dspdArgs += "--yes"
}

Write-Host "[*] Executing dspd engine..." -ForegroundColor DarkGray
Write-Host "    Command: `"$dspdPath`" $($dspdArgs -join ' ')`n" -ForegroundColor DarkGray

& $dspdPath @dspdArgs

Write-Host "`n[+] Benchmark finished successfully." -ForegroundColor Green
