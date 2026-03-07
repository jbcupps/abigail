<#
.SYNOPSIS
    Live Tauri E2E — validates the desktop runtime skill/secrets/instruction
    pipeline by building the real executable and running the probe mode.

.DESCRIPTION
    Builds the Tauri app in release mode, then launches the binary with
    ABIGAIL_E2E_PROBE=1 to exercise the production wiring against a temp
    data directory.

    Exit codes:
      0  = all probe checks passed
      1  = one or more probe checks failed
      2  = build failure

.PARAMETER SkipBuild
    Skip the cargo build step (reuse an existing release binary).

.EXAMPLE
    # Deterministic-only (no external deps):
    .\scripts\tests\live_tauri_skill_secrets_e2e.ps1
#>

[CmdletBinding()]
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path

Write-Host "`n=== Abigail Live Tauri E2E ===" -ForegroundColor Cyan
Write-Host "Repo:  $RepoRoot"
Write-Host "Time:  $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')`n"

# ── Build ───────────────────────────────────────────────────────────
if (-not $SkipBuild) {
    Write-Host "[BUILD] cargo build -p abigail-app --release ..." -ForegroundColor Yellow
    Push-Location $RepoRoot
    try {
        cargo build -p abigail-app --release 2>&1
        if ($LASTEXITCODE -ne 0) {
            Write-Host "[BUILD] FAILED (exit $LASTEXITCODE)" -ForegroundColor Red
            exit 2
        }
        Write-Host "[BUILD] OK" -ForegroundColor Green
    } finally {
        Pop-Location
    }
} else {
    Write-Host "[BUILD] skipped (-SkipBuild)" -ForegroundColor DarkGray
}

# ── Locate binary ──────────────────────────────────────────────────
$binary = Join-Path $RepoRoot 'target\release\abigail-app.exe'
if (-not (Test-Path $binary)) {
    # Tauri names the binary after productName in tauri.conf.json
    $binary = Join-Path $RepoRoot 'target\release\Abigail.exe'
}
if (-not (Test-Path $binary)) {
    Write-Host "[ERROR] Release binary not found at target\release\" -ForegroundColor Red
    exit 2
}
Write-Host "[PROBE] Binary: $binary"

# ── Run probe ──────────────────────────────────────────────────────
$env:ABIGAIL_E2E_PROBE = "1"
$env:RUST_LOG = "abigail_app=debug,abigail_skills=debug"

Write-Host "[PROBE] Launching probe mode ...`n" -ForegroundColor Yellow

& $binary 2>&1 | ForEach-Object { Write-Host $_ }
$probeExit = $LASTEXITCODE

# Clean up env
Remove-Item Env:ABIGAIL_E2E_PROBE -ErrorAction SilentlyContinue
Remove-Item Env:RUST_LOG -ErrorAction SilentlyContinue

# ── Report ─────────────────────────────────────────────────────────
Write-Host ""
if ($probeExit -eq 0) {
    Write-Host "=== E2E PASSED ===" -ForegroundColor Green
} else {
    Write-Host "=== E2E FAILED (exit $probeExit) ===" -ForegroundColor Red
}
exit $probeExit
