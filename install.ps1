# Luna installer (Windows PowerShell).
# Usage: irm https://luna.tngklp.com/install.ps1 | iex
#
# Downloads the latest luna release binary for the TUI from GitHub releases
# and installs it to %LOCALAPPDATA%\Programs\luna, adding it to the user PATH.

$ErrorActionPreference = "Stop"

$Repo = "tngklp/luna"
$InstallDir = if ($env:LUNA_INSTALL_DIR) { $env:LUNA_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\luna" }

Write-Host "Installing Luna (TUI)..." -ForegroundColor Cyan

$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { "x86_64" }
    "ARM64" { "aarch64" }
    default { Write-Error "Unsupported architecture: $env:PROCESSOR_ARCHITECTURE" }
}
$target = "$arch-pc-windows-msvc"
$asset = "luna-$target.zip"
$url = "https://github.com/$Repo/releases/latest/download/$asset"

$tmp = New-Item -ItemType Directory -Force -Path (Join-Path $env:TEMP "luna-install-$(Get-Random)")
try {
    Write-Host "Downloading $url..."
    Invoke-WebRequest -Uri $url -OutFile (Join-Path $tmp $asset) -UseBasicParsing

    Expand-Archive -Path (Join-Path $tmp $asset) -DestinationPath $tmp -Force

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Move-Item -Path (Join-Path $tmp "luna.exe") -Destination (Join-Path $InstallDir "luna.exe") -Force

    # Add to user PATH if missing.
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -notlike "*$InstallDir*") {
        [Environment]::SetEnvironmentVariable("Path", "$userPath;$InstallDir", "User")
        Write-Host "Added $InstallDir to your PATH (restart your terminal to pick it up)." -ForegroundColor Yellow
    }

    Write-Host ""
    Write-Host "Luna installed to $InstallDir\luna.exe" -ForegroundColor Green
    Write-Host "Run 'luna' to start. Configure providers in:"
    Write-Host "  $env:APPDATA\luna\Luna\config.toml"
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
