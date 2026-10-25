#Requires -Version 5.1
<#
.SYNOPSIS
    Interactive installer for the Luna terminal interface.

.DESCRIPTION
    Downloads a prebuilt `luna` binary from GitHub Releases, verifies its
    checksum, installs it under the user's local app data, adds it to PATH, and
    walks through provider setup.

    No Rust toolchain is required.

.PARAMETER Version
    Release tag to install, e.g. `v0.1.0`. Defaults to the latest release.

.PARAMETER InstallDir
    Where to put the binary. Defaults to %LOCALAPPDATA%\Luna\bin.

.PARAMETER Yes
    Accept every default and skip the prompts. Useful for automation.

.PARAMETER NoPath
    Do not modify PATH.

.EXAMPLE
    irm https://raw.githubusercontent.com/tngklp/luna/main/install.ps1 | iex

.EXAMPLE
    .\install.ps1 -Version v0.1.0 -Yes
#>
[CmdletBinding()]
param(
    [string]$Version = '',
    [string]$InstallDir = '',
    [switch]$Yes,
    [switch]$NoPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Repo = 'tngklp/luna'
$BinaryName = 'luna.exe'

# ---------------------------------------------------------------- presentation

function Write-Step([string]$Message) {
    Write-Host ''
    Write-Host "  $Message" -ForegroundColor Cyan
}

function Write-Ok([string]$Message) {
    Write-Host "  ✓ $Message" -ForegroundColor Green
}

function Write-Warn([string]$Message) {
    Write-Host "  ! $Message" -ForegroundColor Yellow
}

function Write-Fail([string]$Message) {
    Write-Host "  ✗ $Message" -ForegroundColor Red
}

function Write-Banner {
    Write-Host ''
    Write-Host '   ██╗   ██╗███╗   ██╗███╗   ██╗ █████╗ ' -ForegroundColor Magenta
    Write-Host '   ██║   ██║████╗  ██║████╗  ██║██╔══██╗' -ForegroundColor Magenta
    Write-Host '   ██║   ██║██╔██╗ ██║██╔██╗ ██║███████║' -ForegroundColor Magenta
    Write-Host '   ██║   ██║██║╚██╗██║██║╚██╗██║██╔══██║' -ForegroundColor Magenta
    Write-Host '   ╚██████╔╝██║ ╚████║██║ ╚████║██║  ██║' -ForegroundColor Magenta
    Write-Host '    ╚═════╝ ╚═╝  ╚═══╝╚═╝  ╚═══╝╚═╝  ╚═╝' -ForegroundColor Magenta
    Write-Host ''
    Write-Host '   AI coding agent — terminal installer' -ForegroundColor DarkGray
    Write-Host ''
}

# -------------------------------------------------------------------- prompting

function Read-Choice {
    param(
        [string]$Prompt,
        [string[]]$Options,
        [int]$Default = 0
    )
    Write-Host ''
    Write-Host "  $Prompt" -ForegroundColor White
    for ($i = 0; $i -lt $Options.Count; $i++) {
        $marker = if ($i -eq $Default) { '*' } else { ' ' }
        Write-Host "   $marker $($i + 1). $($Options[$i])"
    }
    if ($Yes) {
        Write-Host "  → $($Options[$Default]) (default)" -ForegroundColor DarkGray
        return $Default
    }
    while ($true) {
        $answer = Read-Host "  Choice [$($Default + 1)]"
        if ([string]::IsNullOrWhiteSpace($answer)) { return $Default }
        $parsed = 0
        if ([int]::TryParse($answer, [ref]$parsed) -and $parsed -ge 1 -and $parsed -le $Options.Count) {
            return $parsed - 1
        }
        Write-Warn "Enter a number between 1 and $($Options.Count)."
    }
}

function Read-Text {
    param([string]$Prompt, [string]$Default = '')
    if ($Yes) { return $Default }
    if ([string]::IsNullOrWhiteSpace($Default)) {
        return (Read-Host "  $Prompt")
    }
    $answer = Read-Host "  $Prompt [$Default]"
    if ([string]::IsNullOrWhiteSpace($answer)) { return $Default }
    return $answer
}

function Read-Secret {
    param([string]$Prompt)
    if ($Yes) { return '' }
    $secure = Read-Host "  $Prompt" -AsSecureString
    $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
    try {
        return [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
    } finally {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)
    }
}

# --------------------------------------------------------------------- platform

function Get-TargetTriple {
    $arch = $env:PROCESSOR_ARCHITECTURE
    switch ($arch) {
        'AMD64' { return 'x86_64-pc-windows-msvc' }
        'ARM64' { return 'aarch64-pc-windows-msvc' }
        'x86'   { throw 'Luna does not ship a 32-bit Windows build.' }
        default { throw "Unsupported architecture: $arch" }
    }
}

function Resolve-Version {
    if (-not [string]::IsNullOrWhiteSpace($Version)) {
        if ($Version -notmatch '^v') { return "v$Version" }
        return $Version
    }
    Write-Host '  Looking up the latest release…' -ForegroundColor DarkGray
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" `
        -Headers @{ 'User-Agent' = 'luna-installer' }
    return $release.tag_name
}

function Get-ArchiveName([string]$Triple) {
    return "luna-$Triple.zip"
}

# --------------------------------------------------------------------- download

function Install-Luna {
    $triple = Get-TargetTriple
    $tag = Resolve-Version
    Write-Ok "Target: $triple"
    Write-Ok "Version: $tag"

    $archive = Get-ArchiveName $triple
    $base = "https://github.com/$Repo/releases/download/$tag"
    $temp = Join-Path ([IO.Path]::GetTempPath()) ("luna-install-" + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $temp | Out-Null

    try {
        Write-Step "Downloading $archive"
        $archivePath = Join-Path $temp $archive
        Invoke-WebRequest -Uri "$base/$archive" -OutFile $archivePath -UseBasicParsing
        Write-Ok "Downloaded $([math]::Round((Get-Item $archivePath).Length / 1MB, 2)) MB"

        # Verify the checksum when the release publishes one.
        Write-Step 'Verifying the download'
        $checksums = Join-Path $temp 'checksums.txt'
        $verified = $false
        try {
            Invoke-WebRequest -Uri "$base/checksums.txt" -OutFile $checksums -UseBasicParsing
            $expected = $null
            foreach ($line in Get-Content $checksums) {
                $parts = $line -split '\s+', 2
                if ($parts.Count -eq 2 -and $parts[1].Trim() -eq $archive) {
                    $expected = $parts[0].Trim().ToLowerInvariant()
                    break
                }
            }
            if ($expected) {
                $actual = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
                if ($actual -ne $expected) {
                    throw "Checksum mismatch: expected $expected, got $actual"
                }
                $verified = $true
                Write-Ok 'Checksum verified'
            } else {
                Write-Warn 'No checksum published for this archive'
            }
        } catch [System.Net.WebException] {
            Write-Warn 'checksums.txt is not available for this release'
        }
        if (-not $verified) {
            Write-Warn 'Continuing without checksum verification'
        }

        Write-Step 'Extracting'
        Expand-Archive -Path $archivePath -DestinationPath $temp -Force
        $binary = Get-ChildItem -Path $temp -Filter $BinaryName -Recurse | Select-Object -First 1
        if (-not $binary) { throw "$BinaryName was not found in the archive" }

        if ([string]::IsNullOrWhiteSpace($InstallDir)) {
            $InstallDir = Join-Path $env:LOCALAPPDATA 'Luna\bin'
        }
        New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
        $target = Join-Path $InstallDir $BinaryName
        Copy-Item -Path $binary.FullName -Destination $target -Force
        Write-Ok "Installed to $target"

        if (-not $NoPath) {
            Add-ToPath $InstallDir
        }

        Write-Step 'Verifying the install'
        $reported = & $target --version 2>&1
        if ($LASTEXITCODE -eq 0) {
            Write-Ok "luna $reported"
        } else {
            Write-Warn "The binary did not report a version: $reported"
        }

        return $target
    } finally {
        Remove-Item -Recurse -Force $temp -ErrorAction SilentlyContinue
    }
}

function Add-ToPath([string]$Directory) {
    $current = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ([string]::IsNullOrWhiteSpace($current)) {
        [Environment]::SetEnvironmentVariable('Path', $Directory, 'User')
        Write-Ok 'Added to PATH'
        return
    }
    $entries = $current -split ';' | Where-Object { $_ -ne '' }
    if ($entries -contains $Directory) {
        Write-Ok 'Already on PATH'
        return
    }
    [Environment]::SetEnvironmentVariable('Path', ($current.TrimEnd(';') + ';' + $Directory), 'User')
    Write-Ok 'Added to PATH (open a new terminal to use it)'
}

# --------------------------------------------------------------------- provider

function Configure-Provider {
    Write-Step 'Provider setup'
    Write-Host '  Luna needs one model provider. A local model needs no key.' -ForegroundColor DarkGray

    $choice = Read-Choice -Prompt 'Which provider will you use?' -Options @(
        'Anthropic (Claude)',
        'OpenAI (GPT)',
        'Google Gemini',
        'Local (Ollama or llama.cpp)',
        'Skip for now'
    ) -Default 0

    # Must match what `directories::ProjectDirs` resolves to, which is where the
    # app looks: %APPDATA%\luna\config\config.toml on Windows.
    $configDir = Join-Path $env:APPDATA 'luna\config'
    $configPath = Join-Path $configDir 'config.toml'
    New-Item -ItemType Directory -Force -Path $configDir | Out-Null

    $provider = switch ($choice) {
        0 { 'anthropic' }
        1 { 'openai' }
        2 { 'gemini' }
        3 { 'local' }
        default { '' }
    }

    if ($provider -eq '') {
        Write-Warn "Skipped. Set an API key later, or edit $configPath"
        return
    }

    $envVar = switch ($provider) {
        'anthropic' { 'ANTHROPIC_API_KEY' }
        'openai'    { 'OPENAI_API_KEY' }
        'gemini'    { 'GEMINI_API_KEY' }
        default     { '' }
    }

    if ($envVar -ne '') {
        $existing = [Environment]::GetEnvironmentVariable($envVar, 'User')
        if (-not [string]::IsNullOrWhiteSpace($existing)) {
            Write-Ok "$envVar is already set"
        } else {
            $key = Read-Secret "Paste your $envVar (input is hidden, blank to skip)"
            if (-not [string]::IsNullOrWhiteSpace($key)) {
                [Environment]::SetEnvironmentVariable($envVar, $key, 'User')
                Write-Ok "$envVar saved for your user"
            } else {
                Write-Warn "No key saved. Set $envVar before running luna."
            }
        }
    } else {
        $url = Read-Text -Prompt 'Local server URL' -Default 'http://localhost:11434/v1'
        Write-Ok "Using $url"
    }

    # Write a minimal config so the first run does not have to guess.
    if (-not (Test-Path $configPath)) {
        $defaultModel = switch ($provider) {
            'anthropic' { 'anthropic/claude-sonnet-4-5' }
            'openai'    { 'openai/gpt-5' }
            'gemini'    { 'gemini/gemini-2.5-pro' }
            default     { 'local/llama3.2' }
        }
        $lines = @(
            '[general]',
            "default_model = `"$defaultModel`"",
            'max_concurrent_agents = 4',
            'max_steps = 60',
            '',
            '[approvals]',
            'read = "allow"',
            'write = "ask"',
            'bash = "ask"',
            'network = "allow"'
        )
        Set-Content -Path $configPath -Value ($lines -join "`n") -Encoding UTF8
        Write-Ok "Wrote $configPath"
    } else {
        Write-Ok "Kept the existing $configPath"
    }
}

# ------------------------------------------------------------------------- main

Write-Banner

Write-Step 'Checking your system'
$PSVersionTable.PSVersion | Out-Null
Write-Ok "PowerShell $($PSVersionTable.PSVersion)"
Write-Ok "Windows $([Environment]::OSVersion.Version)"

$installed = Install-Luna
Configure-Provider

Write-Host ''
Write-Host '  ─────────────────────────────────────────────' -ForegroundColor DarkGray
Write-Ok 'Luna is installed'
Write-Host ''
Write-Host '  Start it with:' -ForegroundColor White
Write-Host '    luna' -ForegroundColor Cyan
Write-Host ''
Write-Host '  If the command is not found, open a new terminal.' -ForegroundColor DarkGray
Write-Host '  Want the desktop app too? Install the Luna setup from:' -ForegroundColor DarkGray
Write-Host "    https://github.com/$Repo/releases/latest" -ForegroundColor DarkGray
Write-Host ''
