param(
    [Parameter(Mandatory = $true)]
    [string]$Version,
    [Parameter(Mandatory = $true)]
    [string]$PackageName,
    [Parameter(Mandatory = $true)]
    [string]$Publisher,
    [Parameter(Mandatory = $true)]
    [string]$PublisherDisplayName,
    [string]$IconPath = "assets/icons/app-icon.png",
    [string]$X64Exe = "",
    [string]$Arm64Exe = "",
    [string]$OutputDir = "dist-msix",
    [switch]$SkipBundle
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Resolve-MsixVersion([string]$RawVersion) {
    $clean = $RawVersion.Trim() -replace '^v', ''
    $core = ($clean -split '-')[0]
    $parts = $core.Split('.')
    if ($parts.Count -lt 3 -or $parts.Count -gt 4) {
        throw "Unsupported version format '$RawVersion'; expected MAJOR.MINOR.PATCH"
    }
    $nums = @()
    foreach ($p in $parts) {
        $n = 0
        if (-not [int]::TryParse($p, [ref]$n) -or $n -lt 0 -or $n -gt 65535) {
            throw "Invalid numeric version segment '$p' in '$RawVersion'"
        }
        $nums += $n
    }
    if ($nums.Count -eq 3) {
        $nums += 0
    }
    return "$($nums[0]).$($nums[1]).$($nums[2]).$($nums[3])"
}

function Find-WindowsSdkTool([string]$ToolName) {
    $kitsRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    if (Test-Path $kitsRoot) {
        $candidates = Get-ChildItem -Path $kitsRoot -Directory |
            Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
            Sort-Object { [version]$_.Name } -Descending
        foreach ($dir in $candidates) {
            $tool = Join-Path $dir.FullName "x64\$ToolName"
            if (Test-Path $tool) {
                return $tool
            }
        }
    }
    $cmd = Get-Command $ToolName -ErrorAction SilentlyContinue
    if ($cmd) {
        return $cmd.Source
    }
    throw "Could not find Windows SDK tool '$ToolName'"
}

function Write-ScaledPng([string]$SrcPath, [string]$DestPath, [int]$Width, [int]$Height) {
    Add-Type -AssemblyName System.Drawing
    $src = [System.Drawing.Image]::FromFile($SrcPath)
    try {
        $bmp = New-Object System.Drawing.Bitmap($Width, $Height, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        try {
            $gfx = [System.Drawing.Graphics]::FromImage($bmp)
            try {
                $gfx.Clear([System.Drawing.Color]::Transparent)
                $gfx.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
                $gfx.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
                $gfx.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
                $gfx.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality

                $scale = [Math]::Min($Width / [double]$src.Width, $Height / [double]$src.Height)
                $drawW = [int][Math]::Round($src.Width * $scale)
                $drawH = [int][Math]::Round($src.Height * $scale)
                $offsetX = [int][Math]::Floor(($Width - $drawW) / 2.0)
                $offsetY = [int][Math]::Floor(($Height - $drawH) / 2.0)
                $gfx.DrawImage($src, $offsetX, $offsetY, $drawW, $drawH)
            } finally {
                $gfx.Dispose()
            }
            $bmp.Save($DestPath, [System.Drawing.Imaging.ImageFormat]::Png)
        } finally {
            $bmp.Dispose()
        }
    } finally {
        $src.Dispose()
    }
}

$msixVersion = Resolve-MsixVersion $Version
$makeAppx = Find-WindowsSdkTool "makeappx.exe"
$makePri = Find-WindowsSdkTool "makepri.exe"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$templatePath = Join-Path $PSScriptRoot "AppxManifest.xml.template"
$resolvedIcon = (Resolve-Path (Join-Path $repoRoot $IconPath)).Path

$targets = @()
if (-not [string]::IsNullOrWhiteSpace($X64Exe)) {
    $targets += @{ Arch = "x64"; Exe = (Resolve-Path $X64Exe).Path }
}
if (-not [string]::IsNullOrWhiteSpace($Arm64Exe)) {
    $targets += @{ Arch = "arm64"; Exe = (Resolve-Path $Arm64Exe).Path }
}
if ($targets.Count -eq 0) {
    throw "Specify at least one of -X64Exe or -Arm64Exe"
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$resolvedOut = (Resolve-Path $OutputDir).Path
$workDir = Join-Path $resolvedOut "_work"
if (Test-Path $workDir) {
    Remove-Item -Recurse -Force $workDir
}
New-Item -ItemType Directory -Force -Path $workDir | Out-Null

$assetsDir = Join-Path $workDir "Assets"
New-Item -ItemType Directory -Force -Path $assetsDir | Out-Null

$assetSpecs = @(
    @{ Name = "StoreLogo.png"; W = 50; H = 50 },
    @{ Name = "StoreLogo.scale-200.png"; W = 100; H = 100 },
    @{ Name = "Square44x44Logo.png"; W = 44; H = 44 },
    @{ Name = "Square44x44Logo.scale-200.png"; W = 88; H = 88 },
    @{ Name = "Square44x44Logo.targetsize-48_altform-unplated.png"; W = 48; H = 48 },
    @{ Name = "Square44x44Logo.targetsize-256_altform-unplated.png"; W = 256; H = 256 },
    @{ Name = "Square150x150Logo.png"; W = 150; H = 150 },
    @{ Name = "Square150x150Logo.scale-200.png"; W = 300; H = 300 },
    @{ Name = "Wide310x150Logo.png"; W = 310; H = 150 },
    @{ Name = "Wide310x150Logo.scale-200.png"; W = 620; H = 300 }
)
foreach ($spec in $assetSpecs) {
    Write-ScaledPng $resolvedIcon (Join-Path $assetsDir $spec.Name) $spec.W $spec.H
}

$priConfig = Join-Path $workDir "priconfig.xml"
& $makePri createconfig /cf $priConfig /dq en-US /o
if ($LASTEXITCODE -ne 0) {
    throw "makepri createconfig failed with exit code $LASTEXITCODE"
}

$templateXml = Get-Content -Raw -Encoding UTF8 $templatePath
$bundleInputDir = Join-Path $workDir "bundle-in"
New-Item -ItemType Directory -Force -Path $bundleInputDir | Out-Null

$escName = [System.Security.SecurityElement]::Escape($PackageName)
$escPublisher = [System.Security.SecurityElement]::Escape($Publisher)
$escDisplayName = [System.Security.SecurityElement]::Escape($PublisherDisplayName)

foreach ($target in $targets) {
    $arch = $target.Arch
    $stageDir = Join-Path $workDir "stage-$arch"
    New-Item -ItemType Directory -Force -Path $stageDir | Out-Null
    Copy-Item -Recurse -Force $assetsDir (Join-Path $stageDir "Assets")
    Copy-Item -Force $target.Exe (Join-Path $stageDir "safe-your-eyes.exe")

    $manifest = $templateXml.
        Replace('__PACKAGE_NAME__', $escName).
        Replace('__PACKAGE_PUBLISHER__', $escPublisher).
        Replace('__PACKAGE_VERSION__', $msixVersion).
        Replace('__PUBLISHER_DISPLAY_NAME__', $escDisplayName).
        Replace('__ARCH__', $arch)
    $manifestPath = Join-Path $stageDir "AppxManifest.xml"
    [System.IO.File]::WriteAllText($manifestPath, $manifest, (New-Object System.Text.UTF8Encoding($false)))

    & $makePri new /pr $stageDir /cf $priConfig /mn $manifestPath /of (Join-Path $stageDir "resources.pri") /o
    if ($LASTEXITCODE -ne 0) {
        throw "makepri new ($arch) failed with exit code $LASTEXITCODE"
    }

    $msixName = "safe-your-eyes-$msixVersion-$arch.msix"
    $msixPath = Join-Path $bundleInputDir $msixName
    & $makeAppx pack /d $stageDir /p $msixPath /o
    if ($LASTEXITCODE -ne 0) {
        throw "makeappx pack ($arch) failed with exit code $LASTEXITCODE"
    }
    Copy-Item -Force $msixPath (Join-Path $resolvedOut $msixName)
}

if (-not $SkipBundle) {
    $bundlePath = Join-Path $resolvedOut "safe-your-eyes-$msixVersion.msixbundle"
    & $makeAppx bundle /d $bundleInputDir /p $bundlePath /bv $msixVersion /o
    if ($LASTEXITCODE -ne 0) {
        throw "makeappx bundle failed with exit code $LASTEXITCODE"
    }
    Write-Host "Built Microsoft Store MSIX bundle: $bundlePath"
}

Remove-Item -Recurse -Force $workDir
