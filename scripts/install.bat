@echo off
setlocal enabledelayedexpansion

REM ─── Praxis Installer (Windows CMD) ─────────────────────────────────────────
REM This script installs the Praxis CLI tool on Windows.
REM ──────────────────────────────────────────────────────────────────────────────

set "INSTALL_DIR=%USERPROFILE%\.praxis\bin"
set "BUILD_TYPE=release"

REM ── 1. Check for Rust ───────────────────────────────────────────────────────
where cargo >nul 2>nul
if %ERRORLEVEL% neq 0 (
    echo Rust not found. Installing via rustup...
    curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs -o "%TEMP%\rustup-init.exe"
    "%TEMP%\rustup-init.exe" -y
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
) else (
    echo Rust detected
)

REM ── 2. Locate or clone the repository ───────────────────────────────────────
if exist "%~dp0..\Cargo.toml" (
    pushd "%~dp0.."
    set "REPO_DIR=%CD%"
    popd
    echo Using local repository: !REPO_DIR!
) else (
    set "REPO_DIR=%TEMP%\praxis-build-%RANDOM%"
    echo Cloning repository...
    git clone --depth 1 https://github.com/kurosss/praxis.git "!REPO_DIR!"
)

cd /d "!REPO_DIR!"

REM ── 3. Build ────────────────────────────────────────────────────────────────
echo Building Praxis (%BUILD_TYPE%)...
if /i "%BUILD_TYPE%"=="release" (
    cargo build --release --workspace
    set "BINARY_PATH=target\release\praxis.exe"
) else (
    cargo build --workspace
    set "BINARY_PATH=target\debug\praxis.exe"
)

REM ── 4. Install ──────────────────────────────────────────────────────────────
echo Installing to %INSTALL_DIR%...
if not exist "%INSTALL_DIR%" mkdir "%INSTALL_DIR%"
copy "!BINARY_PATH!" "%INSTALL_DIR%\praxis.exe" >nul

REM Also copy API server if built
if exist "target\release\praxis-api-server.exe" (
    copy "target\release\praxis-api-server.exe" "%INSTALL_DIR%\praxis-api-server.exe" >nul
    echo API server also installed
)

REM ── 5. PATH setup ───────────────────────────────────────────────────────────
echo !PATH! | findstr /C:"!INSTALL_DIR!" >nul
if %ERRORLEVEL% neq 0 (
    echo.
    echo Warning: %INSTALL_DIR% is not in your PATH.
    echo Run the following to add it permanently:
    echo.
    echo     setx PATH "%%PATH%%;%INSTALL_DIR%"
    echo.
)

REM ── 6. Verify ───────────────────────────────────────────────────────────────
echo.
if exist "%INSTALL_DIR%\praxis.exe" (
    echo Praxis installed successfully!
    "%INSTALL_DIR%\praxis.exe" --help
)

endlocal
