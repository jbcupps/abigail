# LEGACY: builds the retired single-agent tauri-app, not the Abigail framework
# installer. Use scripts/build-mvp-windows.ps1 for the family-facing product.
# Requires: Rust, Node.js 20+, npm.

$ErrorActionPreference = "Stop"
$LegacyAppDir = (Get-Item $PSScriptRoot).Parent.FullName
$RepoRoot = (Get-Item $LegacyAppDir).Parent.Parent.FullName

Write-Host "Installing frontend deps (legacy/tauri-app/src-ui)..."
Set-Location "$LegacyAppDir\src-ui"
npm install
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Building legacy Tauri app (installer)..."
Set-Location $LegacyAppDir
cargo tauri build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# Generate the Hive master key (idempotent - won't overwrite existing)
Write-Host "Generating Hive master key..."
Set-Location $RepoRoot
cargo run -p abigail-keygen -- --gen-master
# Non-fatal if master key already exists

$BundleNsis = "$LegacyAppDir\target\release\bundle\nsis"
$BundleDir = if (Test-Path $BundleNsis) { $BundleNsis } else { "$LegacyAppDir\target\release\bundle" }

Write-Host "Opening bundle folder: $BundleDir"
Invoke-Item $BundleDir
