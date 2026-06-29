<#
.SYNOPSIS
    Build and stage Abigail's split runtime binaries for the one-step installer.

.DESCRIPTION
    The family-facing installer launches Abigail Hive as the single visible app.
    Entity Runtime, hive-daemon, and entity-daemon are bundled as internal
    resources so the user never has to download or place separate binaries.
#>
param(
    [ValidateSet("debug", "release")]
    [string]$Configuration = "release"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$resourceDir = Join-Path $repoRoot "hive-app\resources"

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory
    )

    Push-Location $WorkingDirectory
    try {
        & $FilePath @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "$FilePath $($Arguments -join ' ') failed in $WorkingDirectory"
        }
    } finally {
        Pop-Location
    }
}

function Build-Frontend {
    param(
        [Parameter(Mandatory = $true)][string]$AppDir,
        [Parameter(Mandatory = $true)][string]$Label
    )

    $uiDir = Join-Path $AppDir "src-ui"
    if (-not (Test-Path (Join-Path $uiDir "package.json"))) {
        return
    }

    Write-Host "Installing $Label frontend dependencies..."
    Invoke-Checked -FilePath "npm" -Arguments @("ci") -WorkingDirectory $uiDir
    Write-Host "Building $Label frontend..."
    Invoke-Checked -FilePath "npm" -Arguments @("run", "build") -WorkingDirectory $uiDir
}

Build-Frontend -AppDir (Join-Path $repoRoot "hive-app") -Label "Abigail Hive"
Build-Frontend -AppDir (Join-Path $repoRoot "entity-runtime-app") -Label "Abigail Entity Runtime"

$cargoArgs = @(
    "build",
    "-p", "hive-daemon",
    "-p", "entity-daemon",
    "-p", "abigail-hive-app",
    "-p", "abigail-entity-runtime-app"
)
if ($Configuration -eq "release") {
    $cargoArgs += "--release"
}

Write-Host "Building internal Abigail runtime binaries..."
Invoke-Checked -FilePath "cargo" -Arguments $cargoArgs -WorkingDirectory $repoRoot

$targetRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repoRoot "target" }
$binaryDir = Join-Path $targetRoot $Configuration
$requiredBinaries = @(
    "abigail-entity-runtime-app.exe",
    "hive-daemon.exe",
    "entity-daemon.exe"
)

New-Item -ItemType Directory -Force -Path $resourceDir | Out-Null
foreach ($binary in $requiredBinaries) {
    $source = Join-Path $binaryDir $binary
    if (-not (Test-Path $source)) {
        throw "Missing required installer resource: $source"
    }
    Copy-Item -Path $source -Destination (Join-Path $resourceDir $binary) -Force
}

Write-Host "Staged Abigail installer resources:"
Get-ChildItem $resourceDir -File |
    Where-Object { $_.Name -ne ".gitkeep" } |
    Select-Object Name, Length |
    Format-Table -AutoSize |
    Out-String |
    Write-Host
