param(
    [switch]$SkipWindows,
    [switch]$SkipLinux
)

$ErrorActionPreference = "Stop"

$projectRoot = (Resolve-Path (Split-Path -Parent $MyInvocation.MyCommand.Path)).Path
$wslDistro = if ($env:MAIBOT_WSL_DISTRO) { $env:MAIBOT_WSL_DISTRO } else { "Ubuntu-24.04" }

function ConvertTo-BashSingleQuoted([string]$Value) {
    return "'" + $Value.Replace("'", "'\''") + "'"
}

function Invoke-NativeChecked([string]$FilePath, [string[]]$Arguments) {
    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed ($LASTEXITCODE): $FilePath $($Arguments -join ' ')"
    }
}

if ($projectRoot -match '^([A-Za-z]):\\(.*)$') {
    $drive = $Matches[1].ToLowerInvariant()
    $path = $Matches[2].Replace('\', '/')
    $wslProjectRoot = "/mnt/$drive/$path"
} else {
    $escapedWindowsPath = $projectRoot.Replace('\', '\\')
    $wslProjectRoot = (wsl -d $wslDistro -- wslpath -a $escapedWindowsPath).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($wslProjectRoot)) {
        throw "Failed to map project path into WSL distro '$wslDistro'."
    }
}

$outputDir = Join-Path $projectRoot "output"
New-Item -ItemType Directory -Force -Path $outputDir | Out-Null

if (-not $SkipWindows) {
    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    $rustup = Get-Command rustup -ErrorAction SilentlyContinue
    if ($cargo -and $rustup) {
        Write-Host "Building Windows x86_64 release..."
        Invoke-NativeChecked "rustup" @("target", "add", "x86_64-pc-windows-msvc")
        Invoke-NativeChecked "cargo" @("fetch", "--locked")
        Invoke-NativeChecked "cargo" @("build", "--release", "--locked", "--target", "x86_64-pc-windows-msvc")
        Copy-Item `
            -LiteralPath (Join-Path $projectRoot "target\x86_64-pc-windows-msvc\release\maibot-manager-tui.exe") `
            -Destination (Join-Path $outputDir "maibot-manager-windows-x86_64.exe") `
            -Force
    } else {
        Write-Warning "Skipped Windows build: cargo/rustup not found on Windows PATH. Install Rust from https://rustup.rs/ or rerun with -SkipWindows."
    }
}

if (-not $SkipLinux) {
    $escapedProjectRoot = ConvertTo-BashSingleQuoted $wslProjectRoot
    $bashCommand = "cd $escapedProjectRoot && bash ./build-release.sh"

    Write-Host "Using WSL distro: $wslDistro"
    wsl -d $wslDistro -- bash -lc $bashCommand
    if ($LASTEXITCODE -ne 0) {
        exit $LASTEXITCODE
    }
}

Write-Host "Build complete:"
Get-ChildItem -LiteralPath $outputDir | Format-Table Mode, Length, LastWriteTime, Name
Pause
