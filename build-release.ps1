param(
    [switch]$SkipWindows,
    [switch]$SkipLinux,
    [switch]$PauseAtEnd,
    [string]$WslDistro = $(if ($env:MAIBOT_WSL_DISTRO) { $env:MAIBOT_WSL_DISTRO } else { "Ubuntu-24.04" }),
    [string]$WindowsTargetDir,
    [string]$OutputDir
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$ProjectRoot = (Resolve-Path (Split-Path -Parent $PSCommandPath)).Path
if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $OutputDir = Join-Path $ProjectRoot "output"
}
if ([string]::IsNullOrWhiteSpace($WindowsTargetDir)) {
    # Keep release builds away from the default target dir. The default exe is
    # often what developers run for manual TUI checks, so Cargo may be unable
    # to overwrite it on Windows while the process is still alive.
    $WindowsTargetDir = Join-Path $ProjectRoot "target\build-release-windows"
}

function Write-Step([string]$Message) {
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Write-Ok([string]$Message) {
    Write-Host " OK  $Message" -ForegroundColor Green
}

function Write-Warn([string]$Message) {
    Write-Host " !  $Message" -ForegroundColor Yellow
}

function Write-Fail([string]$Message) {
    Write-Host "ERR  $Message" -ForegroundColor Red
}

function Write-WslLine([object]$Line) {
    $text = if ($Line -is [System.Management.Automation.ErrorRecord]) {
        $Line.Exception.Message
    } else {
        [string]$Line
    }

    $text = $text.Replace("`0", "")
    if ([string]::IsNullOrWhiteSpace($text)) {
        return
    }

    # Some WSL builds emit a UTF-16-ish localized localhost proxy warning on
    # stderr. After PowerShell decodes it as text, it can contain embedded NULs
    # and scramble captured logs. It is informational, not a build failure.
    if ($text -match '^wsl:\s' -and $text -match 'localhost') {
        if (-not $script:FilteredWslLocalhostWarning) {
            Write-Warn "Filtered a WSL localhost proxy warning from the build log."
            $script:FilteredWslLocalhostWarning = $true
        }
        return
    }

    Write-Host $text
}

function ConvertTo-WindowsCommandLineArgument([string]$Value) {
    if ($null -eq $Value -or $Value.Length -eq 0) {
        return '""'
    }
    if ($Value -notmatch '[\s"]') {
        return $Value
    }

    $result = '"'
    $backslashes = 0
    foreach ($char in $Value.ToCharArray()) {
        if ($char -eq '\') {
            $backslashes += 1
        } elseif ($char -eq '"') {
            $result += ('\' * (($backslashes * 2) + 1))
            $result += '"'
            $backslashes = 0
        } else {
            if ($backslashes -gt 0) {
                $result += ('\' * $backslashes)
                $backslashes = 0
            }
            $result += $char
        }
    }
    if ($backslashes -gt 0) {
        $result += ('\' * ($backslashes * 2))
    }
    $result += '"'
    return $result
}

function ConvertTo-BashSingleQuoted([string]$Value) {
    return "'" + $Value.Replace("'", "'\''") + "'"
}

function Invoke-NativeChecked([string]$FilePath, [string[]]$Arguments) {
    Write-Host "    $FilePath $($Arguments -join ' ')" -ForegroundColor DarkGray
    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code $LASTEXITCODE`: $FilePath $($Arguments -join ' ')"
    }
}

function Get-WslProjectRoot([string]$WindowsPath, [string]$Distro) {
    if ($WindowsPath -match '^([A-Za-z]):\\(.*)$') {
        $drive = $Matches[1].ToLowerInvariant()
        $path = $Matches[2].Replace('\', '/')
        return "/mnt/$drive/$path"
    }

    $escapedWindowsPath = $WindowsPath.Replace('\', '\\')
    $mappedPath = (& wsl -d $Distro -- wslpath -a $escapedWindowsPath).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($mappedPath)) {
        throw "Failed to map project path into WSL distro '$Distro': $WindowsPath"
    }
    return $mappedPath
}

function Assert-Artifact([string]$Path, [string]$Label) {
    if (-not (Test-Path -LiteralPath $Path)) {
        throw "Missing expected artifact: $Label ($Path)"
    }
    $item = Get-Item -LiteralPath $Path
    if ($item.Length -le 0) {
        throw "Artifact is empty: $Label ($Path)"
    }
    Write-Ok "$Label -> $Path"
}

function Write-WslCapturedFile([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) {
        return
    }

    $content = [System.IO.File]::ReadAllText($Path, [System.Text.Encoding]::UTF8)
    $content = $content.Replace("`0", "")
    foreach ($line in ($content -split "`r?`n")) {
        Write-WslLine $line
    }
}

function Invoke-WslChecked([string]$Distro, [string]$BashCommand) {
    $stdout = [IO.Path]::GetTempFileName()
    $stderr = [IO.Path]::GetTempFileName()
    try {
        $arguments = @("-d", $Distro, "--", "bash", "-lc", $BashCommand)
        $argumentLine = ($arguments | ForEach-Object { ConvertTo-WindowsCommandLineArgument $_ }) -join " "

        $process = Start-Process `
            -FilePath "wsl.exe" `
            -ArgumentList $argumentLine `
            -RedirectStandardOutput $stdout `
            -RedirectStandardError $stderr `
            -NoNewWindow `
            -Wait `
            -PassThru

        $script:FilteredWslLocalhostWarning = $false
        Write-WslCapturedFile $stderr
        Write-WslCapturedFile $stdout

        if ($process.ExitCode -ne 0) {
            throw "Linux build failed in WSL with exit code $($process.ExitCode)."
        }
    } finally {
        Remove-Item -LiteralPath $stdout -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $stderr -Force -ErrorAction SilentlyContinue
    }
}

function Copy-Artifact([string]$Source, [string]$Destination, [string]$Label) {
    if (-not (Test-Path -LiteralPath $Source)) {
        throw "Missing build output for $Label`: $Source"
    }

    try {
        Copy-Item -LiteralPath $Source -Destination $Destination -Force -ErrorAction Stop
    } catch {
        throw "$Label output is locked and could not be overwritten: $Destination. Close any running copy of this binary and rerun build-release.ps1."
    }

    return $Destination
}

try {
    Write-Step "MaiBot Manager release build"
    Write-Host "    Project: $ProjectRoot"
    Write-Host "    Output:  $OutputDir"

    New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null

    if (-not $SkipWindows) {
        Write-Step "Building Windows x86_64 release"
        $cargo = Get-Command cargo -ErrorAction SilentlyContinue
        $rustup = Get-Command rustup -ErrorAction SilentlyContinue
        if ($cargo -and $rustup) {
            Invoke-NativeChecked "rustup" @("target", "add", "x86_64-pc-windows-msvc")
            Invoke-NativeChecked "cargo" @("fetch", "--locked")
            Invoke-NativeChecked "cargo" @(
                "build",
                "--release",
                "--locked",
                "--target",
                "x86_64-pc-windows-msvc",
                "--target-dir",
                $WindowsTargetDir
            )

            $windowsSource = Join-Path $WindowsTargetDir "x86_64-pc-windows-msvc\release\maibot-manager-tui.exe"
            $windowsTarget = Join-Path $OutputDir "maibot-manager-windows-x86_64.exe"
            $windowsArtifact = Copy-Artifact $windowsSource $windowsTarget "Windows x86_64"
            Assert-Artifact $windowsArtifact "Windows x86_64"
        } else {
            Write-Warn "Skipped Windows build: cargo/rustup not found on Windows PATH. Install Rust from https://rustup.rs/ or rerun with -SkipWindows."
        }
    } else {
        Write-Warn "Skipped Windows build by request."
    }

    if (-not $SkipLinux) {
        Write-Step "Building Linux artifacts through WSL"
        if (-not (Get-Command wsl -ErrorAction SilentlyContinue)) {
            throw "wsl.exe was not found. Install WSL or rerun with -SkipLinux."
        }

        $wslProjectRoot = Get-WslProjectRoot $ProjectRoot $WslDistro
        $escapedProjectRoot = ConvertTo-BashSingleQuoted $wslProjectRoot
        $bashCommand = "cd $escapedProjectRoot && bash ./build-release.sh"

        Write-Host "    WSL distro: $WslDistro"
        Write-Host "    WSL path:   $wslProjectRoot"
        [Console]::Out.Flush()
        Invoke-WslChecked $WslDistro $bashCommand

        foreach ($linuxOutput in @("maibot-manager-linux-x86_64", "maibot-manager-linux-arm64")) {
            Assert-Artifact (Join-Path $OutputDir $linuxOutput) "Linux $linuxOutput"
        }
    } else {
        Write-Warn "Skipped Linux build by request."
    }

    Write-Step "Build complete"
    Get-ChildItem -LiteralPath $OutputDir |
        Sort-Object Name |
        Format-Table Mode, Length, LastWriteTime, Name
} catch {
    Write-Fail $_.Exception.Message
    throw
} finally {
    if ($PauseAtEnd) {
        Read-Host "Press Enter to exit"
    }
}
