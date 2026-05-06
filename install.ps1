# spex installer — Windows (PowerShell)
#
# Usage (run in PowerShell):
#   iwr -useb https://github.com/johangm90/spex/releases/latest/download/install.ps1 | iex
#
# Or save and run:
#   Set-ExecutionPolicy Bypass -Scope Process -Force
#   .\install.ps1
#
# Options (pass via env before running):
#   $env:SPEX_VERSION = "v0.6.0"   # pin a specific version (default: latest)
#   $env:SPEX_INSTALL_DIR = "C:\tools\spex"  # override install directory
#
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

$REPO        = "johangm90/spex"
$BINARY      = "spex.exe"
$INSTALL_DIR = if ($env:SPEX_INSTALL_DIR) { $env:SPEX_INSTALL_DIR } else { "$env:LOCALAPPDATA\spex" }

function Write-Info { Write-Host "[spex] $args" -ForegroundColor Cyan }
function Write-Ok   { Write-Host "[spex] $args" -ForegroundColor Green }
function Write-Err  { Write-Host "[spex] ERROR: $args" -ForegroundColor Red; exit 1 }

# ── Detect architecture ───────────────────────────────────────────────────────

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
$TARGET = switch ($arch) {
    'X64'   { "x86_64-pc-windows-msvc" }
    'Arm64' { "aarch64-pc-windows-msvc" }
    default { Write-Err "Unsupported architecture: $arch" }
}

Write-Info "Detected architecture: $TARGET"

# ── Resolve version ───────────────────────────────────────────────────────────

if ($env:SPEX_VERSION) {
    $TAG = $env:SPEX_VERSION
    Write-Info "Using pinned version: $TAG"
} else {
    Write-Info "Fetching latest release..."
    try {
        $release = Invoke-RestMethod "https://api.github.com/repos/$REPO/releases/latest" `
            -Headers @{ "User-Agent" = "spex-installer" }
        $TAG = $release.tag_name
    } catch {
        Write-Err "Could not fetch release info: $_"
    }
    Write-Info "Latest release: $TAG"
}

# ── Download ──────────────────────────────────────────────────────────────────

$ASSET         = "spex-$TAG-$TARGET.zip"
$DOWNLOAD_URL  = "https://github.com/$REPO/releases/download/$TAG/$ASSET"
$CHECKSUM_URL  = "$DOWNLOAD_URL.sha256"

$TMP = Join-Path $env:TEMP "spex-install-$([System.IO.Path]::GetRandomFileName())"
New-Item -ItemType Directory -Force -Path $TMP | Out-Null

$ZIP_PATH      = Join-Path $TMP $ASSET
$CHECKSUM_PATH = Join-Path $TMP "$ASSET.sha256"

Write-Info "Downloading $ASSET..."
Invoke-WebRequest -Uri $DOWNLOAD_URL -OutFile $ZIP_PATH -UseBasicParsing

Write-Info "Verifying checksum..."
Invoke-WebRequest -Uri $CHECKSUM_URL -OutFile $CHECKSUM_PATH -UseBasicParsing

$EXPECTED = (Get-Content $CHECKSUM_PATH -Raw).Trim().Split()[0].ToLower()
$ACTUAL   = (Get-FileHash $ZIP_PATH -Algorithm SHA256).Hash.ToLower()

if ($EXPECTED -ne $ACTUAL) {
    Write-Err "Checksum mismatch!`n  expected: $EXPECTED`n  got:      $ACTUAL"
}
Write-Info "Checksum OK."

# ── Install ───────────────────────────────────────────────────────────────────

Expand-Archive -Path $ZIP_PATH -DestinationPath $TMP -Force

New-Item -ItemType Directory -Force -Path $INSTALL_DIR | Out-Null

$EXTRACTED_EXE = Join-Path $TMP "spex-$TAG-$TARGET\$BINARY"
Copy-Item $EXTRACTED_EXE (Join-Path $INSTALL_DIR $BINARY) -Force

Write-Info "Binary installed to $INSTALL_DIR\$BINARY"

# ── Add to PATH (user scope, no admin required) ───────────────────────────────

$USER_PATH = [System.Environment]::GetEnvironmentVariable("PATH", "User")
if ($USER_PATH -notlike "*$INSTALL_DIR*") {
    [System.Environment]::SetEnvironmentVariable(
        "PATH", "$USER_PATH;$INSTALL_DIR", "User"
    )
    Write-Info "Added $INSTALL_DIR to user PATH."
    Write-Info "Restart your terminal (or run: `$env:PATH += ';$INSTALL_DIR') to use spex now."
} else {
    Write-Info "$INSTALL_DIR is already in PATH."
}

# ── Cleanup ───────────────────────────────────────────────────────────────────

Remove-Item -Recurse -Force $TMP -ErrorAction SilentlyContinue

# ── Done ──────────────────────────────────────────────────────────────────────

Write-Ok ""
Write-Ok "spex $TAG installed to $INSTALL_DIR\$BINARY"
Write-Ok ""
Write-Ok "Get started:"
Write-Ok "  spex setup        # install agent skills (one-time)"
Write-Ok "  spex init         # initialise spex in an existing project"
Write-Ok "  spex new myapp    # create a new project"
