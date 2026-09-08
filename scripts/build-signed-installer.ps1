# Compatibility name only; all behavior lives in the canonical underscore script.
param(
    [Parameter(Mandatory)][ValidateSet('Prepare','Check','Sign')][string]$Phase,
    [ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version = '0.0.75',
    [switch]$SkipStage
)
$ErrorActionPreference = 'Stop'
& (Join-Path $PSScriptRoot 'build_signed_installer.ps1') -Phase $Phase -Version $Version -SkipStage:$SkipStage
