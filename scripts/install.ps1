param(
    [string]$InstallDir = $(if ($env:MAIBOT_INSTALL_DIR) { $env:MAIBOT_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\MaiBotManager" }),
    [string]$Version = $env:MAIBOT_VERSION,
    [string]$ForceProxy = $env:MAIBOT_FORCE_PROXY,
    [switch]$NoPathUpdate
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Repo = "WhiteCloudOL/MaiBot-Manager-TUI"
$AssetName = "maibot-manager-windows-x86_64.exe"
$BinaryName = "maibot.exe"
$GithubMirrors = @(
    "https://gh-proxy.org",
    "https://hk.gh-proxy.org",
    "https://cdn.gh-proxy.org",
    "https://ghproxy.net",
    "https://ghfast.top",
    "https://github.moeyy.xyz"
)

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

function Convert-GithubUrl([string]$Url, [string]$Proxy) {
    if ([string]::IsNullOrWhiteSpace($Proxy) -or $Proxy -eq "direct") {
        return $Url
    }
    return "$($Proxy.TrimEnd('/'))/$Url"
}

function Invoke-GithubJson([string]$Url) {
    $headers = @{
        "Accept" = "application/vnd.github+json"
        "User-Agent" = "maibot-manager-installer"
    }
    if ($env:GITHUB_TOKEN) {
        $headers["Authorization"] = "Bearer $($env:GITHUB_TOKEN)"
    }

    $proxies = if ($ForceProxy) { @($ForceProxy) } else { @("direct") + $GithubMirrors }
    foreach ($proxy in $proxies) {
        $requestUrl = Convert-GithubUrl $Url $proxy
        try {
            $json = Invoke-RestMethod -Uri $requestUrl -Headers $headers -TimeoutSec 20
            return [pscustomobject]@{
                Json = $json
                Proxy = $proxy
            }
        } catch {
            Write-Warn "GitHub API request failed: $requestUrl ($($_.Exception.Message))"
        }
    }
    throw "Failed to fetch GitHub release metadata. Set MAIBOT_FORCE_PROXY=https://gh-proxy.org and retry if needed."
}

function Get-ReleaseInfo {
    if ($Version) {
        $url = "https://api.github.com/repos/$Repo/releases/tags/$Version"
        return Invoke-GithubJson $url
    }

    # releases/latest does not return prereleases. Automatic builds are nextdev
    # prereleases, so query the releases list and pick the newest matching asset.
    $url = "https://api.github.com/repos/$Repo/releases?per_page=20"
    $result = Invoke-GithubJson $url
    $release = @($result.Json) |
        Where-Object { -not $_.draft -and (@($_.assets) | Where-Object { $_.name -eq $AssetName }) } |
        Select-Object -First 1
    if (-not $release) {
        throw "No recent release contains Windows asset: $AssetName"
    }
    return [pscustomobject]@{
        Json = $release
        Proxy = $result.Proxy
    }
}

function Add-UserPath([string]$PathToAdd) {
    $current = [Environment]::GetEnvironmentVariable("Path", "User")
    $parts = @()
    if ($current) {
        $parts = $current -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
    }
    if ($parts | Where-Object { $_.TrimEnd('\') -ieq $PathToAdd.TrimEnd('\') }) {
        Write-Ok "User PATH already contains $PathToAdd"
        return
    }
    $newPath = (@($parts) + $PathToAdd) -join ';'
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    Write-Ok "Updated user PATH: $PathToAdd"
}

if (-not [Environment]::Is64BitOperatingSystem) {
    throw "This installer supports only 64-bit Windows 10/11."
}

try {
    Write-Step "MaiBot Manager installer"
    Write-Host "    Install dir: $InstallDir"
    if ($Version) {
        Write-Host "    Version:     $Version"
    } else {
        Write-Host "    Version:     newest release containing $AssetName"
    }
    if ($ForceProxy) {
        Write-Host "    Proxy:       $ForceProxy"
    }

    Write-Step "Fetching release metadata"
    $releaseInfo = Get-ReleaseInfo
    $release = $releaseInfo.Json
    $tag = $release.tag_name
    $asset = @($release.assets) | Where-Object { $_.name -eq $AssetName } | Select-Object -First 1
    if (-not $asset) {
        throw "Asset $AssetName was not found in $tag"
    }

    $downloadUrl = Convert-GithubUrl $asset.browser_download_url $releaseInfo.Proxy
    Write-Step "Downloading $tag"
    Write-Host "    $downloadUrl"

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $tmp = Join-Path ([IO.Path]::GetTempPath()) "maibot-manager-$([Guid]::NewGuid()).exe"
    $dst = Join-Path $InstallDir $BinaryName

    Invoke-WebRequest -Uri $downloadUrl -OutFile $tmp -UseBasicParsing
    $bytes = [System.IO.File]::ReadAllBytes($tmp)
    $header = if ($bytes.Length -ge 2) { $bytes[0..1] } else { $bytes }
    if ($header.Length -lt 2 -or $header[0] -ne 0x4D -or $header[1] -ne 0x5A) {
        throw "Downloaded file is not a valid Windows executable."
    }
    Move-Item -LiteralPath $tmp -Destination $dst -Force

    if (-not $NoPathUpdate) {
        Add-UserPath $InstallDir
    } else {
        Write-Warn "Skipped user PATH update by request."
    }

    Write-Host ""
    Write-Ok "Installed ($tag)"
    Write-Host "  Binary:     $dst"
    Write-Host "  Open TUI:   maibot or maibot tui"
    Write-Host "  Help:       maibot help"
    Write-Host ""
    if (-not $NoPathUpdate) {
        Write-Warn "If this terminal cannot find maibot yet, reopen it or run temporarily:"
        Write-Host "  `$env:Path = `"$InstallDir;`$env:Path`""
    }
} catch {
    Write-Fail $_.Exception.Message
    throw
} finally {
    if ($tmp -and (Test-Path -LiteralPath $tmp)) {
        Remove-Item -LiteralPath $tmp -Force
    }
}
