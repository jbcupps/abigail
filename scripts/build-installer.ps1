# Build Abby installer and open the bundle folder.
# Run from repo root. Requires: Rust, Node.js 20+, npm.

$ErrorActionPreference = "Stop"
$RepoRoot = if ($PSScriptRoot) { (Get-Item $PSScriptRoot).Parent.FullName } else { Get-Location }
Set-Location $RepoRoot

Write-Host "Installing frontend deps (tauri-app/src-ui)..."
Set-Location "$RepoRoot\tauri-app\src-ui"
npm install
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "Building Tauri app (installer)..."
Set-Location "$RepoRoot\tauri-app"
cargo tauri build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# Bundle output: workspace target or tauri-app/target
$BundleNsis = "$RepoRoot\target\release\bundle\nsis"
if (-not (Test-Path $BundleNsis)) {
    $BundleNsis = "$RepoRoot\tauri-app\target\release\bundle\nsis"
}
$BundleDir = if (Test-Path $BundleNsis) { $BundleNsis } else { "$RepoRoot\target\release\bundle" }
if (-not (Test-Path $BundleDir)) {
    $BundleDir = "$RepoRoot\tauri-app\target\release\bundle"
}

Write-Host "Opening bundle folder: $BundleDir"
Invoke-Item $BundleDir
