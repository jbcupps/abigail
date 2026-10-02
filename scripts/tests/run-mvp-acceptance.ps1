<#
.SYNOPSIS
Run isolated black-box contracts against built or installed MVP daemons.
.DESCRIPTION
Uses a synthetic local HTTP model fixture. This proves daemon wiring, chat
streaming, scoped leases, and durable transcripts; it is not real-model UAT.
Only the script's own Hive process tree is terminated during cleanup.
#>
[CmdletBinding()]
param(
    [string]$BinaryDir,
    [string]$OutputDir
)
$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
if (-not $BinaryDir) { $BinaryDir = Join-Path $repoRoot 'target/debug' }
if (-not $OutputDir) { $OutputDir = Join-Path $repoRoot 'target/manual-test/mvp-acceptance' }
$nodeArgs = @((Join-Path $PSScriptRoot 'mvp-acceptance.mjs'), '--binary-dir', $BinaryDir, '--output-dir', $OutputDir)
& node @nodeArgs
if ($LASTEXITCODE -ne 0) { throw "MVP contract acceptance failed (exit $LASTEXITCODE). See $OutputDir/latest.json." }
