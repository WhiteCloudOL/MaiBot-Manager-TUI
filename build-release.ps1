$ErrorActionPreference = "Stop"

$projectRoot = (Resolve-Path (Split-Path -Parent $MyInvocation.MyCommand.Path)).Path
$wslDistro = if ($env:MAIBOT_WSL_DISTRO) { $env:MAIBOT_WSL_DISTRO } else { "Ubuntu-24.04" }

function ConvertTo-BashSingleQuoted([string]$Value) {
    return "'" + $Value.Replace("'", "'\''") + "'"
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

$escapedProjectRoot = ConvertTo-BashSingleQuoted $wslProjectRoot
$bashCommand = "cd $escapedProjectRoot && bash ./build-release.sh"

Write-Host "Using WSL distro: $wslDistro"
wsl -d $wslDistro -- bash -lc $bashCommand
Pause
exit $LASTEXITCODE
