<#
.SYNOPSIS
    Install the Praxis CLI tool on Windows.
.DESCRIPTION
    Checks for Rust, clones/builds the project, and installs the binary.
.PARAMETER InstallDir
    Directory where binaries will be installed (default: ~\.praxis\bin).
.PARAMETER BuildType
    Build configuration: "release" or "debug" (default: release).
#>

param(
    [string]$InstallDir = "$env:USERPROFILE\.praxis\bin",
    [ValidateSet("release", "debug")]
    [string]$BuildType = "release"
)

$ErrorActionPreference = "Stop"

# ── 1. Check for Rust ────────────────────────────────────────────────────────
if (-not (Get-Command "cargo" -ErrorAction SilentlyContinue)) {
    Write-Host "→ Rust not found. Installing via rustup..." -ForegroundColor Yellow
    Invoke-WebRequest -Uri "https://sh.rustup.rs" -OutFile "$env:TEMP\rustup-init.exe"
    Start-Process -Wait -FilePath "$env:TEMP\rustup-init.exe" -ArgumentList "-y"
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
} else {
    Write-Host "✓ Rust detected: $(cargo --version)" -ForegroundColor Green
}

# ── 2. Locate or clone the repository ────────────────────────────────────────
$repoDir = if (Test-Path "$PSScriptRoot\..\Cargo.toml") {
    (Get-Item "$PSScriptRoot\..").FullName
    Write-Host "✓ Using local repository: $_" -ForegroundColor Green
} else {
    $tmpDir = "$env:TEMP\praxis-build-$([System.IO.Path]::GetRandomFileName())"
    Write-Host "→ Cloning repository..." -ForegroundColor Yellow
    git clone --depth 1 "https://github.com/kurosss/praxis.git" $tmpDir
    $tmpDir
}

Push-Location $repoDir
try {
    # ── 3. Build ──────────────────────────────────────────────────────────────
    $configFlag = if ($BuildType -eq "release") { "--release" } else { "" }
    Write-Host "→ Building Praxis ($BuildType)..." -ForegroundColor Yellow
    cargo build --workspace $configFlag

    $binaryDir = if ($BuildType -eq "release") { "target\release" } else { "target\debug" }
    $binaryPath = Join-Path $binaryDir "praxis.exe"

    # ── 4. Install ────────────────────────────────────────────────────────────
    Write-Host "→ Installing to $InstallDir..." -ForegroundColor Yellow
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item $binaryPath "$InstallDir\praxis.exe" -Force

    $apiServerPath = Join-Path $binaryDir "praxis-api-server.exe"
    if (Test-Path $apiServerPath) {
        Copy-Item $apiServerPath "$InstallDir\praxis-api-server.exe" -Force
        Write-Host "✓ API server also installed" -ForegroundColor Green
    }

    # ── 5. PATH setup ────────────────────────────────────────────────────────
    $userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($userPath -notlike "*$InstallDir*") {
        Write-Host ""
        Write-Host "⚠  $InstallDir is not in your PATH." -ForegroundColor Yellow
        Write-Host "   Run the following to add it permanently:"
        Write-Host ""
        Write-Host "     [Environment]::SetEnvironmentVariable('PATH', `"`$env:PATH;$InstallDir`", 'User')"
        Write-Host ""
    }

    # ── 6. Verify ────────────────────────────────────────────────────────────
    Write-Host ""
    if (Get-Command "praxis" -ErrorAction SilentlyContinue) {
        Write-Host "✓ Praxis installed successfully!" -ForegroundColor Green
        & praxis --help
    } else {
        Write-Host "✓ Praxis installed to $InstallDir\praxis.exe" -ForegroundColor Green
        Write-Host "  Restart your shell or run: `$env:Path += ';$InstallDir'"
    }
} finally {
    Pop-Location
}
