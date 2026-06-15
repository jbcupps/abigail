<#
.SYNOPSIS
    Build and stage the split Abigail desktop product as a portable, unsigned
    bundle: the two app shells plus the two daemons in one directory.

.DESCRIPTION
    Double-clicking "Abigail Hive" (abigail-hive-app.exe) from the staged
    directory brings up the whole stack with no terminal — the Hive app spawns
    (or adopts) the Hive daemon, which in turn starts the helper and family
    entity-daemons on demand. All inter-process spawning resolves binaries via
    the executable's own directory, so the four binaries must sit side by side.

    This is the unsigned-stabilization path (CLAUDE.md): no NSIS installer, no
    code signing, no updater. Those remain a separate release-only concern. The
    dev launcher (scripts/dev/launch_split_stack.ps1) is unaffected.
#>
param(
    [ValidateSet("debug", "release")]
    [string]$Configuration = "release",
    [string]$OutputDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $OutputDir) {
    $OutputDir = Join-Path $repoRoot "dist\abigail-split"
}

function Build-Frontend {
    param([Parameter(Mandatory = $true)][string]$AppDir)
    $ui = Join-Path $AppDir "src-ui"
    if (-not (Test-Path (Join-Path $ui "package.json"))) {
        return
    }
    Push-Location $ui
    try {
        if (-not (Test-Path "node_modules")) {
            Write-Host "  npm install ($AppDir)"
            & npm install
            if ($LASTEXITCODE -ne 0) { throw "npm install failed in $ui" }
        }
        & npm run build
        if ($LASTEXITCODE -ne 0) { throw "npm run build failed in $ui" }
    } finally {
        Pop-Location
    }
}

Write-Host "Building split-app frontends..."
Build-Frontend -AppDir (Join-Path $repoRoot "hive-app")
Build-Frontend -AppDir (Join-Path $repoRoot "entity-runtime-app")

Write-Host "Building binaries ($Configuration)..."
$cargoArgs = @(
    "build",
    "-p", "hive-daemon",
    "-p", "entity-daemon",
    "-p", "abigail-hive-app",
    "-p", "abigail-entity-runtime-app"
)
if ($Configuration -eq "release") { $cargoArgs += "--release" }
& cargo @cargoArgs
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repoRoot "target" }
$binDir = Join-Path $target $Configuration

$binaries = @(
    "hive-daemon.exe",
    "entity-daemon.exe",
    "abigail-hive-app.exe",
    "abigail-entity-runtime-app.exe"
)

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
foreach ($bin in $binaries) {
    $src = Join-Path $binDir $bin
    if (-not (Test-Path $src)) { throw "Missing built binary: $src" }
    Copy-Item -Path $src -Destination (Join-Path $OutputDir $bin) -Force
}

Write-Host ""
Write-Host "Staged unsigned portable build at: $OutputDir"
Write-Host "Launch by running 'abigail-hive-app.exe' — it starts the whole stack."
Get-ChildItem $OutputDir | Select-Object Name, Length | Format-Table -AutoSize | Out-String | Write-Host
